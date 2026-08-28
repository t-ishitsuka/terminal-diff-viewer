use std::path::PathBuf;
use std::sync::Arc;

/// 作業ツリー vs HEAD で観測される変更の種別。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

impl ChangeKind {
    pub fn marker(self) -> char {
        match self {
            ChangeKind::Added => 'A',
            ChangeKind::Modified => 'M',
            ChangeKind::Deleted => 'D',
            ChangeKind::Renamed => 'R',
            ChangeKind::Untracked => '?',
        }
    }

    /// 種別順に並べるときの序列。
    pub fn order(self) -> u8 {
        match self {
            ChangeKind::Modified => 0,
            ChangeKind::Added => 1,
            ChangeKind::Renamed => 2,
            ChangeKind::Deleted => 3,
            ChangeKind::Untracked => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileChange {
    /// リポジトリルートからの相対パス。
    pub path: PathBuf,
    /// リネーム元。リネーム以外では None。
    pub old_path: Option<PathBuf>,
    pub kind: ChangeKind,
}

impl FileChange {
    /// HEAD 側を引くときのパス。リネームなら旧パスを使う。
    pub fn old_lookup_path(&self) -> &PathBuf {
        self.old_path.as_ref().unwrap_or(&self.path)
    }

    pub fn has_old_side(&self) -> bool {
        !matches!(self.kind, ChangeKind::Added | ChangeKind::Untracked)
    }

    pub fn has_new_side(&self) -> bool {
        self.kind != ChangeKind::Deleted
    }
}

#[derive(Clone, Default, Debug)]
pub struct ChangeSet {
    pub files: Vec<FileChange>,
}

impl ChangeSet {
    pub fn find(&self, path: &std::path::Path) -> Option<&FileChange> {
        self.files.iter().find(|c| c.path == path)
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct LineStat {
    pub added: u32,
    pub removed: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

/// 差分計算・表示ができない理由。仕様上の正常な結果であり、エラーとは区別する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedReason {
    Binary { size: u64 },
    TooLarge { size: u64, limit: u64 },
}

impl UnsupportedReason {
    pub fn describe(self) -> String {
        match self {
            UnsupportedReason::Binary { size } => {
                format!("バイナリファイル ({})", human_size(size))
            }
            UnsupportedReason::TooLarge { size, limit } => format!(
                "サイズ上限を超過 ({} > {})",
                human_size(size),
                human_size(limit)
            ),
        }
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 読み込んだファイルの中身。
#[derive(Clone, Debug)]
pub enum Loaded {
    Text(Arc<[u8]>),
    Unsupported(UnsupportedReason),
}

/// 先頭 8000 バイトに NUL があればバイナリとみなす (git と同じ判定)。
pub fn looks_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8000)];
    head.contains(&0)
}

#[derive(Clone, Debug)]
pub struct HeadInfo {
    /// ブランチ名。detached HEAD なら短縮ハッシュ。
    pub name: String,
}
