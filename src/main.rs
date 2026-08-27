use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use anyhow::{Context, Result};
use clap::Parser;
use ratatui::crossterm::event;

use tdv::app::{self, App};
use tdv::cli;
use tdv::config::Config;
use tdv::git::{GitBackend, gix_backend::GixBackend};
use tdv::task::{AppEvent, Pool, WorkerCtx};

fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let start = match cli.path {
        Some(path) => path,
        None => std::env::current_dir().context("カレントディレクトリを取得できない")?,
    };
    let start = start
        .canonicalize()
        .with_context(|| format!("{} を解決できない", start.display()))?;

    // 設定ファイル → CLI 引数 の順に上書きする
    let mut cfg = Config::load(cli.config.as_deref())?;
    if let Some(v) = cli.max_file_bytes {
        cfg.max_file_bytes = v;
    }
    if let Some(v) = cli.tab_width {
        cfg.text.tab_width = v.max(1);
    }
    if cli.ambiguous_wide {
        cfg.text.ambiguous_wide = true;
    }
    if cli.no_highlight {
        cfg.syntax_highlight = false;
    }

    // リポジトリ外でも tree モードは動かす
    let backend: Option<Arc<dyn GitBackend>> = match GixBackend::discover(&start) {
        Ok(backend) => Some(Arc::new(backend)),
        Err(_) => None,
    };
    let root = backend
        .as_ref()
        .map(|b| b.workdir().to_path_buf())
        .unwrap_or_else(|| start.clone());

    let (tx, rx) = channel::<AppEvent>();
    spawn_input_thread(tx.clone());

    let workers = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).clamp(1, 4))
        .unwrap_or(2);
    let ctx = Arc::new(WorkerCtx {
        backend: backend.clone(),
        max_file_bytes: cfg.max_file_bytes,
    });
    let pool = Pool::spawn(workers, ctx, tx);

    // 起動は常に tree モード。diff へは起動後に m / d で切り替える
    let mut application = App::new(cfg, root, backend, pool);

    // ratatui::try_init が raw mode / alternate screen への移行とパニックフックを設定する
    let mut terminal = ratatui::try_init().context("端末を初期化できない")?;
    let result = app::run(&mut terminal, &mut application, &rx);
    ratatui::restore();
    result
}

fn spawn_input_thread(tx: Sender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            match event::read() {
                Ok(ev) => {
                    if tx.send(AppEvent::Input(ev)).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });
}
