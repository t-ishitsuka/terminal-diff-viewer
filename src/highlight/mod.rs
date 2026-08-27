use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::diff::LineTable;
use crate::ui::text::Rgb;

/// 行内のバイト範囲と前景色。
pub type LineColors = Vec<(Range<usize>, Rgb)>;

#[derive(Debug, Default)]
pub struct Highlighted {
    lines: Vec<LineColors>,
}

impl Highlighted {
    pub fn line(&self, index: u32) -> &[(Range<usize>, Rgb)] {
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
    ASSETS.get_or_init(|| Assets {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        themes: ThemeSet::load_defaults(),
    })
}

pub const DEFAULT_THEME: &str = "base16-ocean.dark";

pub fn theme_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = assets().themes.themes.keys().map(String::as_str).collect();
    names.sort_unstable();
    names
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
/// 可視範囲だけを遅延処理する構成にはパース状態のスナップショットが要るため、
/// v1 では「上限行数までは一括で色付けし、超えるものは色付けしない」方針を採る。
/// 計算はワーカー側で行うので、結果が届くまでは素のテキストが表示される。
pub fn highlight(
    table: &LineTable,
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
        let color = Rgb::new(style.foreground.r, style.foreground.g, style.foreground.b);
        match out.last_mut() {
            Some((range, last)) if *last == color && range.end == start => range.end = end,
            _ => out.push((start..end, color)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;

    fn table(s: &str) -> LineTable {
        LineTable::new(StdArc::from(s.as_bytes()))
    }

    #[test]
    fn rust_source_gets_colors() {
        let t = table("fn main() {\n    let x = 1;\n}\n");
        let h = highlight(&t, Path::new("main.rs"), DEFAULT_THEME, 1000).expect("色付けされる");
        assert!(!h.line(0).is_empty(), "1 行目に色が付く");
        // キーワードと識別子で色が変わる
        let colors: Vec<Rgb> = h.line(0).iter().map(|(_, c)| *c).collect();
        assert!(colors.len() > 1, "1 行が単色になっている: {colors:?}");
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
    fn default_theme_exists() {
        assert!(theme_names().contains(&DEFAULT_THEME));
    }
}
