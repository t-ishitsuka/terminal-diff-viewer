use std::path::PathBuf;
use std::sync::Arc;

use ratatui::crossterm::event::Event;

use crate::diff::{AlignedDiff, LineTable};
use crate::git::{ChangeSet, DiffSpec, FileChange, HeadInfo, UnsupportedReason};
use crate::highlight::Highlighted;
use crate::vfs::DirEntry;

/// シンタックスハイライトの指定。無効なら None。
pub type HighlightOptions = Option<(String, usize)>;

#[derive(Clone, Debug)]
pub enum TaskRequest {
    ScanStatus {
        generation: u64,
        spec: DiffSpec,
    },
    ReadDir {
        generation: u64,
        node: u32,
        dir: PathBuf,
        show_ignored: bool,
    },
    LoadText {
        generation: u64,
        path: PathBuf,
        abs: PathBuf,
    },
    ComputeDiff {
        generation: u64,
        change: FileChange,
        spec: DiffSpec,
    },
    /// 内容が出た後に走らせる色付け。重いので本文の表示を待たせない。
    Highlight {
        generation: u64,
        target: HighlightTarget,
        path: PathBuf,
        table: LineTable,
        theme: String,
        max_lines: usize,
        /// 先頭から何行目まで色付けするか。可視範囲の先行送信に使う。
        upto: usize,
    },
}

/// 色付け結果の適用先。差分は左右を別々に色付けする。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HighlightTarget {
    Text,
    DiffOld,
    DiffNew,
}

#[derive(Clone, Debug)]
pub struct StatusOutcome {
    pub changes: ChangeSet,
    pub head: HeadInfo,
}

#[derive(Clone, Debug)]
pub enum Content {
    Ready { diff: Box<AlignedDiff> },
    Unsupported(UnsupportedReason),
}

#[derive(Clone, Debug)]
pub enum TextOutcome {
    Ready { table: LineTable },
    Unsupported(UnsupportedReason),
}

#[derive(Debug)]
pub enum TaskResult {
    Status {
        generation: u64,
        outcome: Result<StatusOutcome, String>,
    },
    Dir {
        generation: u64,
        node: u32,
        outcome: Result<Vec<DirEntry>, String>,
    },
    Text {
        generation: u64,
        path: PathBuf,
        outcome: Result<TextOutcome, String>,
    },
    Diff {
        generation: u64,
        change: FileChange,
        outcome: Result<Content, String>,
    },
    Highlight {
        generation: u64,
        target: HighlightTarget,
        highlight: Option<Arc<Highlighted>>,
    },
}

pub enum AppEvent {
    Input(Event),
    Task(TaskResult),
}
