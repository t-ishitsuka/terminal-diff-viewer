use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::text::{TextOpts, render_line};
use super::theme::Theme;
use super::tree_pane::gutter_width;
use crate::app::{App, ContentState, DiffView, DisplayRow};
use crate::diff::{Cell, InlineSpans, LineTable, RowKind, RowPair};
use crate::git::Side;

/// 行末のマーカー。改行コードの差だけの変更も見えるようにする。
const CR_MARK: &str = "␍";
const NO_NEWLINE_MARK: &str = "¬";

pub fn draw_side_by_side(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let halves =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
    let block = Block::new().borders(Borders::RIGHT).border_style(theme.dim);
    let left_area = block.inner(halves[0]);
    frame.render_widget(block, halves[0]);
    let right_area = halves[1];

    let rows = build_rows(app, theme, left_area.width, right_area.width, true);
    let Some((left, right)) = rows else { return };
    frame.render_widget(Paragraph::new(left), left_area);
    frame.render_widget(Paragraph::new(right), right_area);
}

pub fn draw_unified(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let rows = build_rows(app, theme, area.width, 0, false);
    let Some((lines, _)) = rows else { return };
    frame.render_widget(Paragraph::new(lines), area);
}

type Rendered<'a> = (Vec<Line<'a>>, Vec<Line<'a>>);

fn build_rows<'a>(
    app: &mut App,
    theme: &Theme,
    left_width: u16,
    right_width: u16,
    side_by_side: bool,
) -> Option<Rendered<'a>> {
    let opts = app.cfg.text;
    let inline_enabled = app.cfg.inline_words;
    let height = app.content_height;
    if height == 0 {
        return None;
    }

    let ContentState::Diff(view) = &mut app.content else {
        return None;
    };
    let total = view.display_len();
    view.offset = view.offset.min(total.saturating_sub(height));

    let old_width = gutter_width(view.diff.old.len(), opts);
    let new_width = gutter_width(view.diff.new.len(), opts);
    let hscroll = view.hscroll;

    let mut left: Vec<Line> = Vec::with_capacity(height);
    let mut right: Vec<Line> = Vec::with_capacity(height);

    for index in view.offset..(view.offset + height).min(total) {
        let Some(display) = view.display_row(index) else {
            break;
        };
        match display {
            DisplayRow::Gap { count, .. } => {
                let text = format!("⋯ {count} 行省略 (Enter で展開)");
                left.push(Line::styled(text.clone(), theme.dim));
                if side_by_side {
                    right.push(Line::styled(text, theme.dim));
                }
            }
            DisplayRow::Row(row) => {
                let spans = view.inline_spans(row, inline_enabled);
                let pair = view.diff.rows[row as usize];
                if side_by_side {
                    left.push(side_line(
                        view,
                        pair,
                        Side::Old,
                        &spans,
                        old_width,
                        left_width,
                        hscroll,
                        opts,
                        theme,
                    ));
                    right.push(side_line(
                        view,
                        pair,
                        Side::New,
                        &spans,
                        new_width,
                        right_width,
                        hscroll,
                        opts,
                        theme,
                    ));
                } else {
                    unified_lines(
                        view, pair, &spans, old_width, new_width, left_width, hscroll, opts, theme,
                        &mut left,
                    );
                }
            }
        }
    }
    Some((left, right))
}

#[expect(clippy::too_many_arguments)]
fn side_line<'a>(
    view: &DiffView,
    pair: RowPair,
    side: Side,
    spans: &InlineSpans,
    num_width: usize,
    width: u16,
    hscroll: usize,
    opts: TextOpts,
    theme: &Theme,
) -> Line<'a> {
    let (cell, table, highlights, marker, bg, inline_bg) = match side {
        Side::Old => (
            pair.left,
            &view.diff.old,
            &spans.old,
            match pair.kind {
                RowKind::Removed | RowKind::Changed => '-',
                _ => ' ',
            },
            theme.removed_bg,
            theme.removed_inline_bg,
        ),
        Side::New => (
            pair.right,
            &view.diff.new,
            &spans.new,
            match pair.kind {
                RowKind::Added | RowKind::Changed => '+',
                _ => ' ',
            },
            theme.added_bg,
            theme.added_inline_bg,
        ),
    };

    let width = width as usize;
    let content_width = width.saturating_sub(num_width + 2);
    match cell {
        Cell::Pad => Line::from(vec![Span::styled(
            " ".repeat(width),
            Style::new().bg(theme.pad_bg),
        )]),
        Cell::Line(index) => {
            let row_bg = if pair.kind == RowKind::Context {
                None
            } else {
                Some(bg)
            };
            let base = row_bg.map_or(Style::new(), |c| Style::new().bg(c));
            let mut out = vec![
                Span::styled(
                    format!("{:>num_width$}", index + 1),
                    theme.gutter.patch(base),
                ),
                Span::styled(marker.to_string(), base),
                Span::raw(" "),
            ];
            out.extend(content_spans(
                table,
                index,
                highlights,
                opts,
                hscroll,
                content_width,
                base,
                Style::new().bg(inline_bg),
                theme,
            ));
            Line::from(out)
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn unified_lines<'a>(
    view: &DiffView,
    pair: RowPair,
    spans: &InlineSpans,
    old_width: usize,
    new_width: usize,
    width: u16,
    hscroll: usize,
    opts: TextOpts,
    theme: &Theme,
    out: &mut Vec<Line<'a>>,
) {
    let num_width = old_width.max(new_width);
    match pair.kind {
        RowKind::Context => out.push(side_line(
            view,
            pair,
            Side::Old,
            spans,
            num_width,
            width,
            hscroll,
            opts,
            theme,
        )),
        RowKind::Removed => out.push(side_line(
            view,
            pair,
            Side::Old,
            spans,
            num_width,
            width,
            hscroll,
            opts,
            theme,
        )),
        RowKind::Added => out.push(side_line(
            view,
            pair,
            Side::New,
            spans,
            num_width,
            width,
            hscroll,
            opts,
            theme,
        )),
        RowKind::Changed => {
            out.push(side_line(
                view,
                pair,
                Side::Old,
                spans,
                num_width,
                width,
                hscroll,
                opts,
                theme,
            ));
            out.push(side_line(
                view,
                pair,
                Side::New,
                spans,
                num_width,
                width,
                hscroll,
                opts,
                theme,
            ));
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn content_spans<'a>(
    table: &LineTable,
    index: u32,
    highlights: &[std::ops::Range<usize>],
    opts: TextOpts,
    hscroll: usize,
    content_width: usize,
    base: Style,
    inline: Style,
    theme: &Theme,
) -> Vec<Span<'a>> {
    let raw = table.line_display(index);
    let text = String::from_utf8_lossy(raw);
    let mut out: Vec<Span> = render_line(&text, highlights, opts, hscroll, content_width)
        .into_iter()
        .map(|(t, hi)| {
            let style = if hi { base.patch(inline) } else { base };
            Span::styled(t, style)
        })
        .collect();

    let mut suffix = String::new();
    if table.has_cr(index) {
        suffix.push_str(CR_MARK);
    }
    if table.is_last_without_newline(index) {
        suffix.push_str(NO_NEWLINE_MARK);
    }
    if !suffix.is_empty() {
        out.push(Span::styled(suffix, theme.dim.patch(base)));
    }
    if base.bg.is_some_and(|c| c != Color::Reset) {
        // 行全体を着色し、変更行の範囲を目で追えるようにする
        let used: usize = out
            .iter()
            .map(|s| super::text::display_width(&s.content, opts))
            .sum();
        if used < content_width {
            out.push(Span::styled(" ".repeat(content_width - used), base));
        }
    }
    out
}
