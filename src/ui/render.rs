use ratatui::style::{Color, Style};
use ratatui::text::Span;

use super::text::Segment;
use super::theme::Theme;

/// セグメントの装飾をスタイルへ落とす。
/// 優先度は 検索一致 > 語単位差分 > シンタックスハイライト の順。
pub fn style_for(segment: &Segment, base: Style, inline_bg: Color, theme: &Theme) -> Style {
    let mut style = base;
    if let Some(rgb) = segment.color {
        style = style.fg(theme.syntax_color(rgb));
    }
    if segment.inline {
        style = style.bg(inline_bg);
    }
    if segment.search {
        style = style.bg(theme.search_bg).fg(theme.search_fg);
    }
    style
}

pub fn spans<'a>(
    segments: Vec<Segment>,
    base: Style,
    inline_bg: Color,
    theme: &Theme,
) -> Vec<Span<'a>> {
    segments
        .into_iter()
        .map(|segment| {
            let style = style_for(&segment, base, inline_bg, theme);
            Span::styled(segment.text, style)
        })
        .collect()
}
