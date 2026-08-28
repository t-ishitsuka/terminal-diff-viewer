use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text::fit_width;
use super::theme::Theme;
use crate::app::{App, ContentState, InputKind, Mode, Overlay};

pub fn draw_header(
    frame: &mut Frame,
    cols: &[Rect],
    app: &mut App,
    theme: &Theme,
    show_tree: bool,
) {
    let opts = app.cfg.text;
    let repo = app
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| app.root.display().to_string());
    let mode = match app.mode {
        Mode::Tree => "TREE",
        Mode::Diff => "DIFF",
    };

    if show_tree {
        let text = fit_width(
            &format!("{mode}  {repo}"),
            cols[0].width.saturating_sub(1) as usize,
            opts,
        );
        frame.render_widget(Paragraph::new(Line::styled(text, theme.header)), cols[0]);
    }

    let right = *cols.last().expect("列は 1 つ以上");
    let path = app
        .content
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| if show_tree { String::new() } else { repo });
    let stat = match &app.content {
        ContentState::Diff(view) => {
            format!("  +{} -{}", view.diff.stat.added, view.diff.stat.removed)
        }
        _ => String::new(),
    };
    let text = fit_width(&format!("{path}{stat}"), right.width as usize, opts);
    frame.render_widget(Paragraph::new(Line::styled(text, theme.header)), right);
}

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let width = area.width as usize;
    let opts = app.cfg.text;

    // 入力中はプロンプトを最優先で出す
    if let Overlay::Input { kind, buffer } = &app.overlay {
        let prefix = match kind {
            InputKind::Search => "/",
            InputKind::Filter => "絞り込み: ",
        };
        let hits = if *kind == InputKind::Search && app.search.is_active() {
            format!("  ({} 件)", app.search.hits.len())
        } else {
            String::new()
        };
        let text = fit_width(&format!("{prefix}{buffer}▏{hits}"), width, opts);
        frame.render_widget(Paragraph::new(Line::styled(text, theme.notice)), area);
        return;
    }

    if let Some(notice) = app.notice.clone() {
        let text = fit_width(&notice, width, opts);
        frame.render_widget(Paragraph::new(Line::styled(text, theme.notice)), area);
        return;
    }

    let branch = app
        .head
        .as_ref()
        .map(|h| h.name.clone())
        .unwrap_or_else(|| "-".into());
    let changed = app.changes.files.len();
    let scanning = if app.scanning { " (走査中)" } else { "" };

    let position = match &app.content {
        ContentState::Diff(view) => {
            let row = view.anchor_row(app.content_height) as usize;
            match view.hunk_cursor.or_else(|| view.diff.hunk_index_at(row)) {
                Some(i) => format!("hunk {}/{}", i + 1, view.diff.hunks.len()),
                None => format!("{} 箇所", view.diff.hunks.len()),
            }
        }
        ContentState::Text(view) => format!("{}行目", view.offset + 1),
        _ => String::new(),
    };

    let search = if app.search.is_active() {
        format!(
            "  /{} {}/{}",
            app.search.query,
            (app.search.current + 1).min(app.search.hits.len().max(1)),
            app.search.hits.len()
        )
    } else {
        String::new()
    };

    let hints = match app.mode {
        Mode::Tree => "[Tab] ペイン  [m] diff  [/] 絞込  [?] ヘルプ",
        Mode::Diff => "[]c] 次の変更  [/] 検索  [z] 折畳  [?] ヘルプ",
    };

    let text = fit_width(
        &format!("{branch} ● {changed} 件変更{scanning}   {position}{search}   {hints}"),
        width,
        opts,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, theme.status))),
        area,
    );
}
