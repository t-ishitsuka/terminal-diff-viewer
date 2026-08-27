use std::path::Path;

use anyhow::{Context, Result};
use ignore::WalkBuilder;

/// ツリーに表示するエントリの種別。色分けの基準になる。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Dir,
    Symlink,
    Executable,
    File,
}

impl EntryKind {
    pub fn is_dir(self) -> bool {
        self == EntryKind::Dir
    }
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn classify(entry: &ignore::DirEntry) -> EntryKind {
    let Some(file_type) = entry.file_type() else {
        return EntryKind::File;
    };
    // follow_links(false) のため、シンボリックリンクはリンクとして報告される
    if file_type.is_symlink() {
        return EntryKind::Symlink;
    }
    if file_type.is_dir() {
        return EntryKind::Dir;
    }
    match entry.metadata() {
        Ok(metadata) if is_executable(&metadata) => EntryKind::Executable,
        _ => EntryKind::File,
    }
}

/// 1 階層だけ読む。全階層を一度に走査すると大規模リポジトリで起動が遅くなるため、
/// ツリーの遅延展開に合わせて呼び出す。
pub fn read_dir(dir: &Path, show_ignored: bool) -> Result<Vec<DirEntry>> {
    let mut builder = WalkBuilder::new(dir);
    builder.max_depth(Some(1)).follow_links(false);
    if show_ignored {
        builder.standard_filters(false);
    } else {
        builder.hidden(false);
    }

    let mut entries = Vec::new();
    for result in builder.build() {
        let entry = result.with_context(|| format!("{} を走査できない", dir.display()))?;
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        entries.push(DirEntry {
            name,
            kind: classify(&entry),
        });
    }

    entries.sort_by(|a, b| {
        b.kind
            .is_dir()
            .cmp(&a.kind.is_dir())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}
