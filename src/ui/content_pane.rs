use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text::render_line;
use super::theme::Theme;
use super::tree_pane::gutter_width;
use crate::app::{App, ContentState};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let opts = app.cfg.text;
    let height = area.height as usize;
    let width = area.width as usize;
    if height == 0 || width == 0 {
        return;
    }

    let message = match &app.content {
        ContentState::Empty => Some(Line::styled(
            "ファイルを選択すると内容を表示する",
            theme.dim,
        )),
        ContentState::Loading { path } => Some(Line::styled(
            format!("読み込み中: {}", path.display()),
            theme.dim,
        )),
        ContentState::Unsupported { path, reason } => Some(Line::styled(
            format!("{}: {}", path.display(), reason.describe()),
            theme.notice,
        )),
        ContentState::Failed { path, error } => Some(Line::styled(
            format!("{} の読み込みに失敗: {error}", path.display()),
            theme.change_style(crate::git::ChangeKind::Deleted),
        )),
        _ => None,
    };
    if let Some(line) = message {
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let ContentState::Text(view) = &mut app.content else {
        return;
    };
    let total = view.table.len();
    let max_offset = total.saturating_sub(height);
    view.offset = view.offset.min(max_offset);

    let num_width = gutter_width(total, opts);
    let content_width = width.saturating_sub(num_width + 1);
    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for index in view.offset..(view.offset + height).min(total) {
        let raw = view.table.line_display(index as u32);
        let text = String::from_utf8_lossy(raw);
        let segments = render_line(&text, &[], opts, view.hscroll, content_width);
        let mut spans = vec![
            Span::styled(format!("{:>num_width$}", index + 1), theme.gutter),
            Span::raw(" "),
        ];
        spans.extend(segments.into_iter().map(|(t, _)| Span::raw(t)));
        lines.push(Line::from(spans).style(Style::new()));
    }
    frame.render_widget(Paragraph::new(lines), area);
}
