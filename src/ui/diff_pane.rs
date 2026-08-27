use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::render::spans;
use super::text::{Marks, TextOpts, render_line};
use super::theme::Theme;
use super::tree_pane::gutter_width;
use crate::app::{App, ContentState, DiffView, DisplayRow, SearchState};
use crate::diff::{Cell, InlineSpans, RowKind, RowPair};
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

    let Some((left, right)) = build_rows(app, theme, left_area.width, right_area.width, true)
    else {
        return;
    };
    frame.render_widget(Paragraph::new(left), left_area);
    frame.render_widget(Paragraph::new(right), right_area);
}

pub fn draw_unified(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let Some((lines, _)) = build_rows(app, theme, area.width, 0, false) else {
        return;
    };
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

    let search = app.search.clone();
    let ContentState::Diff(view) = &mut app.content else {
        return None;
    };
    let total = view.display_len();
    view.offset = view.offset.min(total.saturating_sub(height));

    let old_width = gutter_width(view.diff.old.len());
    let new_width = gutter_width(view.diff.new.len());
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
                let inline = view.inline_spans(row, inline_enabled);
                let pair = view.diff.rows[row as usize];
                let ctx = RowCtx {
                    view,
                    theme,
                    opts,
                    hscroll,
                    inline: &inline,
                    search: &search,
                    row,
                };
                if side_by_side {
                    left.push(side_line(&ctx, pair, Side::Old, old_width, left_width));
                    right.push(side_line(&ctx, pair, Side::New, new_width, right_width));
                } else {
                    unified_lines(&ctx, pair, old_width.max(new_width), left_width, &mut left);
                }
            }
        }
    }
    Some((left, right))
}

struct RowCtx<'a> {
    view: &'a DiffView,
    theme: &'a Theme,
    opts: TextOpts,
    hscroll: usize,
    inline: &'a InlineSpans,
    search: &'a SearchState,
    row: u32,
}

fn side_line<'a>(
    ctx: &RowCtx<'_>,
    pair: RowPair,
    side: Side,
    num_width: usize,
    width: u16,
) -> Line<'a> {
    let theme = ctx.theme;
    let right = side == Side::New;
    let (cell, table, highlight, inline_ranges, marker, bg, fg, inline_bg, gutter) = match side {
        Side::Old => (
            pair.left,
            &ctx.view.diff.old,
            ctx.view.old_highlight.as_ref(),
            &ctx.inline.old,
            match pair.kind {
                RowKind::Removed | RowKind::Changed => '-',
                _ => ' ',
            },
            theme.removed_bg,
            theme.removed_fg,
            theme.removed_inline_bg,
            theme.gutter_removed,
        ),
        Side::New => (
            pair.right,
            &ctx.view.diff.new,
            ctx.view.new_highlight.as_ref(),
            &ctx.inline.new,
            match pair.kind {
                RowKind::Added | RowKind::Changed => '+',
                _ => ' ',
            },
            theme.added_bg,
            theme.added_fg,
            theme.added_inline_bg,
            theme.gutter_added,
        ),
    };

    let width = width as usize;
    let content_width = width.saturating_sub(num_width + 2);
    let Cell::Line(index) = cell else {
        return Line::from(Span::styled(
            " ".repeat(width),
            Style::new().bg(theme.pad_bg),
        ));
    };

    let changed = pair.kind != RowKind::Context;
    let base = if changed {
        Style::new().bg(bg).fg(fg)
    } else {
        Style::new()
    };
    let gutter_style = if changed { gutter.bg(bg) } else { theme.gutter };

    let raw = table.line_display(index);
    let text = String::from_utf8_lossy(raw);
    let marks = Marks {
        inline: inline_ranges,
        search: ctx.search.ranges(ctx.row, right),
        colors: highlight.map_or(&[][..], |h| h.line(index)),
    };
    let segments = render_line(&text, marks, ctx.opts, ctx.hscroll, content_width);

    let mut out = vec![
        Span::styled(format!("{:>num_width$}", index + 1), gutter_style),
        Span::styled(marker.to_string(), gutter_style),
        Span::styled(" ", base),
    ];
    let used: usize = segments
        .iter()
        .map(|s| super::text::display_width(&s.text, ctx.opts))
        .sum();
    out.extend(spans(segments, base, inline_bg, theme));

    let mut suffix = String::new();
    if table.has_cr(index) {
        suffix.push_str(CR_MARK);
    }
    if table.is_last_without_newline(index) {
        suffix.push_str(NO_NEWLINE_MARK);
    }
    let suffix_width = suffix.chars().count();
    if !suffix.is_empty() {
        out.push(Span::styled(suffix, theme.dim.patch(base)));
    }
    // 変更行は行末まで着色し、範囲を目で追えるようにする
    if changed {
        let filled = used + suffix_width;
        if filled < content_width {
            out.push(Span::styled(" ".repeat(content_width - filled), base));
        }
    }
    Line::from(out)
}

fn unified_lines<'a>(
    ctx: &RowCtx<'_>,
    pair: RowPair,
    num_width: usize,
    width: u16,
    out: &mut Vec<Line<'a>>,
) {
    match pair.kind {
        RowKind::Context | RowKind::Removed => {
            out.push(side_line(ctx, pair, Side::Old, num_width, width));
        }
        RowKind::Added => out.push(side_line(ctx, pair, Side::New, num_width, width)),
        RowKind::Changed => {
            out.push(side_line(ctx, pair, Side::Old, num_width, width));
            out.push(side_line(ctx, pair, Side::New, num_width, width));
        }
    }
}
