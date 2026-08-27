use std::path::Path;

use anyhow::{Context, Result};
use ignore::WalkBuilder;

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
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
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        entries.push(DirEntry { name, is_dir });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}
