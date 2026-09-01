//! 作業ツリーの監視。変更が続いている間はまとめ、落ち着いてから 1 回だけ通知する。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::task::AppEvent;

/// 変更が止まってからこれだけ待って通知する。
const QUIET: Duration = Duration::from_millis(250);
/// 変更が続いていても、これを超えたら一度通知する。
const MAX_WAIT: Duration = Duration::from_millis(1000);

/// 監視のハンドル。落とすと監視が止まるため、アプリの実行中は保持する。
pub struct FsWatcher {
    _watcher: RecommendedWatcher,
}

/// 作業ツリーの監視を始める。通知は `AppEvent::FsChanged` として届く。
pub fn spawn(root: &Path, out: Sender<AppEvent>) -> Result<FsWatcher> {
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx).context("ファイル監視を開始できない")?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("{} を監視できない", root.display()))?;

    let filter = Filter::new(root);
    std::thread::spawn(move || debounce(&filter, &rx, &out));
    Ok(FsWatcher { _watcher: watcher })
}

/// 静まるまでイベントをまとめ、1 回だけ通知する。
fn debounce(filter: &Filter, rx: &Receiver<notify::Result<Event>>, out: &Sender<AppEvent>) {
    loop {
        // 最初の 1 件はいくら待ってもよい
        let Ok(first) = rx.recv() else {
            return;
        };
        if !filter.is_relevant(&first) {
            continue;
        }
        let started = Instant::now();
        loop {
            match rx.recv_timeout(QUIET) {
                Ok(_) if started.elapsed() < MAX_WAIT => {}
                Ok(_) => break,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if out.send(AppEvent::FsChanged).is_err() {
            return;
        }
    }
}

/// 監視対象外のイベントを落とす。
struct Filter {
    root: PathBuf,
    ignore: Gitignore,
}

impl Filter {
    fn new(root: &Path) -> Self {
        // ルート直下の .gitignore と除外設定のみを見る。入れ子の .gitignore は
        // 拾えないが、取りこぼしても余分な更新が 1 回増えるだけで害はない。
        let mut builder = GitignoreBuilder::new(root);
        builder.add(root.join(".gitignore"));
        builder.add(root.join(".git/info/exclude"));
        let ignore = builder.build().unwrap_or_else(|_| Gitignore::empty());
        Self {
            root: root.to_path_buf(),
            ignore,
        }
    }

    fn is_relevant(&self, event: &notify::Result<Event>) -> bool {
        let Ok(event) = event else {
            return false;
        };
        event.paths.iter().any(|path| self.watches(path))
    }

    fn watches(&self, path: &Path) -> bool {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return false;
        };
        // .git の中は git 自身の操作で頻繁に変わるため見ない
        if rel.components().any(|c| c.as_os_str() == ".git") {
            return false;
        }
        !self
            .ignore
            .matched_path_or_any_parents(path, path.is_dir())
            .is_ignore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_directory_and_ignored_paths_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        let filter = Filter::new(root);

        assert!(filter.watches(&root.join("src/main.rs")));
        assert!(filter.watches(&root.join(".gitignore")));
        assert!(!filter.watches(&root.join(".git/index")));
        assert!(!filter.watches(&root.join("target/debug/tdv")));
        assert!(!filter.watches(&root.join("build.log")));
        // リポジトリ外のパスは無視する
        assert!(!filter.watches(Path::new("/tmp/other")));
    }
}
