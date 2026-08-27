pub mod message;

pub use message::*;

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::diff::{LineTable, align};
use crate::git::{DiffSpec, GitBackend, Loaded, Side};

pub struct WorkerCtx {
    pub backend: Option<Arc<dyn GitBackend>>,
    pub max_file_bytes: u64,
}

/// I/O と差分計算をメインループから切り離すためのワーカープール。
pub struct Pool {
    tx: Sender<TaskRequest>,
}

impl Pool {
    pub fn spawn(workers: usize, ctx: Arc<WorkerCtx>, out: Sender<AppEvent>) -> Self {
        let (tx, rx) = channel::<TaskRequest>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..workers.max(1) {
            let rx: Arc<Mutex<Receiver<TaskRequest>>> = Arc::clone(&rx);
            let ctx = Arc::clone(&ctx);
            let out = out.clone();
            std::thread::spawn(move || {
                loop {
                    let request = {
                        let guard = match rx.lock() {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        guard.recv()
                    };
                    let Ok(request) = request else { return };
                    let result = handle(&ctx, request);
                    if out.send(AppEvent::Task(result)).is_err() {
                        return;
                    }
                }
            });
        }
        Self { tx }
    }

    pub fn submit(&self, request: TaskRequest) {
        // 受信側が閉じているのはアプリ終了時のみ。取りこぼしても影響はない
        let _ = self.tx.send(request);
    }
}

fn handle(ctx: &WorkerCtx, request: TaskRequest) -> TaskResult {
    match request {
        TaskRequest::ScanStatus { generation } => TaskResult::Status {
            generation,
            outcome: scan_status(ctx).map_err(|e| format!("{e:#}")),
        },
        TaskRequest::ReadDir {
            generation,
            node,
            dir,
            show_ignored,
        } => TaskResult::Dir {
            generation,
            node,
            outcome: crate::vfs::read_dir(&dir, show_ignored).map_err(|e| format!("{e:#}")),
        },
        TaskRequest::LoadText {
            generation,
            path,
            abs,
        } => TaskResult::Text {
            generation,
            path,
            outcome: load_text(ctx, &abs).map_err(|e| format!("{e:#}")),
        },
        TaskRequest::ComputeDiff { generation, change } => {
            let outcome = compute_diff(ctx, &change).map_err(|e| format!("{e:#}"));
            TaskResult::Diff {
                generation,
                change,
                outcome,
            }
        }
    }
}

fn scan_status(ctx: &WorkerCtx) -> anyhow::Result<StatusOutcome> {
    let backend = ctx
        .backend
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Git リポジトリではない"))?;
    Ok(StatusOutcome {
        changes: backend.changes(DiffSpec::WorktreeVsHead)?,
        head: backend.head()?,
    })
}

fn load_text(ctx: &WorkerCtx, abs: &std::path::Path) -> anyhow::Result<TextOutcome> {
    let loaded = crate::git::gix_backend::read_worktree_file(abs, ctx.max_file_bytes)?;
    Ok(match loaded {
        Loaded::Text(bytes) => TextOutcome::Ready(LineTable::new(bytes)),
        Loaded::Unsupported(reason) => TextOutcome::Unsupported(reason),
    })
}

fn compute_diff(ctx: &WorkerCtx, change: &crate::git::FileChange) -> anyhow::Result<Content> {
    let backend = ctx
        .backend
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Git リポジトリではない"))?;
    let old = backend.load(Side::Old, change, ctx.max_file_bytes)?;
    let new = backend.load(Side::New, change, ctx.max_file_bytes)?;
    let (old, new) = match (old, new) {
        (Loaded::Unsupported(reason), _) | (_, Loaded::Unsupported(reason)) => {
            return Ok(Content::Unsupported(reason));
        }
        (Loaded::Text(o), Loaded::Text(n)) => (o, n),
    };
    Ok(Content::Ready(align(
        LineTable::new(old),
        LineTable::new(new),
    )))
}
