use std::ops::Range;
use std::sync::Arc;

use crate::git::LineStat;

/// ファイル内容を保持し、行を範囲参照で提供する。行の実体は複製しない。
#[derive(Clone, Debug, Default)]
pub struct LineTable {
    bytes: Arc<[u8]>,
    ranges: Vec<(u32, u32)>,
    trailing_newline: bool,
}

impl LineTable {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        let mut ranges = Vec::new();
        let mut start = 0usize;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\n' {
                ranges.push((start as u32, i as u32));
                start = i + 1;
            }
        }
        let trailing_newline = start == bytes.len();
        if !trailing_newline {
            ranges.push((start as u32, bytes.len() as u32));
        }
        Self {
            bytes,
            ranges,
            trailing_newline,
        }
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// 改行を除いた行。CRLF の `\r` は含んだまま返す (差分計算用)。
    pub fn line(&self, i: u32) -> &[u8] {
        let (s, e) = self.ranges[i as usize];
        &self.bytes[s as usize..e as usize]
    }

    /// 表示用。末尾の `\r` を除く。
    pub fn line_display(&self, i: u32) -> &[u8] {
        let raw = self.line(i);
        raw.strip_suffix(b"\r").unwrap_or(raw)
    }

    pub fn has_cr(&self, i: u32) -> bool {
        self.line(i).ends_with(b"\r")
    }

    /// 最終行が改行で終わっていない場合に true。
    pub fn is_last_without_newline(&self, i: u32) -> bool {
        !self.trailing_newline && i as usize + 1 == self.ranges.len()
    }

    pub fn iter_lines(&self) -> impl Iterator<Item = &[u8]> + '_ {
        (0..self.ranges.len() as u32).map(|i| self.line(i))
    }
}

/// 各側のセル。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Cell {
    Line(u32),
    /// 対向にのみ行がある。
    Pad,
}

impl Cell {
    pub fn line(self) -> Option<u32> {
        match self {
            Cell::Line(i) => Some(i),
            Cell::Pad => None,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RowKind {
    Context,
    Removed,
    Added,
    /// 左右とも存在し内容が異なる。語単位ハイライトの対象。
    Changed,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RowPair {
    pub left: Cell,
    pub right: Cell,
    pub kind: RowKind,
}

/// 変更ブロックの rows 上の位置。ジャンプに使う。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HunkAnchor {
    pub rows: Range<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct AlignedDiff {
    pub rows: Vec<RowPair>,
    pub old: LineTable,
    pub new: LineTable,
    pub hunks: Vec<HunkAnchor>,
    pub stat: LineStat,
}

impl AlignedDiff {
    /// `from` より後で最初に始まる変更ブロックの先頭行。
    /// 指定行より後にある最初の変更箇所の番号。
    pub fn next_hunk_index(&self, row: usize) -> Option<usize> {
        self.hunks.iter().position(|h| h.rows.start as usize > row)
    }

    /// 指定行より前にある最後の変更箇所の番号。
    pub fn prev_hunk_index(&self, row: usize) -> Option<usize> {
        self.hunks
            .iter()
            .rposition(|h| (h.rows.start as usize) < row)
    }

    pub fn hunk_index_at(&self, row: usize) -> Option<usize> {
        self.hunks
            .iter()
            .position(|h| (h.rows.start as usize) <= row && row < h.rows.end as usize)
    }
}
