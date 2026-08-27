use ratatui::style::{Color, Modifier, Style};

use crate::git::ChangeKind;
use crate::ui::text::Rgb;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Palette {
    #[default]
    RedGreen,
    /// 赤 / 緑の識別が難しい場合の代替。
    BlueOrange,
}

impl Palette {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "red-green" => Some(Palette::RedGreen),
            "blue-orange" => Some(Palette::BlueOrange),
            _ => None,
        }
    }
}

/// 差分の着色は「行背景 + マーカー + 前景色」の 3 点で表す。
/// 赤 / 緑だけに頼らないよう、マーカー文字 (`-` / `+`) は常に表示する。
#[derive(Clone, Debug)]
pub struct Theme {
    pub removed_bg: Color,
    pub added_bg: Color,
    pub removed_fg: Color,
    pub added_fg: Color,
    pub removed_inline_bg: Color,
    pub added_inline_bg: Color,
    pub pad_bg: Color,
    pub search_bg: Color,
    pub search_fg: Color,
    pub gutter: Style,
    pub gutter_removed: Style,
    pub gutter_added: Style,
    pub selection: Style,
    pub selection_blur: Style,
    pub header: Style,
    pub status: Style,
    pub notice: Style,
    pub error: Style,
    pub dim: Style,
    truecolor: bool,
}

impl Theme {
    pub fn new(palette: Palette) -> Self {
        let truecolor = matches!(
            std::env::var("COLORTERM").as_deref(),
            Ok("truecolor") | Ok("24bit")
        );
        let base = Self {
            truecolor,
            ..Self::skeleton()
        };
        match (palette, truecolor) {
            (Palette::RedGreen, true) => Self {
                removed_bg: Color::Rgb(74, 34, 38),
                added_bg: Color::Rgb(28, 66, 42),
                removed_fg: Color::Rgb(255, 190, 190),
                added_fg: Color::Rgb(185, 245, 195),
                removed_inline_bg: Color::Rgb(126, 48, 58),
                added_inline_bg: Color::Rgb(44, 108, 62),
                pad_bg: Color::Rgb(26, 26, 30),
                search_bg: Color::Rgb(180, 160, 40),
                search_fg: Color::Rgb(20, 20, 20),
                ..base
            },
            (Palette::RedGreen, false) => Self {
                removed_bg: Color::Indexed(52),
                added_bg: Color::Indexed(22),
                removed_fg: Color::Indexed(217),
                added_fg: Color::Indexed(157),
                removed_inline_bg: Color::Indexed(88),
                added_inline_bg: Color::Indexed(28),
                pad_bg: Color::Indexed(235),
                search_bg: Color::Indexed(178),
                search_fg: Color::Indexed(16),
                ..base
            },
            (Palette::BlueOrange, true) => Self {
                removed_bg: Color::Rgb(74, 48, 20),
                added_bg: Color::Rgb(22, 48, 78),
                removed_fg: Color::Rgb(255, 214, 160),
                added_fg: Color::Rgb(175, 214, 255),
                removed_inline_bg: Color::Rgb(128, 82, 26),
                added_inline_bg: Color::Rgb(32, 82, 132),
                pad_bg: Color::Rgb(26, 26, 30),
                search_bg: Color::Rgb(180, 160, 40),
                search_fg: Color::Rgb(20, 20, 20),
                ..base
            },
            (Palette::BlueOrange, false) => Self {
                removed_bg: Color::Indexed(58),
                added_bg: Color::Indexed(24),
                removed_fg: Color::Indexed(223),
                added_fg: Color::Indexed(153),
                removed_inline_bg: Color::Indexed(94),
                added_inline_bg: Color::Indexed(26),
                pad_bg: Color::Indexed(235),
                search_bg: Color::Indexed(178),
                search_fg: Color::Indexed(16),
                ..base
            },
        }
    }

    fn skeleton() -> Self {
        Self {
            removed_bg: Color::Reset,
            added_bg: Color::Reset,
            removed_fg: Color::Reset,
            added_fg: Color::Reset,
            removed_inline_bg: Color::Reset,
            added_inline_bg: Color::Reset,
            pad_bg: Color::Reset,
            search_bg: Color::Reset,
            search_fg: Color::Reset,
            gutter: Style::new().fg(Color::DarkGray),
            gutter_removed: Style::new().fg(Color::Red),
            gutter_added: Style::new().fg(Color::Green),
            selection: Style::new().add_modifier(Modifier::REVERSED),
            selection_blur: Style::new().add_modifier(Modifier::BOLD),
            header: Style::new().add_modifier(Modifier::BOLD),
            status: Style::new().fg(Color::DarkGray),
            notice: Style::new().fg(Color::Yellow),
            error: Style::new().fg(Color::Red),
            dim: Style::new().fg(Color::DarkGray),
            truecolor: false,
        }
    }

    /// シンタックスハイライトの色を端末の表現へ落とす。
    pub fn syntax_color(&self, rgb: Rgb) -> Color {
        if self.truecolor {
            Color::Rgb(rgb.r, rgb.g, rgb.b)
        } else {
            Color::Indexed(to_ansi256(rgb))
        }
    }

    pub fn change_style(&self, kind: ChangeKind) -> Style {
        let color = match kind {
            ChangeKind::Added => Color::Green,
            ChangeKind::Modified => Color::Yellow,
            ChangeKind::Deleted => Color::Red,
            ChangeKind::Renamed => Color::Cyan,
            ChangeKind::Untracked => Color::Blue,
        };
        Style::new().fg(color)
    }
}

/// 24bit 非対応の端末向けに、6x6x6 カラーキューブとグレースケールへ近似する。
fn to_ansi256(rgb: Rgb) -> u8 {
    let (r, g, b) = (rgb.r, rgb.g, rgb.b);
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return 232 + ((u16::from(r) - 8) * 24 / 247) as u8;
    }
    let level = |v: u8| -> u16 { (u16::from(v) * 5 + 127) / 255 };
    (16 + 36 * level(r) + 6 * level(g) + level(b)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi256_maps_extremes() {
        assert_eq!(to_ansi256(Rgb::new(0, 0, 0)), 16);
        assert_eq!(to_ansi256(Rgb::new(255, 255, 255)), 231);
        assert_eq!(to_ansi256(Rgb::new(255, 0, 0)), 196);
        assert_eq!(to_ansi256(Rgb::new(0, 255, 0)), 46);
    }

    #[test]
    fn palette_parses_known_names() {
        assert_eq!(Palette::parse("red-green"), Some(Palette::RedGreen));
        assert_eq!(Palette::parse("blue-orange"), Some(Palette::BlueOrange));
        assert_eq!(Palette::parse("unknown"), None);
    }
}
