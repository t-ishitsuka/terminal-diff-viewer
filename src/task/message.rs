use std::path::PathBuf;
use std::sync::Arc;

use ratatui::crossterm::event::Event;

use crate::diff::{AlignedDiff, LineTable};
use crate::git::{ChangeSet, FileChange, HeadInfo, UnsupportedReason};
use crate::highlight::Highlighted;
use crate::vfs::DirEntry;

/// シンタックスハイライトの指定。無効なら None。
pub type HighlightOptions = Option<(String, usize)>;

#[derive(Clone, Debug)]
pub enum TaskRequest {
    ScanStatus {
        generation: u64,
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
        highlight: HighlightOptions,
    },
    ComputeDiff {
        generation: u64,
        change: FileChange,
        highlight: HighlightOptions,
    },
}

#[derive(Clone, Debug)]
pub struct StatusOutcome {
    pub changes: ChangeSet,
    pub head: HeadInfo,
}

#[derive(Clone, Debug)]
pub enum Content {
    Ready {
        diff: Box<AlignedDiff>,
        old_highlight: Option<Arc<Highlighted>>,
        new_highlight: Option<Arc<Highlighted>>,
    },
    Unsupported(UnsupportedReason),
}

#[derive(Clone, Debug)]
pub enum TextOutcome {
    Ready {
        table: LineTable,
        highlight: Option<Arc<Highlighted>>,
    },
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
}

pub enum AppEvent {
    Input(Event),
    Task(TaskResult),
}
