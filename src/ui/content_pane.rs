use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::render::spans;
use super::text::{Marks, render_line, wrap_line};
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
            theme.error,
        )),
        _ => None,
    };
    if let Some(line) = message {
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let search = &app.search;
    let wrap = app.wrap;
    let ContentState::Text(view) = &mut app.content else {
        return;
    };
    let total = view.table.len();
    // 折り返し中は 1 行が複数行を占めるため、末尾行まで送れるようにする
    let last = if wrap {
        total.saturating_sub(1)
    } else {
        total.saturating_sub(height)
    };
    view.offset = view.offset.min(last);

    let num_width = gutter_width(total);
    let content_width = width.saturating_sub(num_width + 1);
    let mut lines: Vec<Line> = Vec::with_capacity(height);
    let mut index = view.offset;
    while index < total && lines.len() < height {
        let raw = view.table.line_display(index as u32);
        let text = String::from_utf8_lossy(raw);
        let colors = view
            .highlight
            .as_ref()
            .map_or(&[][..], |h| h.line(index as u32));
        let marks = Marks {
            search: search.ranges(index as u32, false),
            colors,
            ..Marks::default()
        };
        let chunks = if wrap {
            wrap_line(&text, marks, opts, content_width)
        } else {
            vec![render_line(&text, marks, opts, view.hscroll, content_width)]
        };
        for (i, segments) in chunks.into_iter().enumerate() {
            // 折り返しの 2 行目以降は行番号を空にして桁を保つ
            let number = if i == 0 {
                format!("{:>num_width$}", index + 1)
            } else {
                " ".repeat(num_width)
            };
            let mut out = vec![Span::styled(number, theme.gutter), Span::raw(" ")];
            out.extend(spans(segments, Style::new(), Color::Reset, theme));
            lines.push(Line::from(out));
        }
        index += 1;
    }
    frame.render_widget(Paragraph::new(lines), area);
}
