use std::ops::Range;

use gix::diff::blob::{Algorithm, Diff, InternedInput};

use super::inline::similarity;
use super::model::*;
use crate::git::LineStat;

/// 変更ブロック内で同じ位置の行を「同一行の変更」とみなす類似度の下限。
pub const PAIR_THRESHOLD: f32 = 0.35;

/// 旧 / 新の全行を 1 本の行ペア列へ整列する。
/// hunk 間の変更なし区間も打ち切らずに出力するため、結果は常にファイル全体を含む。
pub fn align(old: LineTable, new: LineTable) -> AlignedDiff {
    let mut rows: Vec<RowPair> = Vec::with_capacity(old.len().max(new.len()));
    let mut hunks: Vec<HunkAnchor> = Vec::new();
    let mut stat = LineStat::default();

    {
        let mut input: InternedInput<&[u8]> = InternedInput::default();
        input.update_before(old.iter_lines());
        input.update_after(new.iter_lines());
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);

        let mut old_cur = 0u32;
        let mut new_cur = 0u32;
        for hunk in diff.hunks() {
            push_context(&mut rows, &mut old_cur, &mut new_cur, hunk.before.start);
            let start = rows.len() as u32;
            let (before, after) = (hunk.before.clone(), hunk.after.clone());
            align_block(&old, &new, before, after, &mut rows, &mut stat);
            hunks.push(HunkAnchor {
                rows: start..rows.len() as u32,
            });
            old_cur = hunk.before.end;
            new_cur = hunk.after.end;
        }
        push_context(&mut rows, &mut old_cur, &mut new_cur, old.len() as u32);
    }

    AlignedDiff {
        rows,
        old,
        new,
        hunks,
        stat,
    }
}

fn push_context(rows: &mut Vec<RowPair>, old_cur: &mut u32, new_cur: &mut u32, until: u32) {
    while *old_cur < until {
        rows.push(RowPair {
            left: Cell::Line(*old_cur),
            right: Cell::Line(*new_cur),
            kind: RowKind::Context,
        });
        *old_cur += 1;
        *new_cur += 1;
    }
}

fn align_block(
    old: &LineTable,
    new: &LineTable,
    before: Range<u32>,
    after: Range<u32>,
    rows: &mut Vec<RowPair>,
    stat: &mut LineStat,
) {
    let removed = before.end - before.start;
    let added = after.end - after.start;

    if added == 0 {
        for i in before {
            rows.push(removed_row(i));
            stat.removed += 1;
        }
        return;
    }
    if removed == 0 {
        for i in after {
            rows.push(added_row(i));
            stat.added += 1;
        }
        return;
    }

    // 1 行が 1 行に置き換わった場合は、内容が似ていなくても同じ行の変更として扱う
    let force_pair = removed == 1 && added == 1;
    let paired = removed.min(added);
    for k in 0..paired {
        let (oi, ni) = (before.start + k, after.start + k);
        if force_pair || similarity(old.line(oi), new.line(ni)) >= PAIR_THRESHOLD {
            rows.push(RowPair {
                left: Cell::Line(oi),
                right: Cell::Line(ni),
                kind: RowKind::Changed,
            });
        } else {
            // 偶然同じ位置に来ただけの無関係な行。左右に分けて出す
            rows.push(removed_row(oi));
            rows.push(added_row(ni));
        }
        stat.removed += 1;
        stat.added += 1;
    }
    for k in paired..removed {
        rows.push(removed_row(before.start + k));
        stat.removed += 1;
    }
    for k in paired..added {
        rows.push(added_row(after.start + k));
        stat.added += 1;
    }
}

fn removed_row(i: u32) -> RowPair {
    RowPair {
        left: Cell::Line(i),
        right: Cell::Pad,
        kind: RowKind::Removed,
    }
}

fn added_row(i: u32) -> RowPair {
    RowPair {
        left: Cell::Pad,
        right: Cell::Line(i),
        kind: RowKind::Added,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn table(s: &str) -> LineTable {
        LineTable::new(Arc::from(s.as_bytes()))
    }

    fn aligned(old: &str, new: &str) -> AlignedDiff {
        align(table(old), table(new))
    }

    /// 左側を rows から復元すると元の旧ファイルに一致する。
    fn reconstruct(d: &AlignedDiff, left: bool) -> Vec<String> {
        d.rows
            .iter()
            .filter_map(|r| {
                let (cell, table) = if left {
                    (r.left, &d.old)
                } else {
                    (r.right, &d.new)
                };
                cell.line()
                    .map(|i| String::from_utf8_lossy(table.line(i)).into_owned())
            })
            .collect()
    }

    fn lines_of(s: &str) -> Vec<String> {
        table(s)
            .iter_lines()
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect()
    }

    fn assert_invariants(old: &str, new: &str) {
        let d = aligned(old, new);
        assert_eq!(reconstruct(&d, true), lines_of(old), "左側の復元");
        assert_eq!(reconstruct(&d, false), lines_of(new), "右側の復元");
        for r in &d.rows {
            if r.kind == RowKind::Context {
                let (l, rr) = (r.left.line().unwrap(), r.right.line().unwrap());
                assert_eq!(d.old.line(l), d.new.line(rr), "Context 行が不一致");
            }
        }
        let mut prev_end = 0;
        for h in &d.hunks {
            assert!(h.rows.start >= prev_end, "hunk が重複または逆順");
            assert!(h.rows.start < h.rows.end);
            prev_end = h.rows.end;
        }
    }

    #[test]
    fn context_only_when_identical() {
        let d = aligned("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(d.rows.len(), 3);
        assert!(d.rows.iter().all(|r| r.kind == RowKind::Context));
        assert!(d.hunks.is_empty());
    }

    #[test]
    fn keeps_all_context_lines_outside_hunks() {
        // 変更箇所から離れた行も全て残る (全文表示の要件)
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let new = "1\n2\n3\n4\nX\n6\n7\n8\n9\n10\n";
        let d = aligned(old, new);
        assert_eq!(d.rows.len(), 10);
        assert_eq!(d.hunks.len(), 1);
        assert_invariants(old, new);
    }

    #[test]
    fn pure_addition_pads_left() {
        let d = aligned("a\n", "a\nb\nc\n");
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.rows[1].kind, RowKind::Added);
        assert_eq!(d.rows[1].left, Cell::Pad);
        assert_eq!(d.stat.added, 2);
        assert_eq!(d.stat.removed, 0);
    }

    #[test]
    fn pure_deletion_pads_right() {
        let d = aligned("a\nb\nc\n", "a\n");
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.rows[1].kind, RowKind::Removed);
        assert_eq!(d.rows[1].right, Cell::Pad);
        assert_eq!(d.stat.removed, 2);
    }

    #[test]
    fn similar_lines_pair_into_changed_row() {
        let d = aligned("let x = 1;\n", "let x = 2;\n");
        assert_eq!(d.rows.len(), 1);
        assert_eq!(d.rows[0].kind, RowKind::Changed);
    }

    #[test]
    fn single_line_replacement_always_pairs() {
        let d = aligned("fn main() {}\n", "struct Foo;\n");
        assert_eq!(d.rows.len(), 1);
        assert_eq!(d.rows[0].kind, RowKind::Changed);
    }

    #[test]
    fn dissimilar_lines_in_multiline_block_split_into_two_rows() {
        let old = "fn main() {}\nlet x = 1;\n";
        let new = "struct Foo;\nlet x = 2;\n";
        let d = aligned(old, new);
        assert_invariants(old, new);
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.rows[0].kind, RowKind::Removed);
        assert_eq!(d.rows[1].kind, RowKind::Added);
        assert_eq!(d.rows[2].kind, RowKind::Changed);
    }

    #[test]
    fn one_to_three_change() {
        let old = "a\nb\nc\n";
        let new = "a\nb1\nb2\nb3\nc\n";
        let d = aligned(old, new);
        assert_invariants(old, new);
        assert_eq!(d.rows.len(), 5);
    }

    #[test]
    fn empty_old_means_all_added() {
        let d = aligned("", "a\nb\n");
        assert_eq!(d.rows.len(), 2);
        assert!(d.rows.iter().all(|r| r.kind == RowKind::Added));
    }

    #[test]
    fn invariants_hold_for_various_inputs() {
        let cases = [
            ("", ""),
            ("a\n", ""),
            ("", "a\n"),
            ("a\nb\nc\n", "c\nb\na\n"),
            ("x\n", "x"),
            ("head\nmid\ntail\n", "head\ntail\n"),
            ("a\n\n\nb\n", "a\nb\n"),
            ("日本語\nコメント\n", "日本語\n変更\n"),
        ];
        for (old, new) in cases {
            assert_invariants(old, new);
        }
    }

    #[test]
    fn missing_trailing_newline_is_tracked() {
        let t = table("a\nb");
        assert_eq!(t.len(), 2);
        assert!(t.is_last_without_newline(1));
    }

    #[test]
    fn crlf_is_visible_in_diff_but_stripped_for_display() {
        let t = table("a\r\n");
        assert!(t.has_cr(0));
        assert_eq!(t.line_display(0), b"a");
        assert_eq!(t.line(0), b"a\r");
        // 改行コードのみの変更も差分として検出される
        let d = aligned("a\n", "a\r\n");
        assert_eq!(d.hunks.len(), 1);
    }
}
