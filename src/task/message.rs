use std::path::PathBuf;

use ratatui::crossterm::event::Event;

use crate::diff::AlignedDiff;
use crate::git::{ChangeSet, FileChange, HeadInfo, UnsupportedReason};
use crate::vfs::DirEntry;

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
    },
    ComputeDiff {
        generation: u64,
        change: FileChange,
    },
}

#[derive(Clone, Debug)]
pub struct StatusOutcome {
    pub changes: ChangeSet,
    pub head: HeadInfo,
}

#[derive(Clone, Debug)]
pub enum Content {
    Ready(AlignedDiff),
    Unsupported(UnsupportedReason),
}

#[derive(Clone, Debug)]
pub enum TextOutcome {
    Ready(crate::diff::LineTable),
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
