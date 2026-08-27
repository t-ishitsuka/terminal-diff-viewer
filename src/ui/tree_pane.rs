use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text::{TextOpts, fit_width, pad_to};
use super::theme::Theme;
use crate::app::{App, Focus};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let opts = app.cfg.text;
    let focused = app.focus == Focus::Tree;
    let width = area.width as usize;
    let height = area.height as usize;
    if height == 0 || width == 0 {
        return;
    }

    let tree = app.tree();
    tree.scroll_into_view(height);
    let offset = tree.offset();
    let selected = tree.selected_index();
    let ids: Vec<u32> = tree
        .visible()
        .iter()
        .skip(offset)
        .take(height)
        .copied()
        .collect();

    if ids.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled("(表示するものがない)", theme.dim)),
            area,
        );
        return;
    }

    let mut lines: Vec<Line> = Vec::with_capacity(ids.len());
    for (row, id) in ids.iter().enumerate() {
        let node = tree.node(*id);
        let icon = if node.is_dir {
            if node.expanded { "▾ " } else { "▸ " }
        } else {
            ""
        };
        let label = format!("{}{icon}{}", "  ".repeat(node.depth as usize), node.name);
        let label = fit_width(&label, width.saturating_sub(2), opts);
        let label = pad_to(&label, width.saturating_sub(2), opts);

        let marker = node.status.map_or(' ', |k| k.marker());
        let marker_style = node
            .status
            .map_or(theme.dim, |kind| theme.change_style(kind));

        let row_style = if offset + row == selected {
            if focused {
                theme.selection
            } else {
                theme.selection_blur
            }
        } else {
            Style::new()
        };

        lines.push(
            Line::from(vec![
                Span::styled(marker.to_string(), marker_style),
                Span::raw(" "),
                Span::raw(label),
            ])
            .style(row_style),
        );
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// 行番号などの桁揃えに使う、内容ペインと共通のガター幅計算。
pub fn gutter_width(total_lines: usize, opts: TextOpts) -> usize {
    let _ = opts;
    let digits = total_lines.max(1).to_string().len();
    digits.max(3)
}
