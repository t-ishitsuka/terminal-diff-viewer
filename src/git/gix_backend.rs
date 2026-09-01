use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use gix::bstr::{BStr, BString, ByteSlice};

use super::model::*;
use super::{DiffSpec, GitBackend};
use crate::vfs::walker::is_executable;

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
/// パスごとに集約する。どちらを使うかは比較対象で決まる。
#[derive(Default, Clone)]
struct Acc {
    tree: Option<TreeObservation>,
    worktree: Option<WorktreeObservation>,
    tree_old_path: Option<BString>,
    worktree_old_path: Option<BString>,
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
    fn resolve(&self, spec: &DiffSpec) -> Option<ChangeKind> {
        use TreeObservation as T;
        use WorktreeObservation as W;
        match spec {
            DiffSpec::WorktreeVsHead => self.resolve_worktree_vs_head(),
            // stage 済みの変更のみ。作業ツリー側の観測は無視する
            DiffSpec::StagedVsHead => self.tree.map(|t| match t {
                T::Addition => ChangeKind::Added,
                T::Deletion => ChangeKind::Deleted,
                T::Modification => ChangeKind::Modified,
                T::Rewrite => ChangeKind::Renamed,
            }),
            // 未 stage の変更のみ。index との差だけを見る
            DiffSpec::Range { .. } => None,
            DiffSpec::WorktreeVsIndex => self.worktree.map(|w| match w {
                W::Removed => ChangeKind::Deleted,
                W::Modified => ChangeKind::Modified,
                W::Untracked => ChangeKind::Untracked,
                W::Rewrite => ChangeKind::Renamed,
            }),
        }
    }

    /// docs/03-architecture.md §4.1 の統合表。None を返す場合は一覧から除外する。
    fn resolve_worktree_vs_head(&self) -> Option<ChangeKind> {
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

    /// リネーム元。比較対象ごとに参照する観測が変わる。
    fn old_path(&self, spec: &DiffSpec) -> Option<&BString> {
        match spec {
            DiffSpec::WorktreeVsHead => self
                .tree_old_path
                .as_ref()
                .or(self.worktree_old_path.as_ref()),
            DiffSpec::StagedVsHead => self.tree_old_path.as_ref(),
            DiffSpec::WorktreeVsIndex => self.worktree_old_path.as_ref(),
            DiffSpec::Range { .. } => None,
        }
    }
}

/// コミット履歴。
impl GixBackend {
    fn log_impl(&self, skip: usize, limit: usize) -> Result<Vec<CommitInfo>> {
        let repo = self.repo.to_thread_local();
        let Ok(head) = repo.head_id() else {
            // コミットが 1 つも無いリポジトリでは空の履歴を返す
            return Ok(Vec::new());
        };
        let walk = repo
            .rev_walk([head.detach()])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .context("コミットを辿れない")?;

        let mut out = Vec::with_capacity(limit);
        for info in walk.skip(skip).take(limit) {
            let info = info.context("コミットの読み取りに失敗")?;
            let commit = info.object().context("コミットを開けない")?;
            let id = info.id.to_hex().to_string();
            let short = info.id.to_hex_with_len(7).to_string();
            let subject = commit
                .message()
                .map(|m| m.summary().to_string())
                .unwrap_or_default();
            let (author, time) = match commit.author() {
                Ok(a) => {
                    let time = a
                        .time()
                        .map(|t| t.format_or_unix(gix::date::time::format::SHORT))
                        .unwrap_or_default();
                    (a.name.to_string(), time)
                }
                Err(_) => (String::new(), String::new()),
            };
            out.push(CommitInfo {
                id,
                short,
                subject,
                author,
                time,
            });
        }
        Ok(out)
    }

    /// コミット 1 件の差分を表す比較対象。親が無いコミットは空ツリーと比べる。
    fn commit_spec_impl(&self, id: &str) -> Result<DiffSpec> {
        let repo = self.repo.to_thread_local();
        let commit = repo
            .rev_parse_single(id)
            .with_context(|| format!("{id} を解決できない"))?
            .object()?
            .try_into_commit()
            .with_context(|| format!("{id} はコミットではない"))?;
        let from = commit
            .parent_ids()
            .next()
            .map(|p| p.detach().to_hex().to_string())
            .unwrap_or_default();
        Ok(DiffSpec::Range {
            from,
            to: commit.id().to_hex().to_string(),
        })
    }
}
/// ref 間比較。ツリー同士を比較するため作業ツリーも index も読まない。
impl GixBackend {
    fn changes_between(&self, from: &str, to: &str) -> Result<ChangeSet> {
        use gix::object::tree::diff::{Action, Change};

        let repo = self.repo.to_thread_local();
        let old = resolve_tree(&repo, from)?;
        let new = resolve_tree(&repo, to)?;

        let mut files: Vec<FileChange> = Vec::new();
        old.changes()?
            .for_each_to_obtain_tree(&new, |change| {
                let entry = match change {
                    Change::Addition {
                        location,
                        entry_mode,
                        ..
                    } => (!entry_mode.is_tree()).then_some((location, None, ChangeKind::Added)),
                    Change::Deletion {
                        location,
                        entry_mode,
                        ..
                    } => (!entry_mode.is_tree()).then_some((location, None, ChangeKind::Deleted)),
                    Change::Modification {
                        location,
                        entry_mode,
                        ..
                    } => (!entry_mode.is_tree()).then_some((location, None, ChangeKind::Modified)),
                    Change::Rewrite {
                        source_location,
                        location,
                        ..
                    } => Some((location, Some(source_location), ChangeKind::Renamed)),
                };
                if let Some((location, source, kind)) = entry {
                    files.push(FileChange {
                        path: gix::path::from_bstr(location).into_owned(),
                        old_path: source.map(|s| gix::path::from_bstr(s).into_owned()),
                        kind,
                    });
                }
                Ok::<_, std::convert::Infallible>(Action::Continue(()))
            })
            .with_context(|| format!("{from}..{to} の差分を取得できない"))?;

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(ChangeSet { files })
    }
}

/// revspec を解決してツリーを得る。
fn resolve_tree<'repo>(repo: &'repo gix::Repository, rev: &str) -> Result<gix::Tree<'repo>> {
    repo.rev_parse_single(rev)
        .with_context(|| format!("{rev} を解決できない"))?
        .object()?
        .peel_to_tree()
        .with_context(|| format!("{rev} はツリーへ辿れない"))
}

/// ツリーから blob を引く。
fn tree_blob(
    repo: &gix::Repository,
    rev: &str,
    rela_path: &Path,
    max_bytes: u64,
) -> Result<Loaded> {
    let mut tree = resolve_tree(repo, rev)?;
    let entry = tree
        .peel_to_entry_by_path(rela_path)?
        .ok_or_else(|| anyhow!("{rev} に {} が見つからない", rela_path.display()))?;
    let object = entry.object()?;
    Ok(classify(object.data.as_slice(), max_bytes))
}
/// index の書き換え。1 ファイル分の stage / unstage をまとめて扱う。
impl GixBackend {
    /// index を読み込み、編集し、書き戻す。書き込みは gix がロックを取って行う。
    fn edit_index(
        &self,
        edit: impl FnOnce(&gix::Repository, &mut gix::index::File) -> Result<()>,
    ) -> Result<()> {
        let repo = self.repo.to_thread_local();
        let mut index = (*repo.index_or_empty().context("index を読めない")?).clone();
        edit(&repo, &mut index)?;
        index.sort_entries();
        // エントリを変えるとツリーキャッシュが古くなるため落とす
        index.remove_tree();
        index
            .write(gix::index::write::Options::default())
            .context("index を書き込めない")?;
        Ok(())
    }
}

fn to_bstring(path: &Path) -> BString {
    gix::path::into_bstr(path).into_owned()
}

/// 同じパスのエントリを stage を問わず取り除く。競合中のエントリもまとめて消える。
fn remove_path(index: &mut gix::index::File, path: &BStr) {
    index.remove_entries(|_, entry_path, _| entry_path == path);
}

fn push_entry(
    index: &mut gix::index::File,
    path: &BStr,
    id: gix::ObjectId,
    mode: gix::index::entry::Mode,
    stat: gix::index::entry::Stat,
) {
    remove_path(index, path);
    index.dangerously_push_entry(stat, id, gix::index::entry::Flags::empty(), mode, path);
}

/// 作業ツリーの内容を blob として書き、index のエントリを作り直す。
fn stage_from_worktree(
    repo: &gix::Repository,
    index: &mut gix::index::File,
    abs: &Path,
    rela: &BStr,
) -> Result<()> {
    let meta = std::fs::symlink_metadata(abs)
        .with_context(|| format!("{} の情報を取得できない", abs.display()))?;
    let (id, mode) = if meta.is_symlink() {
        let target = std::fs::read_link(abs)
            .with_context(|| format!("{} のリンク先を読めない", abs.display()))?;
        let id = repo.write_blob(gix::path::into_bstr(target.as_path()).as_ref())?;
        (id.detach(), gix::index::entry::Mode::SYMLINK)
    } else {
        let bytes =
            std::fs::read(abs).with_context(|| format!("{} を読み込めない", abs.display()))?;
        let mode = if is_executable(&meta) {
            gix::index::entry::Mode::FILE_EXECUTABLE
        } else {
            gix::index::entry::Mode::FILE
        };
        (repo.write_blob(&bytes)?.detach(), mode)
    };
    // stat が実体とずれていても内容の比較で救えるが、揃えておけば status が速い
    let stat = gix::index::fs::Metadata::from_path_no_follow(abs)
        .ok()
        .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
        .unwrap_or_default();
    push_entry(index, rela, id, mode, stat);
    Ok(())
}

/// HEAD のエントリで index を上書きする。HEAD に無ければ index からも消す。
fn restore_from_head(
    repo: &gix::Repository,
    index: &mut gix::index::File,
    rela_path: &Path,
) -> Result<()> {
    let rela = to_bstring(rela_path);
    let entry = match repo.head_tree() {
        Ok(mut tree) => tree.peel_to_entry_by_path(rela_path)?,
        // コミットが 1 つも無いリポジトリでは HEAD 側が存在しない
        Err(_) => None,
    };
    match entry {
        Some(entry) => {
            let mode = match entry.mode().kind() {
                gix::object::tree::EntryKind::BlobExecutable => {
                    gix::index::entry::Mode::FILE_EXECUTABLE
                }
                gix::object::tree::EntryKind::Link => gix::index::entry::Mode::SYMLINK,
                _ => gix::index::entry::Mode::FILE,
            };
            // 実体と一致するか分からないので stat は空にし、status 側で内容比較させる
            push_entry(
                index,
                rela.as_ref(),
                entry.object_id(),
                mode,
                gix::index::entry::Stat::default(),
            );
        }
        None => remove_path(index, rela.as_ref()),
    }
    Ok(())
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

    fn changes(&self, spec: &DiffSpec) -> Result<ChangeSet> {
        if let Some((from, to)) = spec.range() {
            return self.changes_between(from, to);
        }
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
                        e.tree_old_path = Some(source_location.into_owned());
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
                        e.worktree_old_path = old;
                    }
                },
            }
        }

        let mut files: Vec<FileChange> = acc
            .into_iter()
            .filter_map(|(path, a)| {
                let kind = a.resolve(spec)?;
                Some(FileChange {
                    path: to_path(&path),
                    old_path: a.old_path(spec).map(to_path),
                    kind,
                })
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(ChangeSet { files })
    }

    fn load(
        &self,
        spec: &DiffSpec,
        side: Side,
        change: &FileChange,
        max_bytes: u64,
    ) -> Result<Loaded> {
        let empty = || Ok(Loaded::Text(Arc::from(Vec::new())));
        match side {
            Side::Old => {
                if !change.has_old_side() {
                    return empty();
                }
                let repo = self.repo.to_thread_local();
                let path = change.old_lookup_path();
                match spec.range() {
                    Some((from, _)) => tree_blob(&repo, from, path, max_bytes),
                    None if spec.old_is_index() => index_blob(&repo, path, max_bytes),
                    None => tree_blob(&repo, "HEAD", path, max_bytes),
                }
            }
            Side::New => {
                if !change.has_new_side() {
                    return empty();
                }
                match spec.range() {
                    Some((_, to)) => {
                        let repo = self.repo.to_thread_local();
                        tree_blob(&repo, to, &change.path, max_bytes)
                    }
                    None if spec.new_is_index() => {
                        let repo = self.repo.to_thread_local();
                        index_blob(&repo, &change.path, max_bytes)
                    }
                    None => read_worktree_file(&self.workdir.join(&change.path), max_bytes),
                }
            }
        }
    }

    fn log(&self, skip: usize, limit: usize) -> Result<Vec<CommitInfo>> {
        self.log_impl(skip, limit)
    }

    fn commit_spec(&self, id: &str) -> Result<DiffSpec> {
        self.commit_spec_impl(id)
    }

    fn stage(&self, change: &FileChange) -> Result<()> {
        let workdir = self.workdir.clone();
        self.edit_index(|repo, index| {
            // リネームは新パスを足して旧パスを落とす
            if let Some(old) = &change.old_path {
                remove_path(index, to_bstring(old).as_ref());
            }
            let abs = workdir.join(&change.path);
            let rela = to_bstring(&change.path);
            if abs.symlink_metadata().is_ok() {
                stage_from_worktree(repo, index, &abs, rela.as_ref())
            } else {
                // 作業ツリーから消えているなら、削除を index へ反映する
                remove_path(index, rela.as_ref());
                Ok(())
            }
        })
    }

    fn unstage(&self, change: &FileChange) -> Result<()> {
        self.edit_index(|repo, index| {
            if let Some(old) = &change.old_path {
                restore_from_head(repo, index, old)?;
            }
            restore_from_head(repo, index, &change.path)
        })
    }
}

/// index に登録されている内容 (stage 済みの内容) を引く。
fn index_blob(repo: &gix::Repository, rela_path: &Path, max_bytes: u64) -> Result<Loaded> {
    let index = repo.index().context("index を読めない")?;
    let path = gix::path::into_bstr(rela_path);
    let entry = index
        .entry_by_path(path.as_ref())
        .ok_or_else(|| anyhow!("index に {} が見つからない", rela_path.display()))?;
    let object = repo.find_object(entry.id)?;
    Ok(classify(object.data.as_slice(), max_bytes))
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

#[cfg(test)]
mod tests {
    use super::*;
    use TreeObservation as T;
    use WorktreeObservation as W;

    fn acc(tree: Option<T>, worktree: Option<W>) -> Acc {
        Acc {
            tree,
            worktree,
            ..Acc::default()
        }
    }

    /// tree 観測、worktree 観測、比較対象 3 種それぞれで期待する種別。
    type Case = (
        Option<T>,
        Option<W>,
        Option<ChangeKind>,
        Option<ChangeKind>,
        Option<ChangeKind>,
    );

    /// (HEAD↔index, index↔作業ツリー) の観測から、比較対象ごとに決まる種別。
    #[test]
    fn observations_resolve_per_spec() {
        use ChangeKind::*;
        let cases: &[Case] = &[
            // tree, worktree, WorktreeVsHead, StagedVsHead, WorktreeVsIndex
            (Some(T::Addition), None, Some(Added), Some(Added), None),
            (
                Some(T::Addition),
                Some(W::Modified),
                Some(Added),
                Some(Added),
                Some(Modified),
            ),
            // index へ追加後に作業ツリーから消すと HEAD からは存在しないまま
            (
                Some(T::Addition),
                Some(W::Removed),
                None,
                Some(Added),
                Some(Deleted),
            ),
            (Some(T::Deletion), None, Some(Deleted), Some(Deleted), None),
            (
                Some(T::Modification),
                None,
                Some(Modified),
                Some(Modified),
                None,
            ),
            (
                Some(T::Modification),
                Some(W::Modified),
                Some(Modified),
                Some(Modified),
                Some(Modified),
            ),
            (
                None,
                Some(W::Modified),
                Some(Modified),
                None,
                Some(Modified),
            ),
            (None, Some(W::Removed), Some(Deleted), None, Some(Deleted)),
            (
                None,
                Some(W::Untracked),
                Some(Untracked),
                None,
                Some(Untracked),
            ),
            (Some(T::Rewrite), None, Some(Renamed), Some(Renamed), None),
            (None, Some(W::Rewrite), Some(Renamed), None, Some(Renamed)),
            (None, None, None, None, None),
        ];

        for (tree, worktree, head, staged, index) in cases {
            let a = acc(*tree, *worktree);
            assert_eq!(
                a.resolve(&DiffSpec::WorktreeVsHead),
                *head,
                "{:?}",
                (tree.is_some(), worktree.is_some())
            );
            assert_eq!(a.resolve(&DiffSpec::StagedVsHead), *staged);
            assert_eq!(a.resolve(&DiffSpec::WorktreeVsIndex), *index);
        }
    }

    /// リネーム元は比較対象ごとに参照する観測が変わる。
    #[test]
    fn rename_source_follows_the_spec() {
        let a = Acc {
            tree: Some(T::Rewrite),
            worktree: Some(W::Rewrite),
            tree_old_path: Some(BString::from("old-in-head")),
            worktree_old_path: Some(BString::from("old-in-index")),
        };
        assert_eq!(a.old_path(&DiffSpec::StagedVsHead).unwrap(), "old-in-head");
        assert_eq!(
            a.old_path(&DiffSpec::WorktreeVsIndex).unwrap(),
            "old-in-index"
        );
        assert_eq!(
            a.old_path(&DiffSpec::WorktreeVsHead).unwrap(),
            "old-in-head"
        );

        let only_worktree = Acc {
            worktree: Some(W::Rewrite),
            worktree_old_path: Some(BString::from("old-in-index")),
            ..Acc::default()
        };
        assert_eq!(
            only_worktree.old_path(&DiffSpec::WorktreeVsHead).unwrap(),
            "old-in-index"
        );
    }
}
