use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::ui::text::{Rgb, SyntaxStyle};

/// 行内のバイト範囲と装飾。
pub type LineColors = Vec<(Range<usize>, SyntaxStyle)>;

#[derive(Debug, Default)]
pub struct Highlighted {
    lines: Vec<LineColors>,
}

impl Highlighted {
    pub fn line(&self, index: u32) -> &[(Range<usize>, SyntaxStyle)] {
        self.lines
            .get(index as usize)
            .map_or(&[][..], Vec::as_slice)
    }
}

struct Assets {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
}

/// 構文定義とテーマはバイナリへ埋め込まれている。初回アクセス時に一度だけ展開する。
fn assets() -> &'static Assets {
    static ASSETS: OnceLock<Assets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let mut themes = ThemeSet::load_defaults();
        for theme in [tdv_dark(), tdv_light()] {
            let name = theme.name.clone().expect("テーマ名を設定している");
            themes.themes.insert(name, theme);
        }
        Assets {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes,
        }
    })
}

pub const DEFAULT_THEME: &str = "tdv-dark";

pub fn theme_names() -> Vec<String> {
    let mut names: Vec<String> = assets().themes.themes.keys().cloned().collect();
    names.sort_unstable();
    names
}

const fn rgb(value: u32) -> Color {
    Color {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
        a: 0xff,
    }
}

fn item(scope: &str, color: u32, font_style: FontStyle) -> ThemeItem {
    ThemeItem {
        scope: scope
            .parse::<ScopeSelectors>()
            .expect("スコープ選択子が不正"),
        style: StyleModifier {
            foreground: Some(rgb(color)),
            background: None,
            font_style: Some(font_style),
        },
    }
}

fn build(name: &str, foreground: u32, background: u32, rules: Vec<ThemeItem>) -> Theme {
    Theme {
        name: Some(name.to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(rgb(foreground)),
            background: Some(rgb(background)),
            ..ThemeSettings::default()
        },
        scopes: rules,
    }
}

/// 同梱の base16 系テーマは色を付けるスコープが少なく、大半が既定色のまま残る。
/// エディタ (Zed / VS Code の One Dark) に近い網羅度になるよう、
/// 主要スコープを明示的に色分けしたテーマを持つ。
fn tdv_dark() -> Theme {
    let none = FontStyle::empty();
    build(
        "tdv-dark",
        0xabb2bf,
        0x282c34,
        vec![
            item(
                "comment, punctuation.definition.comment",
                0x7f848e,
                FontStyle::ITALIC,
            ),
            item("string, string.quoted, meta.string", 0x98c379, none),
            item("punctuation.definition.string", 0x89b06a, none),
            item(
                "constant.character.escape, constant.other.placeholder",
                0x56b6c2,
                none,
            ),
            item(
                "constant.numeric, constant.language, constant.other",
                0xd19a66,
                none,
            ),
            item("support.constant", 0xd19a66, none),
            item("keyword, keyword.control, keyword.other", 0xc678dd, none),
            item("keyword.operator, punctuation.accessor", 0x56b6c2, none),
            item("storage, storage.type, storage.modifier", 0xc678dd, none),
            item(
                "entity.name.function, support.function, meta.function-call, variable.function",
                0x61afef,
                none,
            ),
            item("entity.name.macro, support.macro", 0x61afef, none),
            item(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait",
                0xe5c07b,
                none,
            ),
            item(
                "support.type, support.class, entity.other.inherited-class, entity.name.impl",
                0xe5c07b,
                none,
            ),
            // ジェネリック引数の型名は meta.generic しか付かないため、まとめて型色にする
            item("meta.generic", 0xe5c07b, none),
            item(
                "storage.type.numeric, storage.type.primitive",
                0xe5c07b,
                none,
            ),
            item(
                "entity.name.namespace, entity.name.module, meta.path",
                0xe5c07b,
                none,
            ),
            item(
                "entity.name.section, entity.name.label",
                0xe5c07b,
                FontStyle::BOLD,
            ),
            item(
                "meta.mapping.key, meta.object-literal.key, support.type.property-name",
                0xe06c75,
                none,
            ),
            item("entity.name.tag", 0xe06c75, none),
            item("entity.other.attribute-name", 0xd19a66, none),
            item(
                "variable, variable.other, meta.definition.variable",
                0xe06c75,
                none,
            ),
            item("variable.parameter", 0xd19a66, none),
            item("variable.language", 0xe06c75, FontStyle::ITALIC),
            item("meta.annotation, meta.attribute", 0xe5c07b, none),
            item("punctuation", 0x8b95a5, none),
            item(
                "punctuation.section, meta.brace, meta.group",
                0xabb2bf,
                none,
            ),
            item("markup.heading", 0x61afef, FontStyle::BOLD),
            item("markup.bold", 0xd19a66, FontStyle::BOLD),
            item("markup.italic", 0xc678dd, FontStyle::ITALIC),
            item(
                "markup.underline.link, string.other.link",
                0x56b6c2,
                FontStyle::UNDERLINE,
            ),
            item("markup.raw, markup.quote", 0x98c379, none),
            item("markup.inserted", 0x98c379, none),
            item("markup.deleted", 0xe06c75, none),
            item("markup.list", 0xe06c75, none),
            item("invalid, invalid.illegal", 0xff5370, FontStyle::BOLD),
        ],
    )
}

fn tdv_light() -> Theme {
    let none = FontStyle::empty();
    build(
        "tdv-light",
        0x383a42,
        0xfafafa,
        vec![
            item(
                "comment, punctuation.definition.comment",
                0xa0a1a7,
                FontStyle::ITALIC,
            ),
            item("string, string.quoted, meta.string", 0x50a14f, none),
            item(
                "constant.character.escape, constant.other.placeholder",
                0x0184bc,
                none,
            ),
            item(
                "constant.numeric, constant.language, constant.other",
                0x986801,
                none,
            ),
            item("support.constant", 0x986801, none),
            item("keyword, keyword.control, keyword.other", 0xa626a4, none),
            item("keyword.operator, punctuation.accessor", 0x0184bc, none),
            item("storage, storage.type, storage.modifier", 0xa626a4, none),
            item(
                "entity.name.function, support.function, meta.function-call, variable.function",
                0x4078f2,
                none,
            ),
            item(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait",
                0xc18401,
                none,
            ),
            item(
                "support.type, support.class, entity.other.inherited-class, entity.name.impl",
                0xc18401,
                none,
            ),
            item("meta.generic", 0xc18401, none),
            item(
                "storage.type.numeric, storage.type.primitive",
                0xc18401,
                none,
            ),
            item(
                "entity.name.namespace, entity.name.module, meta.path",
                0xc18401,
                none,
            ),
            item(
                "entity.name.section, entity.name.label",
                0xc18401,
                FontStyle::BOLD,
            ),
            item(
                "meta.mapping.key, meta.object-literal.key, support.type.property-name",
                0xe45649,
                none,
            ),
            item(
                "punctuation.section, meta.brace, meta.group",
                0x383a42,
                none,
            ),
            item("entity.name.tag", 0xe45649, none),
            item("entity.other.attribute-name", 0x986801, none),
            item(
                "variable, variable.other, meta.definition.variable",
                0xe45649,
                none,
            ),
            item("variable.parameter", 0x986801, none),
            item("variable.language", 0xe45649, FontStyle::ITALIC),
            item("meta.annotation, meta.attribute", 0xc18401, none),
            item("punctuation", 0x6a737d, none),
            item("markup.heading", 0x4078f2, FontStyle::BOLD),
            item("markup.bold", 0x986801, FontStyle::BOLD),
            item("markup.italic", 0xa626a4, FontStyle::ITALIC),
            item(
                "markup.underline.link, string.other.link",
                0x0184bc,
                FontStyle::UNDERLINE,
            ),
            item("invalid, invalid.illegal", 0xca1243, FontStyle::BOLD),
        ],
    )
}

fn find_syntax<'a>(
    assets: &'a Assets,
    path: &Path,
    first_line: Option<&str>,
) -> Option<&'a SyntaxReference> {
    let by_extension = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|e| assets.syntaxes.find_syntax_by_extension(e));
    if by_extension.is_some() {
        return by_extension;
    }
    // 拡張子が無いファイル (シェルスクリプト等) はファイル名と 1 行目から推測する
    let by_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| assets.syntaxes.find_syntax_by_extension(n));
    if by_name.is_some() {
        return by_name;
    }
    first_line.and_then(|line| assets.syntaxes.find_syntax_by_first_line(line))
}

/// ファイル全体をワーカースレッドで色付けする。
///
/// syntect のパーサは先頭行から順に状態を送る必要があり、途中行から再開できない。
/// 可視範囲だけを遅延処理してもパース総量は変わらないため、上限行数までは一括で
/// 色付けし、超えるものは色付けしない方針を採る。
/// 計算はワーカー側で行うので、結果が届くまでは素のテキストが表示される。
pub fn highlight(
    table: &crate::diff::LineTable,
    path: &Path,
    theme_name: &str,
    max_lines: usize,
) -> Option<Arc<Highlighted>> {
    if table.is_empty() || table.len() > max_lines {
        return None;
    }
    let assets = assets();
    let first_line = std::str::from_utf8(table.line_display(0)).ok();
    let syntax = find_syntax(assets, path, first_line)?;
    let theme = assets
        .themes
        .themes
        .get(theme_name)
        .or_else(|| assets.themes.themes.get(DEFAULT_THEME))?;

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::with_capacity(table.len());
    let mut buffer = String::new();
    for index in 0..table.len() as u32 {
        let Ok(text) = std::str::from_utf8(table.line_display(index)) else {
            // 不正な UTF-8 の行は色付けせず、パーサの状態だけ進める
            lines.push(Vec::new());
            continue;
        };
        buffer.clear();
        buffer.push_str(text);
        buffer.push('\n');
        let Ok(regions) = highlighter.highlight_line(&buffer, &assets.syntaxes) else {
            break;
        };
        lines.push(to_colors(&regions, text.len()));
    }
    lines.resize(table.len(), Vec::new());
    Some(Arc::new(Highlighted { lines }))
}

fn to_colors(regions: &[(syntect::highlighting::Style, &str)], line_len: usize) -> LineColors {
    let mut out: LineColors = Vec::new();
    let mut offset = 0usize;
    for (style, text) in regions {
        let start = offset;
        offset += text.len();
        let end = offset.min(line_len);
        if start >= end {
            continue;
        }
        let syntax = SyntaxStyle {
            color: Some(Rgb::new(
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
            )),
            bold: style.font_style.contains(FontStyle::BOLD),
            italic: style.font_style.contains(FontStyle::ITALIC),
        };
        match out.last_mut() {
            Some((range, last)) if *last == syntax && range.end == start => range.end = end,
            _ => out.push((start..end, syntax)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineTable;

    fn table(s: &str) -> LineTable {
        LineTable::new(Arc::from(s.as_bytes()))
    }

    fn colors(h: &Highlighted, line: u32) -> Vec<Rgb> {
        h.line(line)
            .iter()
            .filter_map(|(_, style)| style.color)
            .collect()
    }

    #[test]
    fn rust_source_gets_many_distinct_colors() {
        let source =
            "// コメント\nfn main() {\n    let count: usize = 1;\n    println!(\"hi\");\n}\n";
        let t = table(source);
        let h = highlight(&t, Path::new("main.rs"), DEFAULT_THEME, 1000).expect("色付けされる");
        let mut all: Vec<Rgb> = (0..t.len() as u32).flat_map(|i| colors(&h, i)).collect();
        all.sort_by_key(|c| (c.r, c.g, c.b));
        all.dedup();
        // キーワード / 型 / 文字列 / 数値 / コメントが別々の色になる
        assert!(all.len() >= 5, "色数が少なすぎる: {all:?}");
    }

    #[test]
    fn comments_are_italic() {
        let t = table("// メモ\nfn main() {}\n");
        let h = highlight(&t, Path::new("a.rs"), DEFAULT_THEME, 1000).unwrap();
        assert!(
            h.line(0).iter().any(|(_, s)| s.italic),
            "コメントが斜体にならない"
        );
    }

    #[test]
    fn ranges_stay_inside_the_line() {
        let t = table("fn main() {}\n");
        let h = highlight(&t, Path::new("main.rs"), DEFAULT_THEME, 1000).unwrap();
        let len = t.line_display(0).len();
        for (range, _) in h.line(0) {
            assert!(range.end <= len, "{range:?} が行長 {len} を超えている");
        }
    }

    #[test]
    fn unknown_extension_is_not_highlighted() {
        let t = table("なにか\n");
        assert!(highlight(&t, Path::new("a.unknown-ext"), DEFAULT_THEME, 1000).is_none());
    }

    #[test]
    fn oversized_file_is_skipped() {
        let t = table(&"x\n".repeat(50));
        assert!(highlight(&t, Path::new("a.rs"), DEFAULT_THEME, 10).is_none());
    }

    #[test]
    fn bundled_themes_are_available() {
        let names = theme_names();
        assert!(names.contains(&"tdv-dark".to_string()));
        assert!(names.contains(&"tdv-light".to_string()));
        assert!(names.contains(&"base16-ocean.dark".to_string()));
    }
}
