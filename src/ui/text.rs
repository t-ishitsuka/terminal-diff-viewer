use std::ops::Range;

use unicode_width::UnicodeWidthChar;

/// 全角文字を含む行の桁揃えはここに集約する。
#[derive(Copy, Clone, Debug)]
pub struct TextOpts {
    pub tab_width: usize,
    /// East Asian Ambiguous 文字を全角として扱うか。端末設定に依存するため切り替え可能にする。
    pub ambiguous_wide: bool,
}

impl Default for TextOpts {
    fn default() -> Self {
        Self {
            tab_width: 4,
            ambiguous_wide: false,
        }
    }
}

impl TextOpts {
    fn char_width(&self, c: char, col: usize) -> usize {
        if c == '\t' {
            let w = self.tab_width.max(1);
            return w - (col % w);
        }
        if is_control(c) {
            return 1;
        }
        let w = if self.ambiguous_wide {
            c.width_cjk()
        } else {
            c.width()
        };
        w.unwrap_or(1).max(1)
    }
}

fn is_control(c: char) -> bool {
    (c as u32) < 0x20 || c as u32 == 0x7f
}

/// 端末非依存の色表現。ratatui / syntect の型を ui/text.rs へ持ち込まないために使う。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// 1 行に重ねる装飾。いずれもバイト範囲で、開始位置の昇順に並んでいること。
#[derive(Copy, Clone, Default)]
pub struct Marks<'a> {
    /// 語単位差分の強調範囲。
    pub inline: &'a [Range<usize>],
    /// 検索一致の範囲。
    pub search: &'a [Range<usize>],
    /// シンタックスハイライトの装飾。
    pub colors: &'a [(Range<usize>, SyntaxStyle)],
}

/// シンタックスハイライト 1 区間分の装飾。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SyntaxStyle {
    pub color: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub inline: bool,
    pub search: bool,
    pub syntax: SyntaxStyle,
}

#[derive(Copy, Clone, PartialEq, Eq)]
struct Attrs {
    inline: bool,
    search: bool,
    syntax: SyntaxStyle,
}

pub fn display_width(s: &str, opts: TextOpts) -> usize {
    let mut col = 0;
    for c in s.chars() {
        col += opts.char_width(c, col);
    }
    col
}

/// 範囲の並びに対して、走査位置を進めながら該当判定を行う補助。
struct RangeCursor<'a, T> {
    items: &'a [T],
    index: usize,
}

impl<'a, T> RangeCursor<'a, T> {
    fn new(items: &'a [T]) -> Self {
        Self { items, index: 0 }
    }

    fn find(&mut self, byte: usize, range_of: impl Fn(&T) -> Range<usize>) -> Option<&'a T> {
        while self.index < self.items.len() && range_of(&self.items[self.index]).end <= byte {
            self.index += 1;
        }
        let item = self.items.get(self.index)?;
        let range = range_of(item);
        (range.start <= byte && byte < range.end).then_some(item)
    }
}

/// 1 行を表示セグメント列へ変換する。
/// タブ展開・制御文字の可視化・水平スクロール・装飾の適用をまとめて行う。
/// 戻り値の合計表示幅は `max_cols` 以下。
pub fn render_line(
    line: &str,
    marks: Marks<'_>,
    opts: TextOpts,
    skip_cols: usize,
    max_cols: usize,
) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    if max_cols == 0 {
        return out;
    }
    let end_col = skip_cols + max_cols;
    let mut col = 0usize;
    let mut inline = RangeCursor::new(marks.inline);
    let mut search = RangeCursor::new(marks.search);
    let mut colors = RangeCursor::new(marks.colors);

    let push = |out: &mut Vec<Segment>, text: &str, attrs: Attrs| {
        let matches_last = out.last().is_some_and(|s| {
            s.inline == attrs.inline && s.search == attrs.search && s.syntax == attrs.syntax
        });
        if matches_last {
            out.last_mut()
                .expect("直前のセグメント")
                .text
                .push_str(text);
        } else {
            out.push(Segment {
                text: text.to_string(),
                inline: attrs.inline,
                search: attrs.search,
                syntax: attrs.syntax,
            });
        }
    };

    for (byte, c) in line.char_indices() {
        if col >= end_col {
            break;
        }
        let w = opts.char_width(c, col);
        let next = col + w;
        if next <= skip_cols {
            col = next;
            continue;
        }

        let attrs = Attrs {
            inline: inline.find(byte, Clone::clone).is_some(),
            search: search.find(byte, Clone::clone).is_some(),
            syntax: colors
                .find(byte, |(r, _)| r.clone())
                .map(|(_, style)| *style)
                .unwrap_or_default(),
        };

        // 境界にまたがる文字は空白で埋め、桁が崩れないようにする
        let visible_start = col.max(skip_cols);
        let visible_end = next.min(end_col);
        let visible = visible_end - visible_start;
        if c == '\t' || col < skip_cols || next > end_col {
            push(&mut out, &" ".repeat(visible), attrs);
        } else if is_control(c) {
            push(&mut out, "·", attrs);
        } else {
            push(&mut out, &c.to_string(), attrs);
        }
        col = next;
    }
    out
}

/// 1 行を幅 `width` ごとに折り返し、画面行ごとのセグメント列を返す。
/// 全角文字が境界をまたいで欠けないよう、桁ではなく文字の切れ目で分割する。
pub fn wrap_line(line: &str, marks: Marks<'_>, opts: TextOpts, width: usize) -> Vec<Vec<Segment>> {
    if width == 0 {
        return Vec::new();
    }
    let mut starts = vec![0usize];
    let mut col = 0usize;
    for c in line.chars() {
        let w = opts.char_width(c, col);
        let start = *starts.last().expect("先頭は必ずある");
        // 幅そのものより広い文字は分割できないため、行頭では折り返さない
        if col > start && col + w > start + width {
            starts.push(col);
        }
        col += w;
    }
    starts
        .into_iter()
        .map(|start| render_line(line, marks, opts, start, width))
        .collect()
}

/// 表示幅が `width` を超える場合に末尾を省略記号へ置き換える。
pub fn fit_width(s: &str, width: usize, opts: TextOpts) -> String {
    if width == 0 {
        return String::new();
    }
    if display_width(s, opts) <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut col = 0usize;
    for c in s.chars() {
        let w = opts.char_width(c, col);
        if col + w > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        col += w;
    }
    out.push('…');
    out
}

/// 表示幅が `width` になるよう右側を空白で埋める。
pub fn pad_to(s: &str, width: usize, opts: TextOpts) -> String {
    let w = display_width(s, opts);
    if w >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + (width - w));
    out.push_str(s);
    out.push_str(&" ".repeat(width - w));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> TextOpts {
        TextOpts::default()
    }

    fn joined(segs: &[Segment]) -> String {
        segs.iter().map(|s| s.text.as_str()).collect()
    }

    fn plain(line: &str, skip: usize, width: usize) -> Vec<Segment> {
        render_line(line, Marks::default(), opts(), skip, width)
    }

    #[test]
    fn full_width_chars_count_as_two_columns() {
        assert_eq!(display_width("日本語", opts()), 6);
        assert_eq!(display_width("abc", opts()), 3);
        assert_eq!(display_width("あa", opts()), 3);
    }

    #[test]
    fn tabs_expand_to_next_tab_stop() {
        assert_eq!(display_width("\t", opts()), 4);
        assert_eq!(display_width("ab\t", opts()), 4);
        assert_eq!(display_width("abcd\t", opts()), 8);
    }

    #[test]
    fn render_clips_to_the_window() {
        assert_eq!(joined(&plain("abcdefgh", 2, 3)), "cde");
    }

    #[test]
    fn render_pads_when_wide_char_straddles_boundary() {
        // 全角 1 文字が右端をまたぐ場合は空白 1 桁になる
        assert_eq!(joined(&plain("あい", 0, 3)), "あ ");
    }

    #[test]
    #[expect(clippy::single_range_in_vec_init)]
    fn render_marks_inline_ranges() {
        let inline = [8..9];
        let marks = Marks {
            inline: &inline,
            ..Marks::default()
        };
        let segs = render_line("let x = 1;", marks, opts(), 0, 20);
        let hit: Vec<&str> = segs
            .iter()
            .filter(|s| s.inline)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(hit, vec!["1"]);
    }

    #[test]
    fn render_applies_syntax_colors() {
        let red = SyntaxStyle {
            color: Some(Rgb::new(255, 0, 0)),
            bold: true,
            italic: false,
        };
        let colors = [(0..3, red)];
        let marks = Marks {
            colors: &colors,
            ..Marks::default()
        };
        let segs = render_line("let x", marks, opts(), 0, 20);
        assert_eq!(segs[0].text, "let");
        assert_eq!(segs[0].syntax, red);
        assert_eq!(segs[1].syntax, SyntaxStyle::default());
    }

    #[test]
    #[expect(clippy::single_range_in_vec_init)]
    fn render_separates_search_matches() {
        let search = [4..7];
        let marks = Marks {
            search: &search,
            ..Marks::default()
        };
        let segs = render_line("let foo = 1;", marks, opts(), 0, 20);
        let hit: Vec<&str> = segs
            .iter()
            .filter(|s| s.search)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(hit, vec!["foo"]);
    }

    #[test]
    fn render_visualizes_control_chars() {
        assert_eq!(joined(&plain("a\u{7}b", 0, 10)), "a·b");
    }

    #[test]
    fn fit_width_truncates_with_ellipsis() {
        assert_eq!(fit_width("abcdef", 4, opts()), "abc…");
        assert_eq!(fit_width("abc", 4, opts()), "abc");
        assert_eq!(fit_width("日本語", 4, opts()), "日…");
    }

    #[test]
    fn pad_to_uses_display_width() {
        assert_eq!(pad_to("あ", 4, opts()), "あ  ");
        assert_eq!(display_width(&pad_to("あ", 4, opts()), opts()), 4);
    }

    #[test]
    fn wrap_splits_a_line_into_screen_rows() {
        let rows = wrap_line("abcdefg", Marks::default(), opts(), 3);
        let texts: Vec<String> = rows.iter().map(|r| joined(r)).collect();
        assert_eq!(texts, vec!["abc", "def", "g"]);
    }

    #[test]
    fn wrap_never_drops_a_full_width_char_at_the_boundary() {
        let rows = wrap_line("あいう", Marks::default(), opts(), 3);
        let joined_all: String = rows.iter().map(|r| joined(r)).collect();
        for c in ['あ', 'い', 'う'] {
            assert_eq!(
                joined_all.matches(c).count(),
                1,
                "{c} が欠けたか重複した: {joined_all:?}"
            );
        }
    }

    #[test]
    fn wrap_of_a_short_line_is_a_single_row() {
        assert_eq!(wrap_line("abc", Marks::default(), opts(), 10).len(), 1);
        assert_eq!(wrap_line("", Marks::default(), opts(), 10).len(), 1);
    }
}
