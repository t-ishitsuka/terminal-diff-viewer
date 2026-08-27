use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text::fit_width;
use super::theme::Theme;
use crate::app::{App, ContentState, Mode};

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
    if let Some(notice) = app.notice.clone() {
        let text = fit_width(&notice, area.width as usize, app.cfg.text);
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
            let row = view.row_at_display(view.offset) as usize;
            match view.diff.hunk_index_at(row) {
                Some(i) => format!("hunk {}/{}", i + 1, view.diff.hunks.len()),
                None => format!("{} 箇所", view.diff.hunks.len()),
            }
        }
        ContentState::Text(view) => format!("{}行目", view.offset + 1),
        _ => String::new(),
    };

    let hints = match app.mode {
        Mode::Tree => "[Tab] ペイン  [m] diff  [r] 再読込  [?] ヘルプ",
        Mode::Diff => "[]c] 次の変更  [z] 折畳  [u] 統合表示  [?] ヘルプ",
    };

    let text = fit_width(
        &format!("{branch} ● {changed} 件変更{scanning}   {position}   {hints}"),
        area.width as usize,
        app.cfg.text,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, theme.status))),
        area,
    );
}
