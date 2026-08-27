use ratatui::style::{Color, Modifier, Style};

use crate::git::ChangeKind;
use crate::ui::text::Rgb;
use crate::vfs::EntryKind;

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

    /// ツリーのエントリ色。ディレクトリ・リンク・実行ファイルを区別し、
    /// 通常ファイルは拡張子の分類で色を変える。
    pub fn entry_style(&self, kind: EntryKind, name: &str) -> Style {
        let (rgb, modifier) = match kind {
            EntryKind::Dir => (entry_color::DIR, Modifier::BOLD),
            EntryKind::Symlink => (entry_color::SYMLINK, Modifier::ITALIC),
            EntryKind::Executable => (entry_color::EXECUTABLE, Modifier::BOLD),
            EntryKind::File => (file_color(name), Modifier::empty()),
        };
        Style::new()
            .fg(self.syntax_color(rgb))
            .add_modifier(modifier)
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

/// ツリーのエントリ色。One Dark 系の色相をそのまま流用する。
mod entry_color {
    use super::Rgb;

    pub const DIR: Rgb = Rgb::new(0x61, 0xaf, 0xef);
    pub const SYMLINK: Rgb = Rgb::new(0x56, 0xb6, 0xc2);
    pub const EXECUTABLE: Rgb = Rgb::new(0x98, 0xc3, 0x79);
    pub const SOURCE: Rgb = Rgb::new(0xd1, 0x9a, 0x66);
    pub const CONFIG: Rgb = Rgb::new(0xe5, 0xc0, 0x7b);
    pub const DOC: Rgb = Rgb::new(0xc6, 0x78, 0xdd);
    pub const MEDIA: Rgb = Rgb::new(0xe0, 0x6c, 0x75);
    pub const ARCHIVE: Rgb = Rgb::new(0xbe, 0x50, 0x46);
    pub const PLAIN: Rgb = Rgb::new(0xab, 0xb2, 0xbf);
}

const SOURCE_EXT: &[&str] = &[
    "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "c", "h", "cc", "cpp", "hpp", "cs",
    "java", "kt", "kts", "rb", "php", "swift", "scala", "hs", "ml", "ex", "exs", "erl", "lua",
    "nix", "zig", "dart", "vim", "el", "clj", "sql", "sh", "bash", "zsh", "fish", "ps1",
];
const CONFIG_EXT: &[&str] = &[
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "ini",
    "conf",
    "cfg",
    "properties",
    "lock",
    "env",
    "editorconfig",
    "gitignore",
    "gitattributes",
    "csv",
    "tsv",
    "xml",
    "plist",
];
const DOC_EXT: &[&str] = &[
    "md", "markdown", "txt", "rst", "adoc", "org", "tex", "pdf", "html", "htm", "css", "scss",
    "sass", "less",
];
const MEDIA_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "mp3", "wav", "flac", "mp4", "mov",
    "webm", "ttf", "otf", "woff", "woff2",
];
const ARCHIVE_EXT: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "jar", "deb", "rpm",
];
/// 拡張子を持たないが分類できるファイル名。
const CONFIG_NAMES: &[&str] = &[
    "makefile",
    "justfile",
    "dockerfile",
    "containerfile",
    "procfile",
    "cargo.lock",
    "flake.lock",
];
const DOC_NAMES: &[&str] = &[
    "readme",
    "license",
    "licence",
    "changelog",
    "authors",
    "notice",
];

fn file_color(name: &str) -> Rgb {
    let lower = name.to_lowercase();
    if CONFIG_NAMES.contains(&lower.as_str()) {
        return entry_color::CONFIG;
    }
    if DOC_NAMES.contains(&lower.as_str()) {
        return entry_color::DOC;
    }
    let Some((_, ext)) = lower.rsplit_once('.') else {
        return entry_color::PLAIN;
    };
    if SOURCE_EXT.contains(&ext) {
        entry_color::SOURCE
    } else if CONFIG_EXT.contains(&ext) {
        entry_color::CONFIG
    } else if DOC_EXT.contains(&ext) {
        entry_color::DOC
    } else if MEDIA_EXT.contains(&ext) {
        entry_color::MEDIA
    } else if ARCHIVE_EXT.contains(&ext) {
        entry_color::ARCHIVE
    } else {
        entry_color::PLAIN
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
    fn file_colors_follow_the_extension_category() {
        assert_eq!(file_color("main.rs"), entry_color::SOURCE);
        assert_eq!(file_color("Cargo.toml"), entry_color::CONFIG);
        assert_eq!(file_color("README.md"), entry_color::DOC);
        assert_eq!(file_color("logo.png"), entry_color::MEDIA);
        assert_eq!(file_color("dump.tar.gz"), entry_color::ARCHIVE);
        assert_eq!(file_color("unknown"), entry_color::PLAIN);
    }

    #[test]
    fn extensionless_known_names_are_classified() {
        assert_eq!(file_color("Makefile"), entry_color::CONFIG);
        assert_eq!(file_color("LICENSE"), entry_color::DOC);
    }

    #[test]
    fn entry_kind_takes_precedence_over_extension() {
        let theme = Theme::new(Palette::RedGreen);
        let dir = theme.entry_style(EntryKind::Dir, "src.rs");
        let file = theme.entry_style(EntryKind::File, "src.rs");
        assert_ne!(dir.fg, file.fg);
        assert!(dir.add_modifier.contains(Modifier::BOLD));
        let link = theme.entry_style(EntryKind::Symlink, "src.rs");
        assert!(link.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn palette_parses_known_names() {
        assert_eq!(Palette::parse("red-green"), Some(Palette::RedGreen));
        assert_eq!(Palette::parse("blue-orange"), Some(Palette::BlueOrange));
        assert_eq!(Palette::parse("unknown"), None);
    }
}
