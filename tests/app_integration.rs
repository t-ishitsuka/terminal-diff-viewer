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
use tdv::git::{DiffSpec, GitBackend, gix_backend::GixBackend};
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
        Self::with_config(root, width, height, Config::default())
    }

    fn with_config(root: &Path, width: u16, height: u16, cfg: Config) -> Self {
        let backend: Arc<dyn GitBackend> =
            Arc::new(GixBackend::discover(root).expect("リポジトリを開けない"));
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

/// 指定した文字列が描画されているセルの style を返す。
fn style_of(h: &mut Harness, needle: char) -> Vec<ratatui::style::Style> {
    h.terminal.clear().unwrap();
    let app = &mut h.app;
    h.terminal.draw(|frame| tdv::ui::draw(frame, app)).unwrap();
    let buffer = h.terminal.backend().buffer().clone();
    let area = *buffer.area();
    let mut out = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            if let Some(cell) = buffer.cell((x, y))
                && cell.symbol() == needle.to_string()
            {
                out.push(cell.style());
            }
        }
    }
    out
}

fn open_main_rs(h: &mut Harness) {
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
}

#[test]
fn changed_rows_are_colored() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    open_main_rs(&mut h);

    let theme = tdv::ui::theme::Theme::new(tdv::ui::theme::Palette::RedGreen);
    // 削除行のマーカー '-' には削除側の背景色が付く
    let minus = style_of(&mut h, '-');
    assert!(
        minus.iter().any(|s| s.bg == Some(theme.removed_bg)),
        "削除行に背景色が付いていない: {minus:?}"
    );
    let plus = style_of(&mut h, '+');
    assert!(
        plus.iter().any(|s| s.bg == Some(theme.added_bg)),
        "追加行に背景色が付いていない: {plus:?}"
    );
}

#[test]
fn syntax_highlight_is_applied_to_diff() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    open_main_rs(&mut h);

    // 本文は色付けを待たずに出る
    let ContentState::Diff(view) = &h.app.content else {
        panic!("差分が表示されていない");
    };
    assert!(
        view.new_highlight.is_none(),
        "色付けが本文の表示をブロックしている"
    );

    h.pump_until(
        "色付け",
        |app| matches!(&app.content, ContentState::Diff(v) if v.new_highlight.is_some()),
    );
    let ContentState::Diff(view) = &h.app.content else {
        panic!("差分が表示されていない");
    };
    // src/main.rs は Rust として認識され、色が付く
    let highlight = view.new_highlight.as_ref().expect("色付けされる");
    assert!(!highlight.line(0).is_empty(), "1 行目に色が付かない");
}

#[test]
fn syntax_highlight_can_be_disabled() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let cfg = Config {
        syntax_highlight: false,
        ..Config::default()
    };
    let mut h = Harness::with_config(dir.path(), 120, 30, cfg);
    open_main_rs(&mut h);

    let ContentState::Diff(view) = &h.app.content else {
        panic!("差分が表示されていない");
    };
    assert!(view.new_highlight.is_none());
}

#[test]
fn search_finds_matches_and_highlights_them() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    open_main_rs(&mut h);

    h.act(Action::StartSearch);
    for c in "line 20".chars() {
        h.act(Action::InputChar(c));
    }
    assert_eq!(
        h.app.search.hits.len(),
        2,
        "左右それぞれで 1 件ずつ一致する"
    );
    h.act(Action::InputSubmit);
    // 一致行まで自動でスクロールする
    let ContentState::Diff(view) = &h.app.content else {
        panic!()
    };
    assert!(view.offset > 0, "一致位置までスクロールしていない");

    let theme = tdv::ui::theme::Theme::new(tdv::ui::theme::Palette::RedGreen);
    let cells = style_of(&mut h, '2');
    assert!(
        cells.iter().any(|s| s.bg == Some(theme.search_bg)),
        "検索一致が強調されていない"
    );
}

#[test]
fn search_without_match_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    open_main_rs(&mut h);

    h.act(Action::StartSearch);
    for c in "存在しない文字列".chars() {
        h.act(Action::InputChar(c));
    }
    h.act(Action::InputSubmit);
    assert!(h.app.search.hits.is_empty());
    assert!(
        h.app
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("一致なし")),
        "{:?}",
        h.app.notice
    );
}

#[test]
fn search_is_case_insensitive_unless_query_has_uppercase() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    open_main_rs(&mut h);

    h.act(Action::StartSearch);
    for c in "LINE".chars() {
        h.act(Action::InputChar(c));
    }
    assert!(h.app.search.hits.is_empty(), "大文字を含む検索は区別する");

    h.act(Action::InputBackspace);
    h.act(Action::InputBackspace);
    h.act(Action::InputBackspace);
    h.act(Action::InputBackspace);
    for c in "line".chars() {
        h.act(Action::InputChar(c));
    }
    assert!(!h.app.search.hits.is_empty(), "小文字のみなら区別しない");
}

#[test]
fn filter_narrows_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });
    let before = h.app.fs_tree.visible_len();

    h.act(Action::StartFilter);
    for c in "readme".chars() {
        h.act(Action::InputChar(c));
    }
    let after = h.app.fs_tree.visible_len();
    assert!(
        after < before,
        "絞り込みで件数が減っていない ({before} -> {after})"
    );
    let screen = h.render();
    assert!(screen.contains("README.md"), "{screen}");
    assert!(!screen.contains("untracked.txt"), "{screen}");

    h.act(Action::InputCancel);
    assert_eq!(h.app.fs_tree.visible_len(), before, "取り消しで元に戻る");
}

#[test]
fn filter_keeps_ancestors_of_matches() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });
    // src を展開して子を読み込ませる
    let src = h.app.fs_tree.find_by_path(Path::new("src")).unwrap();
    h.app.fs_tree.select_node(src);
    h.act(Action::TreeOpen);
    h.pump_until("src の読み込み", |app| {
        app.fs_tree.children_count(src) > 0
    });

    h.act(Action::StartFilter);
    for c in "added".chars() {
        h.act(Action::InputChar(c));
    }
    let screen = h.render();
    assert!(
        screen.contains("src"),
        "祖先ディレクトリが残らない\n{screen}"
    );
    assert!(screen.contains("added.rs"), "{screen}");
    assert!(!screen.contains("README"), "{screen}");
}

#[test]
fn tree_entries_are_colored_by_file_kind() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    // 実行ファイルとシンボリックリンクを用意する
    let script = dir.path().join("run.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("README.md", dir.path().join("link.md")).unwrap();
    }

    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });

    let kinds: Vec<(String, tdv::vfs::EntryKind)> = h
        .app
        .fs_tree
        .visible()
        .to_vec()
        .iter()
        .map(|id| {
            let node = h.app.fs_tree.node(*id);
            (node.name.clone(), node.kind)
        })
        .collect();
    let kind_of = |name: &str| kinds.iter().find(|(n, _)| n == name).map(|(_, k)| *k);
    assert_eq!(kind_of("src"), Some(tdv::vfs::EntryKind::Dir));
    assert_eq!(kind_of("README.md"), Some(tdv::vfs::EntryKind::File));
    #[cfg(unix)]
    {
        assert_eq!(kind_of("run.sh"), Some(tdv::vfs::EntryKind::Executable));
        assert_eq!(kind_of("link.md"), Some(tdv::vfs::EntryKind::Symlink));
    }

    // 種別ごとに色が変わる
    let theme = tdv::ui::theme::Theme::new(tdv::ui::theme::Palette::RedGreen);
    let dir_fg = theme.entry_style(tdv::vfs::EntryKind::Dir, "src").fg;
    let toml_fg = theme
        .entry_style(tdv::vfs::EntryKind::File, "Cargo.toml")
        .fg;
    let md_fg = theme.entry_style(tdv::vfs::EntryKind::File, "README.md").fg;
    let rs_fg = theme.entry_style(tdv::vfs::EntryKind::File, "main.rs").fg;
    let mut all = vec![dir_fg, toml_fg, md_fg, rs_fg];
    all.dedup();
    assert_eq!(all.len(), 4, "種別ごとに色が分かれていない: {all:?}");
}

/// 折り返しの検証用に、1 行が極端に長いファイルだけを持つリポジトリを作る。
fn setup_long_line_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    let long = format!("{}END", "0123456789".repeat(30));
    std::fs::write(root.join("long.txt"), format!("{long}\n")).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
}

/// 長い行を含む 1 行だけの変更。折り返し時の左右整列を確かめる。
fn setup_wrapped_diff_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    let long = format!("{}TAIL", "abcdefghij".repeat(30));
    std::fs::write(root.join("w.txt"), format!("head\n{long}\nZZZ\n")).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    std::fs::write(root.join("w.txt"), "head\nshort\nZZZ\n").unwrap();
}

#[test]
fn unified_toggle_puts_both_sides_in_one_column() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    open_main_rs(&mut h);

    // side-by-side では旧行と新行が同じ画面行に並ぶ
    let side_by_side = h.render();
    assert!(
        side_by_side
            .lines()
            .any(|l| l.matches("line 10").count() == 2),
        "左右に同じ行が並んでいない\n{side_by_side}"
    );

    h.act(Action::ToggleUnified);
    let unified = h.render();
    assert!(unified.contains("line 10 changed"), "{unified}");
    assert!(
        !unified.lines().any(|l| l.matches("line 10").count() == 2),
        "unified なのに左右へ分かれている\n{unified}"
    );
    assert!(unified.contains("unified"), "状態表示がない\n{unified}");
}

#[test]
fn wrap_shows_the_tail_of_a_long_line() {
    let dir = tempfile::tempdir().unwrap();
    setup_long_line_repo(dir.path());
    let mut h = Harness::new(dir.path(), 60, 20);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });
    h.act(Action::TreeMove(0));
    h.pump_until("ファイル読み込み", |app| {
        matches!(&app.content, ContentState::Text(_))
    });

    let plain = h.render();
    assert!(
        !plain.contains("END"),
        "折り返し前から行末が見えている\n{plain}"
    );

    h.act(Action::ToggleWrap);
    let wrapped = h.render();
    assert!(
        wrapped.contains("END"),
        "折り返しても行末が見えない\n{wrapped}"
    );
}

#[test]
fn wrapped_side_by_side_keeps_rows_aligned() {
    let dir = tempfile::tempdir().unwrap();
    setup_wrapped_diff_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());
    h.act(Action::SetMode(Mode::Diff));
    h.pump_until("差分の計算", |app| {
        matches!(&app.content, ContentState::Diff(_))
    });
    h.act(Action::ToggleWrap);

    let screen = h.render();
    assert!(
        screen.contains("TAIL"),
        "長い行が折り返されていない\n{screen}"
    );
    // 左が複数行に折り返されても、次の行は左右で同じ画面行に並ぶ
    let aligned = screen
        .lines()
        .filter(|l| l.matches("ZZZ").count() == 2)
        .count();
    assert_eq!(aligned, 1, "左右の行がずれている\n{screen}");
}

#[test]
fn sort_toggle_groups_changes_by_kind() {
    let dir = tempfile::tempdir().unwrap();
    setup_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());

    fn order(h: &mut Harness) -> Vec<String> {
        let ids = h.app.change_tree.visible().to_vec();
        ids.iter()
            .map(|id| h.app.change_tree.node(*id).path.display().to_string())
            .collect()
    }

    let by_path = vec![
        "src/added.rs",
        "src/main.rs",
        "src/removed.rs",
        "untracked.txt",
    ];
    assert_eq!(order(&mut h), by_path);

    h.act(Action::CycleSort);
    assert_eq!(
        order(&mut h),
        vec![
            "src/main.rs",    // Modified
            "src/added.rs",   // Added
            "src/removed.rs", // Deleted
            "untracked.txt",  // Untracked
        ]
    );

    h.act(Action::CycleSort);
    assert_eq!(order(&mut h), by_path, "パス順へ戻らない");
}

/// 変更箇所を 2 つ持つファイル。連続ジャンプの検証に使う。
fn setup_two_hunk_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("h.txt"), numbered(60)).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    let edited = numbered(60)
        .replace("line 10\n", "line 10 changed\n")
        .replace("line 50\n", "line 50 changed\n");
    std::fs::write(root.join("h.txt"), edited).unwrap();
}

#[test]
fn repeated_hunk_jump_advances_through_every_hunk() {
    let dir = tempfile::tempdir().unwrap();
    setup_two_hunk_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 24);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());
    h.act(Action::SetMode(Mode::Diff));
    h.pump_until("差分の計算", |app| {
        matches!(&app.content, ContentState::Diff(_))
    });
    h.render();

    fn current_hunk(h: &Harness) -> Option<usize> {
        let ContentState::Diff(view) = &h.app.content else {
            panic!("差分が表示されていない");
        };
        assert_eq!(view.diff.hunks.len(), 2, "変更箇所が 2 つでない");
        view.hunk_cursor
    }

    h.act(Action::NextHunk);
    assert_eq!(current_hunk(&h), Some(0));
    let screen = h.render();
    assert!(screen.contains("line 10 changed"), "{screen}");

    // 2 回目で次の変更箇所へ進む (同じ箇所に留まらない)
    h.act(Action::NextHunk);
    assert_eq!(current_hunk(&h), Some(1));
    let screen = h.render();
    assert!(
        screen.contains("line 50 changed"),
        "2 つ目の変更箇所が見えていない\n{screen}"
    );

    h.act(Action::NextHunk);
    assert_eq!(current_hunk(&h), Some(1), "末尾で位置が動いている");
    assert_eq!(
        h.app.notice.as_deref(),
        Some("これ以降に変更箇所はない"),
        "終端が通知されていない"
    );

    h.act(Action::PrevHunk);
    assert_eq!(current_hunk(&h), Some(0), "戻れていない");
    let screen = h.render();
    assert!(screen.contains("line 10 changed"), "{screen}");

    // 手動スクロール後は現在位置から探し直す
    h.act(Action::ContentFirst);
    assert_eq!(current_hunk(&h), None);
    h.act(Action::NextHunk);
    assert_eq!(current_hunk(&h), Some(0));
}

/// 可視範囲の先行色付けを確かめるため、画面より十分長いファイルを用意する。
fn setup_long_file_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    let body: String = (1..=300).map(|i| format!("fn f{i}() {{}}\n")).collect();
    std::fs::write(root.join("long.rs"), &body).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
}

#[test]
fn visible_range_is_colored_before_the_whole_file() {
    let dir = tempfile::tempdir().unwrap();
    setup_long_file_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("ルートの読み込み", |app| {
        app.fs_tree.node_count() > 1
    });
    h.act(Action::TreeMove(0));
    h.pump_until("ファイル読み込み", |app| {
        matches!(&app.content, ContentState::Text(_))
    });

    fn covered(h: &Harness) -> usize {
        let ContentState::Text(view) = &h.app.content else {
            panic!("内容が表示されていない");
        };
        view.highlight.as_ref().map_or(0, |v| v.covered_lines())
    }

    h.pump_until(
        "可視範囲の色付け",
        |app| matches!(&app.content, ContentState::Text(v) if v.highlight.is_some()),
    );
    let first = covered(&h);
    assert!(
        (1..300).contains(&first),
        "先に届くのは可視範囲ぶんのはず: {first}"
    );

    h.pump_until("全文の色付け", |app| {
        matches!(&app.content, ContentState::Text(v)
            if v.highlight.as_ref().is_some_and(|hl| hl.covered_lines() >= 300))
    });
    assert_eq!(covered(&h), 300);
}

/// staged のみ / unstaged のみ / 両方 (3 者で内容が違う) を含むリポジトリ。
fn setup_staged_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    for name in ["staged.txt", "unstaged.txt", "both.txt"] {
        std::fs::write(root.join(name), "v1\n").unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);

    std::fs::write(root.join("staged.txt"), "v2\n").unwrap();
    git(root, &["add", "staged.txt"]);

    std::fs::write(root.join("unstaged.txt"), "v2\n").unwrap();

    // HEAD=v1 / index=v2 / 作業ツリー=v3。比較対象ごとに内容が変わる
    std::fs::write(root.join("both.txt"), "v2\n").unwrap();
    git(root, &["add", "both.txt"]);
    std::fs::write(root.join("both.txt"), "v3\n").unwrap();
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git を実行できない");
    assert!(out.status.success(), "git {args:?} が失敗");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn changed_paths(h: &Harness) -> Vec<String> {
    let mut paths: Vec<String> = h
        .app
        .changes
        .files
        .iter()
        .map(|c| c.path.display().to_string())
        .collect();
    paths.sort();
    paths
}

fn expected_paths(out: &str) -> Vec<String> {
    let mut paths: Vec<String> = out
        .lines()
        .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
        .collect();
    paths.sort();
    paths
}

#[test]
fn diff_spec_switches_the_change_list() {
    let dir = tempfile::tempdir().unwrap();
    setup_staged_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());

    // 作業ツリー vs HEAD: 3 ファイルすべてが変更として出る
    assert_eq!(
        changed_paths(&h),
        expected_paths(&git_output(dir.path(), &["diff", "HEAD", "--name-status"]))
    );
    assert_eq!(changed_paths(&h).len(), 3);

    let next_spec = |h: &mut Harness| {
        let before = h.app.status_generation;
        h.act(Action::CycleDiffSpec);
        h.pump_until("status 再取得", |app| {
            app.status_generation > before && !app.scanning
        });
    };

    next_spec(&mut h);
    assert_eq!(h.app.diff_spec, DiffSpec::StagedVsHead);
    assert_eq!(
        changed_paths(&h),
        expected_paths(&git_output(
            dir.path(),
            &["diff", "--cached", "--name-status"]
        ))
    );
    assert_eq!(changed_paths(&h), vec!["both.txt", "staged.txt"]);

    next_spec(&mut h);
    assert_eq!(h.app.diff_spec, DiffSpec::WorktreeVsIndex);
    assert_eq!(
        changed_paths(&h),
        expected_paths(&git_output(dir.path(), &["diff", "--name-status"]))
    );
    assert_eq!(changed_paths(&h), vec!["both.txt", "unstaged.txt"]);

    next_spec(&mut h);
    assert_eq!(h.app.diff_spec, DiffSpec::WorktreeVsHead);
    assert_eq!(changed_paths(&h).len(), 3);
}

#[test]
fn staged_diff_reads_the_index_contents() {
    let dir = tempfile::tempdir().unwrap();
    setup_staged_repo(dir.path());
    let mut h = Harness::new(dir.path(), 120, 30);
    h.pump_until("status 取得", |app| !app.changes.files.is_empty());
    h.act(Action::SetMode(Mode::Diff));
    h.pump_until("差分の計算", |app| {
        matches!(&app.content, ContentState::Diff(_))
    });

    let open_both = |h: &mut Harness| {
        while h
            .app
            .content
            .path()
            .is_none_or(|p| p != Path::new("both.txt"))
        {
            h.act(Action::NextFile);
            h.pump_until("差分の計算", |app| {
                matches!(&app.content, ContentState::Diff(_))
            });
        }
    };
    let sides = |h: &Harness| -> (String, String) {
        let ContentState::Diff(view) = &h.app.content else {
            panic!("差分が表示されていない");
        };
        (
            String::from_utf8_lossy(view.diff.old.line_display(0)).into_owned(),
            String::from_utf8_lossy(view.diff.new.line_display(0)).into_owned(),
        )
    };

    open_both(&mut h);
    // HEAD=v1 / index=v2 / 作業ツリー=v3
    assert_eq!(sides(&h), ("v1".into(), "v3".into()));

    h.act(Action::CycleDiffSpec);
    h.pump_until("staged の差分", |app| {
        app.diff_spec == DiffSpec::StagedVsHead && matches!(&app.content, ContentState::Diff(_))
    });
    open_both(&mut h);
    assert_eq!(
        sides(&h),
        ("v1".into(), "v2".into()),
        "index の内容が出ていない"
    );

    h.act(Action::CycleDiffSpec);
    h.pump_until("unstaged の差分", |app| {
        app.diff_spec == DiffSpec::WorktreeVsIndex && matches!(&app.content, ContentState::Diff(_))
    });
    open_both(&mut h);
    assert_eq!(
        sides(&h),
        ("v2".into(), "v3".into()),
        "index を旧側にしていない"
    );
}
