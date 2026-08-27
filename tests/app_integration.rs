//! 実際の Git リポジトリを作り、アプリの状態遷移と描画結果を検証する。

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tdv::app::state::{App, ContentState, Focus, Mode};
use tdv::app::{action::Action, update};
use tdv::config::Config;
use tdv::git::{GitBackend, gix_backend::GixBackend};
use tdv::task::{AppEvent, Pool, WorkerCtx};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git を実行できない");
    assert!(
        status.status.success(),
        "git {args:?} が失敗: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

fn numbered(count: usize) -> String {
    (1..=count)
        .map(|i| format!("line {i}\n"))
        .collect::<String>()
}

/// 20 行のファイルを 1 行だけ変更したリポジトリ、および追加 / 削除 / 未追跡を用意する。
fn setup_repo(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("src/main.rs"), numbered(20)).unwrap();
    std::fs::write(root.join("src/removed.rs"), "old\n").unwrap();
    std::fs::write(root.join("README.md"), "readme\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);

    let modified = numbered(20).replace("line 10\n", "line 10 changed\n");
    std::fs::write(root.join("src/main.rs"), modified).unwrap();
    std::fs::remove_file(root.join("src/removed.rs")).unwrap();
    std::fs::write(root.join("src/added.rs"), "added\n").unwrap();
    git(root, &["add", "src/added.rs"]);
    std::fs::write(root.join("untracked.txt"), "untracked\n").unwrap();
}

struct Harness {
    app: App,
    rx: Receiver<AppEvent>,
    terminal: Terminal<TestBackend>,
}

impl Harness {
    fn new(root: &Path, width: u16, height: u16) -> Self {
        let backend: Arc<dyn GitBackend> =
            Arc::new(GixBackend::discover(root).expect("リポジトリを開けない"));
        let cfg = Config::default();
        let (tx, rx) = channel::<AppEvent>();
        let ctx = Arc::new(WorkerCtx {
            backend: Some(Arc::clone(&backend)),
            max_file_bytes: cfg.max_file_bytes,
        });
        let pool = Pool::spawn(2, ctx, tx);
        let app = App::new(cfg, root.to_path_buf(), Some(backend), pool);
        let terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        Self { app, rx, terminal }
    }

    /// 条件が満たされるまでワーカーからの結果を処理する。
    fn pump_until(&mut self, label: &str, cond: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond(&self.app) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "{label} がタイムアウトした");
            match self.rx.recv_timeout(remaining) {
                Ok(AppEvent::Task(result)) => update::on_task(&mut self.app, result),
                Ok(AppEvent::Input(_)) => {}
                Err(_) => panic!("{label} の待機中にチャネルが閉じた"),
            }
        }
    }

    fn act(&mut self, action: Action) {
        update::apply(&mut self.app, action);
    }

    fn render(&mut self) -> String {
        // TestBackend のバッファは差分適用のため、全角文字の裏に前フレームが残る。
        // 検証では毎回クリアして全面再描画させる
        self.terminal.clear().unwrap();
        let app = &mut self.app;
        self.terminal
            .draw(|frame| tdv::ui::draw(frame, app))
            .unwrap();
        let buffer = self.terminal.backend().buffer().clone();
        let area = *buffer.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer.cell((x, y)).map_or(" ", |c| c.symbol()).to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn status_detects_all_change_kinds() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());

    let kinds: Vec<(String, String)> = h
        .app
        .changes
        .files
        .iter()
        .map(|c| (c.path.display().to_string(), format!("{:?}", c.kind)))
        .collect();

    let find = |p: &str| {
        kinds
            .iter()
            .find(|(path, _)| path == p)
            .map(|(_, k)| k.as_str())
    };
    assert_eq!(find("src/main.rs"), Some("Modified"), "{kinds:?}");
    assert_eq!(find("src/added.rs"), Some("Added"), "{kinds:?}");
    assert_eq!(find("src/removed.rs"), Some("Deleted"), "{kinds:?}");
    assert_eq!(find("untracked.txt"), Some("Untracked"), "{kinds:?}");
    assert_eq!(find("README.md"), None, "変更のないファイルは含めない");
}

#[test]
fn diff_view_keeps_the_whole_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());

    h.act(Action::SetMode(Mode::Diff));
    h.pump_until("差分の計算", |app| {
        matches!(&app.content, ContentState::Diff(_))
    });
    // 先頭の変更ファイル (README.md は変更なしのため src/added.rs が先頭)
    while h
        .app
        .content
        .path()
        .is_none_or(|p| p != Path::new("src/main.rs"))
    {
        h.act(Action::NextFile);
        h.pump_until("差分の計算", |app| {
            matches!(&app.content, ContentState::Diff(_))
        });
    }

    let ContentState::Diff(view) = &h.app.content else {
        panic!("差分が表示されていない");
    };
    // 変更は 1 行だが、20 行すべてが行ペアとして残る
    assert_eq!(view.diff.rows.len(), 20, "全文が保持されていない");
    assert_eq!(view.diff.hunks.len(), 1);
    assert_eq!(view.diff.stat.added, 1);
    assert_eq!(view.diff.stat.removed, 1);
}

#[test]
fn diff_render_shows_both_sides_and_distant_context() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());
    h.act(Action::SetMode(Mode::Diff));
    h.pump_until("差分の計算", |app| {
        matches!(&app.content, ContentState::Diff(_))
    });
    while h
        .app
        .content
        .path()
        .is_none_or(|p| p != Path::new("src/main.rs"))
    {
        h.act(Action::NextFile);
        h.pump_until("差分の計算", |app| {
            matches!(&app.content, ContentState::Diff(_))
        });
    }

    let screen = h.render();
    assert!(screen.contains("DIFF"), "{screen}");
    assert!(screen.contains("src/main.rs"), "{screen}");
    // 変更箇所から離れた行も表示される (全文表示)
    assert!(screen.contains("line 1 "), "1 行目が見えない\n{screen}");
    assert!(screen.contains("line 20"), "20 行目が見えない\n{screen}");
    // 左が旧内容、右が新内容
    assert!(screen.contains("line 10 changed"), "{screen}");
    assert!(screen.contains('-') && screen.contains('+'), "{screen}");
}

#[test]
fn tree_mode_lists_files_and_shows_content() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });

    let screen = h.render();
    assert!(screen.contains("TREE"), "{screen}");
    assert!(screen.contains("src"), "{screen}");
    assert!(screen.contains("README.md"), "{screen}");

    // README.md を選んで内容を表示する
    h.act(Action::TreeMove(1));
    h.pump_until("ファイル読み込み", |app| {
        matches!(&app.content, ContentState::Text(_))
    });
    let screen = h.render();
    assert!(screen.contains("readme"), "{screen}");
}

#[test]
fn narrow_terminal_hides_tree_when_content_is_focused() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 70, 20);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });
    h.app.focus = Focus::Content;
    let screen = h.render();
    assert!(!screen.contains("TREE"), "縮退時は左ペインを隠す\n{screen}");
}
