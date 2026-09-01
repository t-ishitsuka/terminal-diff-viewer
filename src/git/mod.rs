pub mod gix_backend;
pub mod model;

pub use model::*;

use std::path::Path;

/// 差分の比較対象。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffSpec {
    /// 作業ツリー vs HEAD。staged と unstaged を統合して 1 つの差分として扱う。
    WorktreeVsHead,
    /// index vs HEAD。stage 済みの変更だけを見る。
    StagedVsHead,
    /// 作業ツリー vs index。まだ stage していない変更だけを見る。
    WorktreeVsIndex,
    /// 任意の 2 つの ref の比較。解決はワーカー側で行うため文字列のまま持つ。
    Range { from: String, to: String },
}

impl DiffSpec {
    /// `s` キーで巡回する 3 種。ref 間比較からは作業ツリー vs HEAD へ戻る。
    pub fn next(&self) -> Self {
        match self {
            DiffSpec::WorktreeVsHead => DiffSpec::StagedVsHead,
            DiffSpec::StagedVsHead => DiffSpec::WorktreeVsIndex,
            DiffSpec::WorktreeVsIndex | DiffSpec::Range { .. } => DiffSpec::WorktreeVsHead,
        }
    }

    pub fn label(&self) -> String {
        match self {
            DiffSpec::WorktreeVsHead => "作業ツリー↔HEAD".into(),
            DiffSpec::StagedVsHead => "staged↔HEAD".into(),
            DiffSpec::WorktreeVsIndex => "作業ツリー↔index".into(),
            DiffSpec::Range { from, to } => format!("{from}..{to}"),
        }
    }

    /// `A..B` / `A..` / `..B` / `A` を受ける。省略側は HEAD とする。
    pub fn parse_range(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("ref を入力する (例: HEAD~3..HEAD)".into());
        }
        if input.contains("...") {
            return Err("三点比較 (A...B) は未対応".into());
        }
        let (from, to) = match input.split_once("..") {
            Some((from, to)) => (from.trim(), to.trim()),
            None => (input, "HEAD"),
        };
        let from = if from.is_empty() { "HEAD" } else { from };
        let to = if to.is_empty() { "HEAD" } else { to };
        Ok(DiffSpec::Range {
            from: from.to_string(),
            to: to.to_string(),
        })
    }

    /// ref 間比較なら (旧側, 新側) の revspec。
    pub fn range(&self) -> Option<(&str, &str)> {
        match self {
            DiffSpec::Range { from, to } => Some((from.as_str(), to.as_str())),
            _ => None,
        }
    }

    /// 旧側が index の内容になるか。
    fn old_is_index(&self) -> bool {
        *self == DiffSpec::WorktreeVsIndex
    }

    /// 新側が index の内容になるか。
    fn new_is_index(&self) -> bool {
        *self == DiffSpec::StagedVsHead
    }
}
/// Git 実装を隔離する境界。gix の API 変更の影響をこの trait の実装に閉じ込める。
pub trait GitBackend: Send + Sync {
    fn workdir(&self) -> &Path;
    fn head(&self) -> anyhow::Result<HeadInfo>;
    fn changes(&self, spec: &DiffSpec) -> anyhow::Result<ChangeSet>;
    fn load(
        &self,
        spec: &DiffSpec,
        side: Side,
        change: &FileChange,
        max_bytes: u64,
    ) -> anyhow::Result<Loaded>;
    /// index を作業ツリーの内容に合わせる。
    fn stage(&self, change: &FileChange) -> anyhow::Result<()>;
    /// index を HEAD の内容に戻す。
    fn unstage(&self, change: &FileChange) -> anyhow::Result<()>;
    /// HEAD から辿れるコミットを新しい順に返す。
    fn log(&self, skip: usize, limit: usize) -> anyhow::Result<Vec<CommitInfo>>;
    /// コミット 1 件の差分を表す比較対象 (親..そのコミット)。
    fn commit_spec(&self, id: &str) -> anyhow::Result<DiffSpec>;
}
