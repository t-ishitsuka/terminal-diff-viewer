//! NFR-01 / NFR-02 の手動計測。既定では実行しない。
//!
//! ```sh
//! cargo test --release --test perf -- --ignored --nocapture
//! ```
//!
//! 実行環境の性能に依存するため CI には含めない。閾値は
//! [docs/01-requirements.md](../docs/01-requirements.md) の非機能要件に合わせている。

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tdv::app::state::{App, ContentState, Mode};
use tdv::app::{action::Action, update};
use tdv::config::Config;
use tdv::git::{GitBackend, gix_backend::GixBackend};
use tdv::task::{AppEvent, Pool, WorkerCtx};

/// NFR-01: 1 万ファイル規模のリポジトリで起動からツリー描画まで。
const TREE_BUDGET: Duration = Duration::from_millis(300);
/// NFR-02: 5000 行ファイルの選択から差分描画まで。
const DIFF_BUDGET: Duration = Duration::from_millis(100);

const FILE_COUNT: usize = 10_000;
const DIRS: usize = 100;
const DIFF_LINES: usize = 5_000;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
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
        out.status.success(),
        "git {args:?} が失敗: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

struct Harness {
    app: App,
    rx: Receiver<AppEvent>,
    terminal: Terminal<TestBackend>,
}

impl Harness {
    fn new(root: &Path) -> Self {
        let cfg = Config::default();
        let backend: Arc<dyn GitBackend> =
            Arc::new(GixBackend::discover(root).expect("リポジトリを開けない"));
        let (tx, rx) = channel::<AppEvent>();
        let ctx = Arc::new(WorkerCtx {
            backend: Some(Arc::clone(&backend)),
            max_file_bytes: cfg.max_file_bytes,
        });
        let pool = Pool::spawn(4, ctx, tx);
        let app = App::new(cfg, root.to_path_buf(), Some(backend), pool);
        let terminal = Terminal::new(TestBackend::new(160, 48)).unwrap();
        Self { app, rx, terminal }
    }

    fn pump_until(&mut self, label: &str, cond: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !cond(&self.app) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "{label} がタイムアウトした");
            match self.rx.recv_timeout(remaining) {
                Ok(AppEvent::Task(result)) => update::on_task(&mut self.app, result),
                Ok(_) => {}
                Err(_) => panic!("{label} の待機中にチャネルが閉じた"),
            }
        }
    }

    fn draw(&mut self) {
        let app = &mut self.app;
        self.terminal
            .draw(|frame| tdv::ui::draw(frame, app))
            .unwrap();
    }
}

fn report(label: &str, elapsed: Duration, budget: Duration) {
    let verdict = if elapsed <= budget { "OK" } else { "超過" };
    println!("{label}: {elapsed:?} (上限 {budget:?}) {verdict}");
}

/// 100 ディレクトリ × 100 ファイルのリポジトリ。1 ファイルだけ未コミットの変更を持つ。
fn setup_large_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    let per_dir = FILE_COUNT / DIRS;
    for d in 0..DIRS {
        let dir = root.join(format!("dir{d:03}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..per_dir {
            std::fs::write(dir.join(format!("file{f:03}.rs")), "fn main() {}\n").unwrap();
        }
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    std::fs::write(root.join("dir000/file000.rs"), "fn main() { /* 変更 */ }\n").unwrap();
}

/// 5000 行のファイルを 50 箇所変更したリポジトリ。
fn setup_wide_diff_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    let original: String = (1..=DIFF_LINES)
        .map(|i| format!("    let value_{i} = compute({i});\n"))
        .collect();
    std::fs::write(root.join("big.rs"), &original).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    let edited: String = (1..=DIFF_LINES)
        .map(|i| {
            if i % 100 == 0 {
                format!("    let value_{i} = compute({i}) + 1;\n")
            } else {
                format!("    let value_{i} = compute({i});\n")
            }
        })
        .collect();
    std::fs::write(root.join("big.rs"), edited).unwrap();
}

#[test]
#[ignore = "手動計測用。--ignored を付けて実行する"]
fn nfr_01_tree_is_drawn_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    setup_large_repo(dir.path());

    let start = Instant::now();
    let mut h = Harness::new(dir.path());
    // ツリーは遅延展開のため、ルート直下が揃った時点で描画できる
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });
    h.draw();
    let tree = start.elapsed();

    // 状態走査は非同期だが、変更記号が出るまでの時間も参考値として測る
    h.pump_until("status 取得", |app| !app.scanning);
    h.draw();
    let status = start.elapsed();

    println!("--- NFR-01 ({FILE_COUNT} ファイル / {DIRS} ディレクトリ) ---");
    report("起動からツリー描画", tree, TREE_BUDGET);
    println!("起動から status 反映: {status:?} (参考値。非同期のため要件外)");
    assert_eq!(h.app.changes.files.len(), 1, "変更ファイル数が想定と違う");
    assert!(tree <= TREE_BUDGET, "NFR-01 未達: {tree:?}");
}

#[test]
#[ignore = "手動計測用。--ignored を付けて実行する"]
fn nfr_02_diff_is_drawn_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    setup_wide_diff_repo(dir.path());

    let mut h = Harness::new(dir.path());
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());

    // 差分モードへ切り替えると選択中のファイルの差分計算が始まる
    let start = Instant::now();
    update::apply(&mut h.app, Action::SetMode(Mode::Diff));
    h.pump_until("差分の計算", |app| {
        matches!(&app.content, ContentState::Diff(_))
    });
    h.draw();
    let elapsed = start.elapsed();

    // 色付けは後追いのため要件外。可視範囲と全文それぞれの到達を参考値として測る
    h.pump_until("可視範囲の色付け", |app| {
        matches!(&app.content, ContentState::Diff(v)
            if v.old_highlight.is_some() && v.new_highlight.is_some())
    });
    let visible_colored = start.elapsed();
    h.pump_until("全文の色付け", |app| {
        matches!(&app.content, ContentState::Diff(v)
            if covered(&v.old_highlight) >= v.diff.old.len()
                && covered(&v.new_highlight) >= v.diff.new.len())
    });
    let colored = start.elapsed();

    let ContentState::Diff(view) = &h.app.content else {
        panic!("差分が表示されていない");
    };
    println!(
        "--- NFR-02 ({DIFF_LINES} 行 / 変更 {} 箇所) ---",
        view.diff.hunks.len()
    );
    report("選択から差分描画", elapsed, DIFF_BUDGET);
    println!("選択から可視範囲の色付け: {visible_colored:?} (参考値。後追いのため要件外)");
    println!("選択から全文の色付け: {colored:?} (参考値。後追いのため要件外)");
    assert_eq!(view.diff.rows.len(), DIFF_LINES, "全文が保持されていない");
    assert!(elapsed <= DIFF_BUDGET, "NFR-02 未達: {elapsed:?}");
}

fn covered(highlight: &Option<std::sync::Arc<tdv::highlight::Highlighted>>) -> usize {
    highlight.as_ref().map_or(0, |h| h.covered_lines())
}

/// NFR 相当の目安。1 ページ目のコミット一覧が出るまで。
const LOG_BUDGET: Duration = Duration::from_millis(300);
const COMMIT_COUNT: usize = 10_000;

/// 1 万コミットのリポジトリ。fast-import で一括生成する。
fn setup_deep_history(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    let mut script = String::new();
    script.push_str("blob\nmark :1\ndata 3\nv1\n");
    for i in 1..=COMMIT_COUNT {
        script.push_str("commit refs/heads/main\n");
        script.push_str(&format!("mark :{}\n", i + 1));
        script.push_str("author test <test@example.com> 1700000000 +0000\n");
        script.push_str("committer test <test@example.com> 1700000000 +0000\n");
        let message = format!("commit {i}\n");
        script.push_str(&format!("data {}\n{message}", message.len()));
        if i > 1 {
            script.push_str(&format!("from :{}\n", i));
        }
        script.push_str("M 100644 :1 f.txt\n");
    }
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["fast-import", "--quiet"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("git fast-import を起動できない");
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(script.as_bytes())
        .unwrap();
    assert!(child.wait().expect("fast-import").success());
    git(root, &["reset", "-q", "--hard", "main"]);
}

#[test]
#[ignore = "手動計測用。--ignored を付けて実行する"]
fn log_mode_opens_within_budget_on_a_deep_history() {
    let dir = tempfile::tempdir().unwrap();
    setup_deep_history(dir.path());

    let mut h = Harness::new(dir.path());
    h.pump_until("status 取得", |app| !app.scanning);

    let start = Instant::now();
    update::apply(&mut h.app, Action::SetMode(Mode::Log));
    h.pump_until("コミット一覧", |app| !app.log_commits.is_empty());
    h.draw();
    let elapsed = start.elapsed();

    println!("--- log モード ({COMMIT_COUNT} コミット) ---");
    report("切り替えから一覧描画", elapsed, LOG_BUDGET);
    println!(
        "1 ページ目の件数: {} (全件は読まない)",
        h.app.log_commits.len()
    );
    assert!(!h.app.log_end, "1 ページで打ち切られていない");
    assert!(elapsed <= LOG_BUDGET, "一覧の表示が遅い: {elapsed:?}");
}
