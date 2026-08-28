use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::render::spans;
use super::text::{Marks, Segment, TextOpts, render_line, wrap_line};
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
    let wrap = app.wrap;
    let height = app.content_height;
    if height == 0 {
        return None;
    }

    let search = app.search.clone();
    let ContentState::Diff(view) = &mut app.content else {
        return None;
    };
    let total = view.display_len();
    // 折り返し中は 1 行が複数行を占めるため、末尾行まで送れるようにする
    let last = if wrap {
        total.saturating_sub(1)
    } else {
        total.saturating_sub(height)
    };
    view.offset = view.offset.min(last);

    let old_width = gutter_width(view.diff.old.len());
    let new_width = gutter_width(view.diff.new.len());
    let hscroll = view.hscroll;

    let mut left: Vec<Line> = Vec::with_capacity(height);
    let mut right: Vec<Line> = Vec::with_capacity(height);

    let mut index = view.offset;
    while index < total && left.len() < height {
        let Some(display) = view.display_row(index) else {
            break;
        };
        index += 1;
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
                    wrap,
                    inline: &inline,
                    search: &search,
                    row,
                };
                if side_by_side {
                    let mut old = side_lines(&ctx, pair, Side::Old, old_width, left_width);
                    let mut new = side_lines(&ctx, pair, Side::New, new_width, right_width);
                    // 折り返しで行数がずれてもペアの対応が崩れないよう揃える
                    let rows = old.lines.len().max(new.lines.len());
                    old.pad_to(rows, left_width);
                    new.pad_to(rows, right_width);
                    left.extend(old.lines);
                    right.extend(new.lines);
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
    wrap: bool,
    inline: &'a InlineSpans,
    search: &'a SearchState,
    row: u32,
}

/// 片側 1 行分の描画結果。折り返しで複数行になることがある。
struct SideRender<'a> {
    lines: Vec<Line<'a>>,
    /// 対向側に合わせて行数を足すときの余白スタイル。
    filler: Style,
}

impl<'a> SideRender<'a> {
    fn pad_to(&mut self, rows: usize, width: u16) {
        while self.lines.len() < rows {
            self.lines.push(Line::from(Span::styled(
                " ".repeat(width as usize),
                self.filler,
            )));
        }
    }
}

fn side_lines<'a>(
    ctx: &RowCtx<'_>,
    pair: RowPair,
    side: Side,
    num_width: usize,
    width: u16,
) -> SideRender<'a> {
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
        let pad = Style::new().bg(theme.pad_bg);
        return SideRender {
            lines: vec![Line::from(Span::styled(" ".repeat(width), pad))],
            filler: pad,
        };
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
    let chunks: Vec<Vec<Segment>> = if ctx.wrap {
        wrap_line(&text, marks, ctx.opts, content_width)
    } else {
        vec![render_line(
            &text,
            marks,
            ctx.opts,
            ctx.hscroll,
            content_width,
        )]
    };

    let mut suffix = String::new();
    if table.has_cr(index) {
        suffix.push_str(CR_MARK);
    }
    if table.is_last_without_newline(index) {
        suffix.push_str(NO_NEWLINE_MARK);
    }

    let last = chunks.len().saturating_sub(1);
    let mut lines = Vec::with_capacity(chunks.len());
    for (i, segments) in chunks.into_iter().enumerate() {
        // 折り返しの 2 行目以降は行番号とマーカーを空にして桁を保つ
        let number = if i == 0 {
            format!("{:>num_width$}", index + 1)
        } else {
            " ".repeat(num_width)
        };
        let mark = if i == 0 { marker } else { ' ' };
        let mut out = vec![
            Span::styled(number, gutter_style),
            Span::styled(mark.to_string(), gutter_style),
            Span::styled(" ", base),
        ];
        let used: usize = segments
            .iter()
            .map(|s| super::text::display_width(&s.text, ctx.opts))
            .sum();
        out.extend(spans(segments, base, inline_bg, theme));

        let mut suffix_width = 0;
        if i == last && !suffix.is_empty() {
            suffix_width = suffix.chars().count();
            out.push(Span::styled(suffix.clone(), theme.dim.patch(base)));
        }
        // 変更行は行末まで着色し、範囲を目で追えるようにする
        if changed {
            let filled = used + suffix_width;
            if filled < content_width {
                out.push(Span::styled(" ".repeat(content_width - filled), base));
            }
        }
        lines.push(Line::from(out));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" ".repeat(width), base)));
    }

    SideRender {
        lines,
        filler: base,
    }
}

fn unified_lines<'a>(
    ctx: &RowCtx<'_>,
    pair: RowPair,
    num_width: usize,
    width: u16,
    out: &mut Vec<Line<'a>>,
) {
    let mut push = |side| out.extend(side_lines(ctx, pair, side, num_width, width).lines);
    match pair.kind {
        RowKind::Context | RowKind::Removed => push(Side::Old),
        RowKind::Added => push(Side::New),
        RowKind::Changed => {
            push(Side::Old);
            push(Side::New);
        }
    }
}
