use ratatui::style::{Color, Modifier, Style};

use crate::git::ChangeKind;

/// 端末のカラーテーマに依存しないよう、背景は差分部分にのみ着色する。
#[derive(Clone, Debug)]
pub struct Theme {
    pub removed_bg: Color,
    pub added_bg: Color,
    pub removed_inline_bg: Color,
    pub added_inline_bg: Color,
    pub pad_bg: Color,
    pub gutter: Style,
    pub selection: Style,
    pub selection_blur: Style,
    pub header: Style,
    pub status: Style,
    pub notice: Style,
    pub dim: Style,
}

impl Theme {
    pub fn detect() -> Self {
        let truecolor = matches!(
            std::env::var("COLORTERM").as_deref(),
            Ok("truecolor") | Ok("24bit")
        );
        if truecolor {
            Self::truecolor()
        } else {
            Self::indexed()
        }
    }

    fn truecolor() -> Self {
        Self {
            removed_bg: Color::Rgb(60, 26, 30),
            added_bg: Color::Rgb(22, 52, 32),
            removed_inline_bg: Color::Rgb(112, 40, 48),
            added_inline_bg: Color::Rgb(36, 92, 52),
            pad_bg: Color::Rgb(28, 28, 28),
            ..Self::common()
        }
    }

    fn indexed() -> Self {
        Self {
            removed_bg: Color::Indexed(52),
            added_bg: Color::Indexed(22),
            removed_inline_bg: Color::Indexed(88),
            added_inline_bg: Color::Indexed(28),
            pad_bg: Color::Indexed(236),
            ..Self::common()
        }
    }

    fn common() -> Self {
        Self {
            removed_bg: Color::Reset,
            added_bg: Color::Reset,
            removed_inline_bg: Color::Reset,
            added_inline_bg: Color::Reset,
            pad_bg: Color::Reset,
            gutter: Style::new().fg(Color::DarkGray),
            selection: Style::new().add_modifier(Modifier::REVERSED),
            selection_blur: Style::new().add_modifier(Modifier::BOLD),
            header: Style::new().add_modifier(Modifier::BOLD),
            status: Style::new().fg(Color::DarkGray),
            notice: Style::new().fg(Color::Yellow),
            dim: Style::new().fg(Color::DarkGray),
        }
    }

    pub fn change_style(&self, kind: ChangeKind) -> Style {
        let color = match kind {
            ChangeKind::Added => Color::Green,
            ChangeKind::Modified => Color::Yellow,
            ChangeKind::Deleted => Color::Red,
            ChangeKind::Renamed => Color::Cyan,
            ChangeKind::Untracked => Color::DarkGray,
        };
        Style::new().fg(color)
    }
}
