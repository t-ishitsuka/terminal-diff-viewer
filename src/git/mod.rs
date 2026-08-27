pub mod gix_backend;
pub mod model;

pub use model::*;

use std::path::Path;

/// 差分の比較対象。v1 は作業ツリー vs HEAD のみ。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DiffSpec {
    WorktreeVsHead,
}

/// Git 実装を隔離する境界。gix の API 変更の影響をこの trait の実装に閉じ込める。
pub trait GitBackend: Send + Sync {
    fn workdir(&self) -> &Path;
    fn head(&self) -> anyhow::Result<HeadInfo>;
    fn changes(&self, spec: DiffSpec) -> anyhow::Result<ChangeSet>;
    fn load(&self, side: Side, change: &FileChange, max_bytes: u64) -> anyhow::Result<Loaded>;
}
