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

pub fn display_width(s: &str, opts: TextOpts) -> usize {
    let mut col = 0;
    for c in s.chars() {
        col += opts.char_width(c, col);
    }
    col
}

/// 1 行を表示セグメント列へ変換する。
/// タブ展開・制御文字の可視化・水平スクロール・強調範囲の適用をまとめて行う。
/// 戻り値は (テキスト, 強調フラグ) の並びで、合計幅は `max_cols` 以下。
pub fn render_line(
    line: &str,
    highlights: &[Range<usize>],
    opts: TextOpts,
    skip_cols: usize,
    max_cols: usize,
) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    if max_cols == 0 {
        return out;
    }
    let end_col = skip_cols + max_cols;
    let mut col = 0usize;
    let mut hi_index = 0usize;

    let push = |out: &mut Vec<(String, bool)>, text: &str, hi: bool| match out.last_mut() {
        Some((buf, last_hi)) if *last_hi == hi => buf.push_str(text),
        _ => out.push((text.to_string(), hi)),
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

        while hi_index < highlights.len() && highlights[hi_index].end <= byte {
            hi_index += 1;
        }
        let hi = highlights
            .get(hi_index)
            .is_some_and(|r| r.start <= byte && byte < r.end);

        // 境界にまたがる文字は空白で埋め、桁が崩れないようにする
        let visible_start = col.max(skip_cols);
        let visible_end = next.min(end_col);
        let visible = visible_end - visible_start;
        if c == '\t' || col < skip_cols || next > end_col {
            push(&mut out, &" ".repeat(visible), hi);
        } else if is_control(c) {
            push(&mut out, "·", hi);
        } else {
            push(&mut out, &c.to_string(), hi);
        }
        col = next;
    }
    out
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

    fn joined(segs: &[(String, bool)]) -> String {
        segs.iter().map(|(t, _)| t.as_str()).collect()
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
        let segs = render_line("abcdefgh", &[], opts(), 2, 3);
        assert_eq!(joined(&segs), "cde");
    }

    #[test]
    fn render_pads_when_wide_char_straddles_boundary() {
        // 全角 1 文字が右端をまたぐ場合は空白 1 桁になる
        let segs = render_line("あい", &[], opts(), 0, 3);
        assert_eq!(joined(&segs), "あ ");
    }

    #[test]
    #[expect(clippy::single_range_in_vec_init)]
    fn render_marks_highlight_ranges() {
        let segs = render_line("let x = 1;", &[8..9], opts(), 0, 20);
        let hi: Vec<&str> = segs
            .iter()
            .filter(|(_, h)| *h)
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(hi, vec!["1"]);
    }

    #[test]
    fn render_visualizes_control_chars() {
        let segs = render_line("a\u{7}b", &[], opts(), 0, 10);
        assert_eq!(joined(&segs), "a·b");
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
}
