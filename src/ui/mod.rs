pub mod content_pane;
pub mod diff_pane;
pub mod overlay;
pub mod status_bar;
pub mod text;
pub mod theme;
pub mod tree_pane;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, ContentState, Focus, Mode};
use theme::Theme;

/// この幅を下回ると左ペインを隠し、差分を unified 表示へ落とす。
pub const NARROW_WIDTH: u16 = 80;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let theme = Theme::detect();
    let area = frame.area();
    if area.height < 3 || area.width < 20 {
        frame.render_widget(Paragraph::new("端末が小さすぎる"), area);
        return;
    }

    let show_status = app.cfg.show_status_bar && area.height >= 5;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(u16::from(show_status)),
    ])
    .split(area);
    let (header, rule, body, status) = (rows[0], rows[1], rows[2], rows[3]);

    let narrow = area.width < NARROW_WIDTH;
    let show_tree = !narrow || app.focus == Focus::Tree;
    let constraints = if show_tree {
        vec![
            Constraint::Fill(app.tree_ratio),
            Constraint::Fill(10 - app.tree_ratio),
        ]
    } else {
        vec![Constraint::Fill(1)]
    };
    let header_cols = Layout::horizontal(constraints.clone()).split(header);
    let body_cols = Layout::horizontal(constraints).split(body);
    let content_area = *body_cols.last().expect("列は 1 つ以上");

    draw_rule(
        frame,
        rule,
        if show_tree { header_cols[0].width } else { 0 },
    );
    status_bar::draw_header(frame, header_cols.as_ref(), app, &theme, show_tree);

    if show_tree {
        let block = Block::new().borders(Borders::RIGHT).border_style(theme.dim);
        let inner = block.inner(body_cols[0]);
        frame.render_widget(block, body_cols[0]);
        app.tree_height = inner.height as usize;
        tree_pane::draw(frame, inner, app, &theme);
    } else {
        app.tree_height = 0;
    }

    draw_content(frame, content_area, app, &theme, narrow);

    if show_status {
        status_bar::draw(frame, status, app, &theme);
    }
    overlay::draw(frame, area, app, &theme);
}

fn draw_content(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme, narrow: bool) {
    app.content_height = area.height as usize;
    match &app.content {
        ContentState::Diff(_) if app.mode == Mode::Diff && !narrow => {
            diff_pane::draw_side_by_side(frame, area, app, theme);
        }
        ContentState::Diff(_) => diff_pane::draw_unified(frame, area, app, theme),
        _ => content_pane::draw(frame, area, app, theme),
    }
}

fn draw_rule(frame: &mut Frame, area: Rect, tree_width: u16) {
    let width = area.width as usize;
    let mut spans: Vec<Span> = Vec::new();
    if tree_width > 0 && (tree_width as usize) < width {
        let left = (tree_width as usize).saturating_sub(1);
        spans.push(Span::raw("─".repeat(left)));
        spans.push(Span::raw("┼"));
        spans.push(Span::raw("─".repeat(width - left - 1)));
    } else {
        spans.push(Span::raw("─".repeat(width)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(Style::new()), area);
}
