use std::path::PathBuf;

use super::action::Action;
use super::state::{App, ContentState, DiffView, DisplayRow, Focus, Mode, Overlay, TextView};
use crate::task::{Content, TaskResult, TextOutcome};
use crate::vfs::{Node, TreeModel};

pub fn apply(app: &mut App, action: Action) {
    match action {
        Action::None => {}
        Action::Quit => app.should_quit = true,
        Action::Escape => {
            app.overlay = Overlay::None;
            app.notice = None;
        }
        Action::ToggleHelp => {
            app.overlay = if app.overlay == Overlay::Help {
                Overlay::None
            } else {
                Overlay::Help
            };
        }
        Action::ToggleMode => {
            let next = if app.mode == Mode::Tree {
                Mode::Diff
            } else {
                Mode::Tree
            };
            set_mode(app, next);
        }
        Action::SetMode(mode) => set_mode(app, mode),
        Action::CycleFocus(_) => {
            app.focus = if app.focus == Focus::Tree {
                Focus::Content
            } else {
                Focus::Tree
            };
        }
        Action::Reload => reload(app),
        Action::ResizeTree(delta) => {
            app.tree_ratio = (app.tree_ratio as i16 + delta).clamp(1, 8) as u16;
        }

        Action::TreeMove(delta) => {
            app.tree().move_selection(delta);
            app.request_content();
        }
        Action::TreeHalfPage(dir) => {
            let step = (app.tree_height / 2).max(1) as isize;
            app.tree().move_selection(step * dir as isize);
            app.request_content();
        }
        Action::TreeFirst => {
            app.tree().select_first();
            app.request_content();
        }
        Action::TreeLast => {
            app.tree().select_last();
            app.request_content();
        }
        Action::TreeOpen => tree_open(app),
        Action::TreeCollapse => tree_collapse(app),
        Action::TreeToggle => {
            if let Some(id) = app.tree().selected_id() {
                let is_dir = app.tree().node(id).is_dir;
                if is_dir {
                    app.tree().toggle(id);
                    load_children_if_needed(app, id);
                }
            }
        }
        Action::ToggleIgnored => {
            app.show_ignored = !app.show_ignored;
            reload_fs_tree(app);
        }
        Action::ToggleHierarchy => {
            app.hierarchical_changes = !app.hierarchical_changes;
            app.rebuild_change_tree();
            if app.mode == Mode::Diff {
                app.request_content();
            }
        }

        Action::ContentScroll(delta) => scroll(app, delta),
        Action::ContentHalfPage(dir) => {
            let step = (app.content_height / 2).max(1) as isize;
            scroll(app, step * dir as isize);
        }
        Action::ContentPage(dir) => {
            let step = app.content_height.max(1) as isize;
            scroll(app, step * dir as isize);
        }
        Action::ContentFirst => set_offset(app, 0),
        Action::ContentLast => {
            let last = content_len(app).saturating_sub(app.content_height.max(1));
            set_offset(app, last);
        }
        Action::ContentHScroll(delta) => hscroll(app, delta),
        Action::NextHunk => jump_hunk(app, true),
        Action::PrevHunk => jump_hunk(app, false),
        Action::NextFile => step_change_file(app, 1),
        Action::PrevFile => step_change_file(app, -1),
        Action::ToggleFold => toggle_fold(app),
        Action::ExpandGap => expand_gap(app),
    }
}

fn set_mode(app: &mut App, mode: Mode) {
    if app.mode == mode {
        return;
    }
    if mode == Mode::Diff && app.backend.is_none() {
        app.notice = Some("Git リポジトリではないため diff モードを使えない".into());
        return;
    }
    let current = app.tree().selected_node().map(|n| n.path.clone());
    app.mode = mode;
    // 切り替え前の選択を引き継ぐ。対象外なら先頭を選ぶ
    if let Some(path) = current
        && let Some(id) = app.tree().find_by_path(&path)
    {
        app.tree().reveal(id);
    }
    if app.tree().selected_id().is_none() {
        app.tree().select_first();
    }
    app.request_content();
}

fn reload(app: &mut App) {
    app.request_status();
    reload_fs_tree(app);
    app.request_content();
}

/// 展開状態と選択を保ったままファイルツリーを作り直す。
fn reload_fs_tree(app: &mut App) {
    app.pending_expand = app.fs_tree.expanded_dir_paths().into_iter().collect();
    app.pending_select = app.fs_tree.selected_node().map(|n| n.path.clone());
    app.fs_tree.clear();
    app.request_root_dir();
}

fn tree_open(app: &mut App) {
    let Some(id) = app.tree().selected_id() else {
        return;
    };
    if app.tree().node(id).is_dir {
        app.tree().expand(id);
        load_children_if_needed(app, id);
    } else {
        app.request_content();
        app.focus = Focus::Content;
    }
}

fn tree_collapse(app: &mut App) {
    let Some(id) = app.tree().selected_id() else {
        return;
    };
    let node = app.tree().node(id);
    if node.is_dir && node.expanded {
        app.tree().collapse(id);
    } else if let Some(parent) = app.tree().parent_of(id) {
        app.tree().select_node(parent);
        app.request_content();
    }
}

fn load_children_if_needed(app: &mut App, id: u32) {
    if app.mode != Mode::Tree || app.tree().children_loaded(id) {
        return;
    }
    let rel = app.fs_tree.node(id).path.clone();
    app.request_dir(id, &rel);
}

fn content_len(app: &App) -> usize {
    match &app.content {
        ContentState::Text(v) => v.table.len(),
        ContentState::Diff(v) => v.display_len(),
        _ => 0,
    }
}

fn set_offset(app: &mut App, value: usize) {
    let max = content_len(app).saturating_sub(app.content_height.max(1));
    let value = value.min(max);
    match &mut app.content {
        ContentState::Text(v) => v.offset = value,
        ContentState::Diff(v) => v.offset = value,
        _ => {}
    }
}

fn scroll(app: &mut App, delta: isize) {
    let current = match &app.content {
        ContentState::Text(v) => v.offset,
        ContentState::Diff(v) => v.offset,
        _ => return,
    };
    let next = (current as isize + delta).max(0) as usize;
    set_offset(app, next);
}

fn hscroll(app: &mut App, delta: isize) {
    let target = match &mut app.content {
        ContentState::Text(v) => &mut v.hscroll,
        ContentState::Diff(v) => &mut v.hscroll,
        _ => return,
    };
    *target = (*target as isize + delta).max(0) as usize;
}

fn jump_hunk(app: &mut App, forward: bool) {
    let height = app.content_height.max(1);
    let ContentState::Diff(view) = &mut app.content else {
        return;
    };
    let current = view.row_at_display(view.offset) as usize;
    let target = if forward {
        view.diff.next_hunk_row(current)
    } else {
        view.diff.prev_hunk_row(current)
    };
    let Some(row) = target else {
        app.notice = Some(if forward {
            "これ以降に変更箇所はない".into()
        } else {
            "これ以前に変更箇所はない".into()
        });
        return;
    };
    // 変更ブロックの先頭が画面上部から 1/4 の位置に来るようにする
    let index = view.display_index_of_row(row as u32);
    let offset = index.saturating_sub(height / 4);
    view.offset = offset;
    let max = view.display_len().saturating_sub(height);
    view.offset = view.offset.min(max);
}

fn step_change_file(app: &mut App, delta: isize) {
    if app.mode != Mode::Diff {
        return;
    }
    let mut moved = false;
    for _ in 0..app.change_tree.visible_len() {
        app.change_tree.move_selection(delta);
        let is_file = app.change_tree.selected_node().is_some_and(|n| !n.is_dir);
        if is_file {
            moved = true;
            break;
        }
    }
    if moved {
        app.request_content();
    }
}

fn toggle_fold(app: &mut App) {
    let context = app.cfg.fold_context;
    let ContentState::Diff(view) = &mut app.content else {
        return;
    };
    let anchor = view.row_at_display(view.offset);
    view.folded = !view.folded;
    view.rebuild_display(context);
    view.offset = view.display_index_of_row(anchor);
}

fn expand_gap(app: &mut App) {
    let context = app.cfg.fold_context;
    let ContentState::Diff(view) = &mut app.content else {
        return;
    };
    if !view.folded {
        return;
    }
    if let Some(DisplayRow::Gap { start, .. }) = view.display_row(view.offset) {
        view.expanded_gaps.insert(start);
        view.rebuild_display(context);
        view.offset = view.display_index_of_row(start);
    }
}

pub fn on_task(app: &mut App, result: TaskResult) {
    match result {
        TaskResult::Status {
            generation,
            outcome,
        } => {
            if generation != app.status_generation {
                return;
            }
            app.scanning = false;
            match outcome {
                Ok(status) => {
                    app.changes = status.changes;
                    app.head = Some(status.head);
                    app.rebuild_change_tree();
                    app.apply_status_to_fs_tree();
                    if app.mode == Mode::Diff && app.content_is_empty() {
                        app.tree().select_first();
                        app.request_content();
                    }
                }
                Err(error) => app.notice = Some(format!("status の取得に失敗: {error}")),
            }
        }

        TaskResult::Dir {
            node,
            outcome,
            generation,
        } => {
            let _ = generation;
            match outcome {
                Ok(entries) => {
                    if app.fs_tree.node_count() <= node as usize {
                        return;
                    }
                    if app.fs_tree.children_count(node) > 0 {
                        return;
                    }
                    let parent_path = app.fs_tree.node(node).path.clone();
                    let depth = if node == TreeModel::ROOT {
                        0
                    } else {
                        app.fs_tree.node(node).depth + 1
                    };
                    let mut to_expand: Vec<(u32, PathBuf)> = Vec::new();
                    let mut to_select: Option<u32> = None;
                    for entry in entries {
                        let path = parent_path.join(&entry.name);
                        let child = if entry.is_dir {
                            Node::dir(entry.name, path.clone(), depth, Some(node))
                        } else {
                            Node::file(entry.name, path.clone(), depth, Some(node))
                        };
                        let id = app.fs_tree.push_child(node, child);
                        if entry.is_dir && app.pending_expand.contains(&path) {
                            app.fs_tree.expand(id);
                            to_expand.push((id, path.clone()));
                        }
                        if app.pending_select.as_deref() == Some(path.as_path()) {
                            to_select = Some(id);
                        }
                    }
                    app.fs_tree.mark_loaded(node);
                    app.apply_status_to_fs_tree();
                    for (id, path) in to_expand {
                        app.request_dir(id, &path);
                    }
                    if let Some(id) = to_select {
                        app.pending_select = None;
                        app.fs_tree.reveal(id);
                        if app.mode == Mode::Tree {
                            app.request_content();
                        }
                    }
                }
                Err(error) => app.notice = Some(format!("ディレクトリを読めない: {error}")),
            }
        }

        TaskResult::Text {
            generation,
            path,
            outcome,
        } => {
            if generation != app.generation {
                return;
            }
            app.content = match outcome {
                Ok(TextOutcome::Ready(table)) => ContentState::Text(TextView {
                    path,
                    table,
                    offset: 0,
                    hscroll: 0,
                }),
                Ok(TextOutcome::Unsupported(reason)) => ContentState::Unsupported { path, reason },
                Err(error) => ContentState::Failed { path, error },
            };
        }

        TaskResult::Diff {
            generation,
            change,
            outcome,
        } => {
            if generation != app.generation {
                return;
            }
            let path = change.path.clone();
            app.content = match outcome {
                Ok(Content::Ready(diff)) => ContentState::Diff(Box::new(DiffView::new(
                    change,
                    diff,
                    !app.cfg.full_file,
                    app.cfg.fold_context,
                ))),
                Ok(Content::Unsupported(reason)) => ContentState::Unsupported { path, reason },
                Err(error) => ContentState::Failed { path, error },
            };
        }
    }
}

impl App {
    fn content_is_empty(&self) -> bool {
        matches!(self.content, ContentState::Empty)
    }
}
