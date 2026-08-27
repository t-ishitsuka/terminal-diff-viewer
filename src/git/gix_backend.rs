use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use gix::bstr::{BString, ByteSlice};

use super::model::*;
use super::{DiffSpec, GitBackend};

pub struct GixBackend {
    repo: gix::ThreadSafeRepository,
    workdir: PathBuf,
}

impl GixBackend {
    pub fn discover(start: &Path) -> Result<Self> {
        let repo = gix::discover(start).context("Git リポジトリが見つからない")?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| anyhow!("bare リポジトリは対象外"))?
            .to_path_buf();
        Ok(Self {
            repo: repo.into_sync(),
            workdir,
        })
    }
}

/// tree_index (HEAD↔index) と index_worktree (index↔作業ツリー) の観測を
/// パスごとに集約し、最後に「作業ツリー vs HEAD」の一種類へ畳む。
#[derive(Default, Clone)]
struct Acc {
    tree: Option<TreeObservation>,
    worktree: Option<WorktreeObservation>,
    old_path: Option<BString>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum TreeObservation {
    Addition,
    Deletion,
    Modification,
    Rewrite,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum WorktreeObservation {
    Removed,
    Modified,
    Untracked,
    Rewrite,
}

impl Acc {
    /// docs/03-architecture.md §4.1 の統合表。None を返す場合は一覧から除外する。
    fn resolve(&self) -> Option<ChangeKind> {
        use TreeObservation as T;
        use WorktreeObservation as W;
        match (self.tree, self.worktree) {
            // index へ追加後に作業ツリーから消えた場合、HEAD から見ると存在しないまま
            (Some(T::Addition), Some(W::Removed)) => None,
            (Some(T::Deletion), _) => Some(ChangeKind::Deleted),
            (None, Some(W::Removed)) => Some(ChangeKind::Deleted),
            (Some(T::Rewrite), _) | (_, Some(W::Rewrite)) => Some(ChangeKind::Renamed),
            (Some(T::Addition), _) => Some(ChangeKind::Added),
            (None, Some(W::Untracked)) => Some(ChangeKind::Untracked),
            (Some(T::Modification), _) | (None, Some(W::Modified)) => Some(ChangeKind::Modified),
            (None, None) => None,
        }
    }
}

fn to_path(p: &BString) -> PathBuf {
    gix::path::from_bstr(p.as_bstr()).into_owned()
}

impl GitBackend for GixBackend {
    fn workdir(&self) -> &Path {
        &self.workdir
    }

    fn head(&self) -> Result<HeadInfo> {
        let repo = self.repo.to_thread_local();
        let name = match repo.head_name()? {
            Some(name) => name.shorten().to_string(),
            None => repo
                .head_id()
                .map(|id| id.to_hex_with_len(7).to_string())
                .unwrap_or_else(|_| "(no commit)".to_string()),
        };
        Ok(HeadInfo { name })
    }

    fn changes(&self, _spec: DiffSpec) -> Result<ChangeSet> {
        use gix::diff::index::ChangeRef;
        use gix::status::index_worktree::{Item as IwItem, RewriteSource};
        use gix::status::plumbing::index_as_worktree::{Change as IwChange, EntryStatus};

        let repo = self.repo.to_thread_local();
        let mut acc: BTreeMap<BString, Acc> = BTreeMap::new();

        let iter = repo
            .status(gix::progress::Discard)?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .into_iter(None)?;

        for item in iter {
            match item? {
                gix::status::Item::TreeIndex(change) => match change {
                    ChangeRef::Addition { location, .. } => {
                        acc.entry(location.into_owned()).or_default().tree =
                            Some(TreeObservation::Addition);
                    }
                    ChangeRef::Deletion { location, .. } => {
                        acc.entry(location.into_owned()).or_default().tree =
                            Some(TreeObservation::Deletion);
                    }
                    ChangeRef::Modification { location, .. } => {
                        acc.entry(location.into_owned()).or_default().tree =
                            Some(TreeObservation::Modification);
                    }
                    ChangeRef::Rewrite {
                        source_location,
                        location,
                        ..
                    } => {
                        let e = acc.entry(location.into_owned()).or_default();
                        e.tree = Some(TreeObservation::Rewrite);
                        e.old_path = Some(source_location.into_owned());
                    }
                },
                gix::status::Item::IndexWorktree(iw) => match iw {
                    IwItem::Modification {
                        rela_path, status, ..
                    } => {
                        let observed = match status {
                            EntryStatus::Change(IwChange::Removed) => {
                                Some(WorktreeObservation::Removed)
                            }
                            EntryStatus::Change(_) | EntryStatus::Conflict { .. } => {
                                Some(WorktreeObservation::Modified)
                            }
                            // NeedsUpdate は stat 情報の更新提案のみ、IntentToAdd は内容変更なし
                            EntryStatus::NeedsUpdate(_) | EntryStatus::IntentToAdd => None,
                        };
                        if let Some(observed) = observed {
                            acc.entry(rela_path).or_default().worktree = Some(observed);
                        }
                    }
                    IwItem::DirectoryContents { entry, .. } => {
                        if entry.status == gix::dir::entry::Status::Untracked {
                            acc.entry(entry.rela_path).or_default().worktree =
                                Some(WorktreeObservation::Untracked);
                        }
                    }
                    IwItem::Rewrite {
                        source,
                        dirwalk_entry,
                        ..
                    } => {
                        let old = match source {
                            RewriteSource::RewriteFromIndex {
                                source_rela_path, ..
                            } => Some(source_rela_path),
                            RewriteSource::CopyFromDirectoryEntry {
                                source_dirwalk_entry,
                                ..
                            } => Some(source_dirwalk_entry.rela_path),
                        };
                        let e = acc.entry(dirwalk_entry.rela_path).or_default();
                        e.worktree = Some(WorktreeObservation::Rewrite);
                        e.old_path = old;
                    }
                },
            }
        }

        let mut files: Vec<FileChange> = acc
            .into_iter()
            .filter_map(|(path, a)| {
                let kind = a.resolve()?;
                Some(FileChange {
                    path: to_path(&path),
                    old_path: a.old_path.as_ref().map(to_path),
                    kind,
                })
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(ChangeSet { files })
    }

    fn load(&self, side: Side, change: &FileChange, max_bytes: u64) -> Result<Loaded> {
        match side {
            Side::Old => {
                if !change.has_old_side() {
                    return Ok(Loaded::Text(Arc::from(Vec::new())));
                }
                let repo = self.repo.to_thread_local();
                let mut tree = repo.head_tree()?;
                let entry = tree
                    .peel_to_entry_by_path(change.old_lookup_path())?
                    .ok_or_else(|| {
                        anyhow!(
                            "HEAD に {} が見つからない",
                            change.old_lookup_path().display()
                        )
                    })?;
                let object = entry.object()?;
                Ok(classify(object.data.as_slice(), max_bytes))
            }
            Side::New => {
                if !change.has_new_side() {
                    return Ok(Loaded::Text(Arc::from(Vec::new())));
                }
                let full = self.workdir.join(&change.path);
                read_worktree_file(&full, max_bytes)
            }
        }
    }
}

/// 作業ツリーのファイルを、サイズ上限とバイナリ判定を適用して読む。
pub fn read_worktree_file(path: &Path, max_bytes: u64) -> Result<Loaded> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("{} の情報を取得できない", path.display()))?;
    if meta.len() > max_bytes {
        return Ok(Loaded::Unsupported(UnsupportedReason::TooLarge {
            size: meta.len(),
            limit: max_bytes,
        }));
    }
    let bytes =
        std::fs::read(path).with_context(|| format!("{} を読み込めない", path.display()))?;
    Ok(classify(&bytes, max_bytes))
}

fn classify(bytes: &[u8], max_bytes: u64) -> Loaded {
    let size = bytes.len() as u64;
    if size > max_bytes {
        return Loaded::Unsupported(UnsupportedReason::TooLarge {
            size,
            limit: max_bytes,
        });
    }
    if looks_binary(bytes) {
        return Loaded::Unsupported(UnsupportedReason::Binary { size });
    }
    Loaded::Text(Arc::from(bytes))
}
