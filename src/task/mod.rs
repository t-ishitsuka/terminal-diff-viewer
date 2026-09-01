pub mod message;

pub use message::*;

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::diff::{LineTable, align};
use crate::git::{DiffSpec, FileChange, GitBackend, Loaded, Side};
use crate::highlight::highlight;

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
        TaskRequest::ScanStatus { generation, spec } => TaskResult::Status {
            generation,
            outcome: scan_status(ctx, &spec).map_err(|e| format!("{e:#}")),
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
            outcome: load_text(ctx, &abs).map_err(|e| format!("{e:#}")),
            path,
        },
        TaskRequest::ComputeDiff {
            generation,
            change,
            spec,
        } => {
            let outcome = compute_diff(ctx, &change, &spec).map_err(|e| format!("{e:#}"));
            TaskResult::Diff {
                generation,
                change,
                outcome,
            }
        }
        TaskRequest::Stage { change, unstage } => {
            let outcome = stage(ctx, &change, unstage).map_err(|e| format!("{e:#}"));
            TaskResult::Staged {
                path: change.path,
                unstage,
                outcome,
            }
        }
        TaskRequest::Highlight {
            generation,
            target,
            path,
            table,
            theme,
            max_lines,
            upto,
        } => TaskResult::Highlight {
            generation,
            target,
            highlight: highlight(&table, &path, &theme, max_lines, upto),
        },
    }
}

fn scan_status(ctx: &WorkerCtx, spec: &DiffSpec) -> anyhow::Result<StatusOutcome> {
    let backend = ctx
        .backend
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Git リポジトリではない"))?;
    Ok(StatusOutcome {
        changes: backend.changes(spec)?,
        head: backend.head()?,
    })
}

fn load_text(ctx: &WorkerCtx, abs: &Path) -> anyhow::Result<TextOutcome> {
    let loaded = crate::git::gix_backend::read_worktree_file(abs, ctx.max_file_bytes)?;
    Ok(match loaded {
        Loaded::Text(bytes) => TextOutcome::Ready {
            table: LineTable::new(bytes),
        },
        Loaded::Unsupported(reason) => TextOutcome::Unsupported(reason),
    })
}

fn compute_diff(ctx: &WorkerCtx, change: &FileChange, spec: &DiffSpec) -> anyhow::Result<Content> {
    let backend = ctx
        .backend
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Git リポジトリではない"))?;
    let old = backend.load(spec, Side::Old, change, ctx.max_file_bytes)?;
    let new = backend.load(spec, Side::New, change, ctx.max_file_bytes)?;
    let (old, new) = match (old, new) {
        (Loaded::Unsupported(reason), _) | (_, Loaded::Unsupported(reason)) => {
            return Ok(Content::Unsupported(reason));
        }
        (Loaded::Text(o), Loaded::Text(n)) => (LineTable::new(o), LineTable::new(n)),
    };

    Ok(Content::Ready {
        diff: Box::new(align(old, new)),
    })
}

fn stage(ctx: &WorkerCtx, change: &FileChange, unstage: bool) -> anyhow::Result<()> {
    let backend = ctx
        .backend
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Git リポジトリではない"))?;
    if unstage {
        backend.unstage(change)
    } else {
        backend.stage(change)
    }
}
