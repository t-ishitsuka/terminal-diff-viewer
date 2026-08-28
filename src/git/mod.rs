pub mod gix_backend;
pub mod model;

pub use model::*;

use std::path::Path;

/// 差分の比較対象。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DiffSpec {
    /// 作業ツリー vs HEAD。staged と unstaged を統合して 1 つの差分として扱う。
    WorktreeVsHead,
    /// index vs HEAD。stage 済みの変更だけを見る。
    StagedVsHead,
    /// 作業ツリー vs index。まだ stage していない変更だけを見る。
    WorktreeVsIndex,
}

impl DiffSpec {
    pub fn next(self) -> Self {
        match self {
            DiffSpec::WorktreeVsHead => DiffSpec::StagedVsHead,
            DiffSpec::StagedVsHead => DiffSpec::WorktreeVsIndex,
            DiffSpec::WorktreeVsIndex => DiffSpec::WorktreeVsHead,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DiffSpec::WorktreeVsHead => "作業ツリー↔HEAD",
            DiffSpec::StagedVsHead => "staged↔HEAD",
            DiffSpec::WorktreeVsIndex => "作業ツリー↔index",
        }
    }

    /// 旧側が index の内容になるか。false なら HEAD のツリーから引く。
    fn old_is_index(self) -> bool {
        self == DiffSpec::WorktreeVsIndex
    }

    /// 新側が index の内容になるか。false なら作業ツリーのファイルを読む。
    fn new_is_index(self) -> bool {
        self == DiffSpec::StagedVsHead
    }
}

/// Git 実装を隔離する境界。gix の API 変更の影響をこの trait の実装に閉じ込める。
pub trait GitBackend: Send + Sync {
    fn workdir(&self) -> &Path;
    fn head(&self) -> anyhow::Result<HeadInfo>;
    fn changes(&self, spec: DiffSpec) -> anyhow::Result<ChangeSet>;
    fn load(
        &self,
        spec: DiffSpec,
        side: Side,
        change: &FileChange,
        max_bytes: u64,
    ) -> anyhow::Result<Loaded>;
}
