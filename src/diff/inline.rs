use std::ops::Range;

use gix::diff::blob::{Algorithm, Diff, InternedInput};

/// 語単位差分を打ち切る上限。minified ファイル等での計算爆発を防ぐ。
const MAX_LINE_BYTES: usize = 2000;
const MAX_TOKENS: usize = 500;

#[derive(Clone, Debug, Default)]
pub struct InlineSpans {
    pub old: Vec<Range<usize>>,
    pub new: Vec<Range<usize>>,
}

/// CJK は空白で単語境界が決まらないため 1 文字を 1 トークンとして扱う。
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F
        | 0x3040..=0x30FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF
        | 0x20000..=0x2FA1F)
}

fn is_word(c: char) -> bool {
    !is_cjk(c) && (c.is_alphanumeric() || c == '_')
}

/// 行を「単語 / 空白の連続 / それ以外 1 文字」へ分割し、バイト範囲を返す。
pub fn tokenize(s: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let mut end = i + c.len_utf8();
        if is_word(c) {
            while let Some(&(j, n)) = it.peek() {
                if is_word(n) {
                    end = j + n.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
        } else if c.is_whitespace() {
            while let Some(&(j, n)) = it.peek() {
                if n.is_whitespace() {
                    end = j + n.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
        }
        out.push(i..end);
    }
    out
}

fn merge_adjacent(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// 2 行の差異をトークン単位で求め、強調すべきバイト範囲を返す。
pub fn inline_diff(old: &str, new: &str) -> InlineSpans {
    if old == new {
        return InlineSpans::default();
    }
    if old.len() > MAX_LINE_BYTES || new.len() > MAX_LINE_BYTES {
        return whole_line(old, new);
    }

    let old_tokens = tokenize(old);
    let new_tokens = tokenize(new);
    if old_tokens.len() > MAX_TOKENS || new_tokens.len() > MAX_TOKENS {
        return whole_line(old, new);
    }

    let mut input: InternedInput<&str> = InternedInput::default();
    input.update_before(old_tokens.iter().map(|r| &old[r.clone()]));
    input.update_after(new_tokens.iter().map(|r| &new[r.clone()]));
    let diff = Diff::compute(Algorithm::Histogram, &input);

    let removed = (0..old_tokens.len())
        .filter(|i| diff.is_removed(*i as u32))
        .map(|i| old_tokens[i].clone())
        .collect();
    let added = (0..new_tokens.len())
        .filter(|i| diff.is_added(*i as u32))
        .map(|i| new_tokens[i].clone())
        .collect();

    InlineSpans {
        old: merge_adjacent(removed),
        new: merge_adjacent(added),
    }
}

// 意図通り「行全体を覆う 1 つの範囲」を持つ Vec を作る
#[expect(clippy::single_range_in_vec_init)]
fn whole_line(old: &str, new: &str) -> InlineSpans {
    InlineSpans {
        old: vec![0..old.len()],
        new: vec![0..new.len()],
    }
}

/// 文字バイグラムの Dice 係数。整列時のペアリング判定に使う軽量な類似度。
pub fn similarity(a: &[u8], b: &[u8]) -> f32 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let cap = 1024;
    let a = &a[..a.len().min(cap)];
    let b = &b[..b.len().min(cap)];
    // 前後に番兵を置く。1 文字の行でもバイグラムが得られる
    let bigrams = |s: &[u8]| -> Vec<u16> {
        let padded: Vec<u8> = std::iter::once(0)
            .chain(s.iter().copied())
            .chain(std::iter::once(0))
            .collect();
        let mut v: Vec<u16> = padded
            .windows(2)
            .map(|w| u16::from(w[0]) << 8 | u16::from(w[1]))
            .collect();
        v.sort_unstable();
        v
    };
    let (x, y) = (bigrams(a), bigrams(b));

    let (mut i, mut j, mut common) = (0usize, 0usize, 0usize);
    while i < x.len() && j < y.len() {
        match x[i].cmp(&y[j]) {
            std::cmp::Ordering::Equal => {
                common += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    2.0 * common as f32 / (x.len() + y.len()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_words_whitespace_and_symbols() {
        let t = tokenize("let x = 1;");
        let got: Vec<&str> = t.iter().map(|r| &"let x = 1;"[r.clone()]).collect();
        assert_eq!(got, vec!["let", " ", "x", " ", "=", " ", "1", ";"]);
    }

    #[test]
    fn tokenize_splits_cjk_per_char() {
        let s = "日本語コメント";
        let t = tokenize(s);
        assert_eq!(t.len(), s.chars().count());
    }

    #[test]
    fn inline_diff_marks_only_changed_token() {
        let spans = inline_diff("let x = 1;", "let x = 2;");
        assert_eq!(spans.old.len(), 1);
        assert_eq!(&"let x = 1;"[spans.old[0].clone()], "1");
        assert_eq!(&"let x = 2;"[spans.new[0].clone()], "2");
    }

    #[test]
    fn inline_diff_on_identical_lines_is_empty() {
        let spans = inline_diff("same", "same");
        assert!(spans.old.is_empty() && spans.new.is_empty());
    }

    #[test]
    fn similarity_bounds() {
        assert_eq!(similarity(b"abc", b"abc"), 1.0);
        assert_eq!(similarity(b"abc", b""), 0.0);
        assert!(similarity(b"let x = 1;", b"let x = 2;") > 0.5);
        assert!(similarity(b"fn main() {}", b"struct Foo;") < 0.35);
        // 番兵により 1 文字の行でも接頭辞の一致が類似度に反映される
        assert!(similarity(b"b", b"b1") >= 0.35);
        assert!(similarity(b"5", b"X") < 0.35);
    }
}
