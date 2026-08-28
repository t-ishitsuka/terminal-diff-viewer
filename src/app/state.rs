use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::diff::{AlignedDiff, InlineSpans, LineTable, RowKind, inline_diff};
use crate::git::{
    ChangeKind, ChangeSet, DiffSpec, FileChange, GitBackend, HeadInfo, UnsupportedReason,
};
use crate::highlight::Highlighted;
use crate::task::{HighlightTarget, Pool, TaskRequest};
use crate::vfs::{Node, TreeModel};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Mode {
    Tree,
    Diff,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Focus {
    Tree,
    Content,
}

/// diff モードのファイル一覧の並び順。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ChangeSort {
    /// リポジトリルートからの相対パス順。
    Path,
    /// 変更種別ごとにまとめ、同種別内はパス順。
    Kind,
}

impl ChangeSort {
    pub fn next(self) -> Self {
        match self {
            ChangeSort::Path => ChangeSort::Kind,
            ChangeSort::Kind => ChangeSort::Path,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChangeSort::Path => "パス順",
            ChangeSort::Kind => "種別順",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum InputKind {
    /// 内容ペインの検索。
    Search,
    /// ツリーのファイル名絞り込み。
    Filter,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Help,
    Input { kind: InputKind, buffer: String },
}

impl Overlay {
    pub fn is_input(&self) -> bool {
        matches!(self, Overlay::Input { .. })
    }
}

/// 内容ペイン内の検索結果。行と左右の別ごとに一致範囲を引けるようにしておく。
#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub query: String,
    by_row: HashMap<(u32, bool), Vec<Range<usize>>>,
    pub hits: Vec<(u32, bool)>,
    pub current: usize,
}

impl SearchState {
    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.by_row.clear();
        self.hits.clear();
        self.current = 0;
    }

    pub fn set(&mut self, query: String, matches: Vec<(u32, bool, Range<usize>)>) {
        self.query = query;
        self.by_row.clear();
        self.hits.clear();
        self.current = 0;
        for (row, right, range) in matches {
            let entry = self.by_row.entry((row, right)).or_default();
            if entry.is_empty() {
                self.hits.push((row, right));
            }
            entry.push(range);
        }
        self.hits.sort_unstable();
    }

    pub fn ranges(&self, row: u32, right: bool) -> &[Range<usize>] {
        self.by_row
            .get(&(row, right))
            .map_or(&[][..], Vec::as_slice)
    }
}

pub struct TextView {
    pub path: PathBuf,
    pub table: LineTable,
    pub highlight: Option<Arc<Highlighted>>,
    pub offset: usize,
    pub hscroll: usize,
}

/// 折り畳み表示のとき、rows の一部を省略行に置き換えて描画する。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DisplayRow {
    Row(u32),
    Gap { start: u32, count: u32 },
}

pub struct DiffView {
    pub change: FileChange,
    pub diff: AlignedDiff,
    pub old_highlight: Option<Arc<Highlighted>>,
    pub new_highlight: Option<Arc<Highlighted>>,
    pub folded: bool,
    pub display: Vec<DisplayRow>,
    pub expanded_gaps: HashSet<u32>,
    pub offset: usize,
    pub hscroll: usize,
    /// 直近にジャンプした変更箇所の番号。手動スクロールで解除する。
    pub hunk_cursor: Option<usize>,
    pub inline: HashMap<u32, InlineSpans>,
}

impl DiffView {
    pub fn new(change: FileChange, diff: AlignedDiff, folded: bool, context: usize) -> Self {
        let mut view = Self {
            change,
            diff,
            old_highlight: None,
            new_highlight: None,
            folded,
            display: Vec::new(),
            expanded_gaps: HashSet::new(),
            offset: 0,
            hscroll: 0,
            hunk_cursor: None,
            inline: HashMap::new(),
        };
        view.rebuild_display(context);
        view
    }

    /// 折り畳みは rows を作り直さず表示行のフィルタとして計算する。
    /// 全文表示との切り替えで差分の再計算が起きない。
    pub fn rebuild_display(&mut self, context: usize) {
        self.display.clear();
        if !self.folded {
            return;
        }
        let total = self.diff.rows.len();
        let mut keep = vec![false; total];
        for hunk in &self.diff.hunks {
            let start = (hunk.rows.start as usize).saturating_sub(context);
            let end = (hunk.rows.end as usize + context).min(total);
            for slot in keep.iter_mut().take(end).skip(start) {
                *slot = true;
            }
        }

        let mut i = 0usize;
        while i < total {
            if keep[i] {
                self.display.push(DisplayRow::Row(i as u32));
                i += 1;
                continue;
            }
            let start = i;
            while i < total && !keep[i] {
                i += 1;
            }
            let count = i - start;
            // 1 行だけの省略は畳んでも行数が減らないため、そのまま表示する
            if count < 2 || self.expanded_gaps.contains(&(start as u32)) {
                for row in start..i {
                    self.display.push(DisplayRow::Row(row as u32));
                }
            } else {
                self.display.push(DisplayRow::Gap {
                    start: start as u32,
                    count: count as u32,
                });
            }
        }
    }

    pub fn display_len(&self) -> usize {
        if self.folded {
            self.display.len()
        } else {
            self.diff.rows.len()
        }
    }

    pub fn display_row(&self, index: usize) -> Option<DisplayRow> {
        if self.folded {
            self.display.get(index).copied()
        } else {
            (index < self.diff.rows.len()).then_some(DisplayRow::Row(index as u32))
        }
    }

    /// rows 上の位置を表示位置へ変換する。折り畳みの切り替えでスクロール位置を保つ。
    pub fn display_index_of_row(&self, row: u32) -> usize {
        if !self.folded {
            return row as usize;
        }
        self.display
            .iter()
            .position(|d| match d {
                DisplayRow::Row(r) => *r >= row,
                DisplayRow::Gap { start, count } => row < start + count,
            })
            .unwrap_or_else(|| self.display.len().saturating_sub(1))
    }

    pub fn row_at_display(&self, index: usize) -> u32 {
        match self.display_row(index) {
            Some(DisplayRow::Row(r)) => r,
            Some(DisplayRow::Gap { start, .. }) => start,
            None => 0,
        }
    }

    /// 変更箇所ジャンプの基準行。ジャンプ後に対象を置く位置と同じにして、
    /// 続けて押したときに同じ変更箇所へ戻らないようにする。
    pub fn anchor_row(&self, height: usize) -> u32 {
        let len = self.display_len();
        if len == 0 {
            return 0;
        }
        let index = (self.offset + height.max(1) / 4).min(len - 1);
        self.row_at_display(index)
    }

    /// 変更ペア行の語単位差分。可視行のみ遅延計算し、結果を保持する。
    pub fn inline_spans(&mut self, row: u32, enabled: bool) -> InlineSpans {
        if !enabled {
            return InlineSpans::default();
        }
        if let Some(cached) = self.inline.get(&row) {
            return cached.clone();
        }
        let pair = self.diff.rows[row as usize];
        let spans = if pair.kind == RowKind::Changed {
            let old = pair.left.line().map(|i| self.diff.old.line_display(i));
            let new = pair.right.line().map(|i| self.diff.new.line_display(i));
            match (old.map(std::str::from_utf8), new.map(std::str::from_utf8)) {
                (Some(Ok(o)), Some(Ok(n))) => inline_diff(o, n),
                // 不正な UTF-8 の行は語単位に分解しない
                _ => InlineSpans::default(),
            }
        } else {
            InlineSpans::default()
        };
        self.inline.insert(row, spans.clone());
        spans
    }
}

pub enum ContentState {
    Empty,
    Loading {
        path: PathBuf,
    },
    Text(Box<TextView>),
    Diff(Box<DiffView>),
    /// 仕様上の正常な結果 (バイナリ / サイズ超過)。
    Unsupported {
        path: PathBuf,
        reason: UnsupportedReason,
    },
    /// 異常。原因を残して表示する。
    Failed {
        path: PathBuf,
        error: String,
    },
}

impl ContentState {
    pub fn path(&self) -> Option<&Path> {
        match self {
            ContentState::Empty => None,
            ContentState::Loading { path }
            | ContentState::Unsupported { path, .. }
            | ContentState::Failed { path, .. } => Some(path),
            ContentState::Text(v) => Some(&v.path),
            ContentState::Diff(v) => Some(&v.change.path),
        }
    }
}

pub struct App {
    pub cfg: Config,
    pub root: PathBuf,
    pub backend: Option<Arc<dyn GitBackend>>,
    pub pool: Pool,
    pub head: Option<HeadInfo>,
    pub mode: Mode,
    pub focus: Focus,
    pub overlay: Overlay,
    pub fs_tree: TreeModel,
    pub change_tree: TreeModel,
    pub changes: ChangeSet,
    pub content: ContentState,
    pub search: SearchState,
    pub notice: Option<String>,
    pub generation: u64,
    pub status_generation: u64,
    pub tree_ratio: u16,
    pub show_ignored: bool,
    pub hierarchical_changes: bool,
    pub change_sort: ChangeSort,
    /// 差分の比較対象。起動後にキーで切り替える。
    pub diff_spec: DiffSpec,
    /// 差分を強制的に unified 表示にする (端末幅による縮退とは独立)。
    pub unified: bool,
    /// 内容ペインの行折り返し。
    pub wrap: bool,
    pub scanning: bool,
    pub should_quit: bool,
    /// ツリー再構築時に展開状態と選択を復元するための保留情報。
    pub pending_expand: HashSet<PathBuf>,
    pub pending_select: Option<PathBuf>,
    /// 直近の描画で使ったペイン高さ。スクロール量の決定に使う。
    pub tree_height: usize,
    pub content_height: usize,
}

impl App {
    pub fn new(
        cfg: Config,
        root: PathBuf,
        backend: Option<Arc<dyn GitBackend>>,
        pool: Pool,
    ) -> Self {
        let tree_ratio = cfg.tree_ratio;
        let mut app = Self {
            cfg,
            root,
            backend,
            pool,
            head: None,
            mode: Mode::Tree,
            focus: Focus::Tree,
            overlay: Overlay::None,
            fs_tree: TreeModel::new(),
            change_tree: TreeModel::new(),
            changes: ChangeSet::default(),
            content: ContentState::Empty,
            search: SearchState::default(),
            notice: None,
            generation: 0,
            status_generation: 0,
            tree_ratio,
            show_ignored: false,
            hierarchical_changes: false,
            change_sort: ChangeSort::Path,
            diff_spec: DiffSpec::WorktreeVsHead,
            unified: false,
            wrap: false,
            scanning: false,
            should_quit: false,
            pending_expand: HashSet::new(),
            pending_select: None,
            tree_height: 1,
            content_height: 1,
        };
        app.request_root_dir();
        app.request_status();
        app
    }

    pub fn tree(&mut self) -> &mut TreeModel {
        match self.mode {
            Mode::Tree => &mut self.fs_tree,
            Mode::Diff => &mut self.change_tree,
        }
    }

    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    pub fn request_root_dir(&mut self) {
        let generation = self.next_generation();
        self.pool.submit(TaskRequest::ReadDir {
            generation,
            node: TreeModel::ROOT,
            dir: self.root.clone(),
            show_ignored: self.show_ignored,
        });
    }

    pub fn request_status(&mut self) {
        if self.backend.is_none() {
            return;
        }
        self.status_generation += 1;
        self.scanning = true;
        let generation = self.status_generation;
        self.pool.submit(TaskRequest::ScanStatus {
            generation,
            spec: self.diff_spec,
        });
    }

    pub fn request_dir(&mut self, node: u32, rel: &Path) {
        let generation = self.next_generation();
        self.pool.submit(TaskRequest::ReadDir {
            generation,
            node,
            dir: self.root.join(rel),
            show_ignored: self.show_ignored,
        });
    }

    fn highlight_options(&self) -> Option<(String, usize)> {
        self.cfg
            .syntax_highlight
            .then(|| (self.cfg.syntax_theme.clone(), self.cfg.max_highlight_lines))
    }

    /// 本文が届いた後に色付けを依頼する。5000 行規模では色付けが差分計算より
    /// 2 桁重いため、素のテキストを先に出して色を後から重ねる。
    ///
    /// 可視範囲ぶんと全文を続けて投げる。ワーカーは先入れ先出しで処理するため、
    /// 画面に見えている範囲の色が先に届く。
    pub fn request_highlight(
        &self,
        generation: u64,
        target: HighlightTarget,
        path: PathBuf,
        table: LineTable,
    ) {
        let Some((theme, max_lines)) = self.highlight_options() else {
            return;
        };
        // 画面 1 面ぶんでは足りない場合があるため少し多めに色付けする
        let visible = self.content_height.saturating_mul(2).max(64);
        if visible < table.len() {
            self.pool.submit(TaskRequest::Highlight {
                generation,
                target,
                path: path.clone(),
                table: table.clone(),
                theme: theme.clone(),
                max_lines,
                upto: visible,
            });
        }
        self.pool.submit(TaskRequest::Highlight {
            generation,
            target,
            path,
            table,
            theme,
            max_lines,
            upto: usize::MAX,
        });
    }
    /// ツリーの選択に応じて右ペインの読み込みを依頼する。
    /// 世代番号を進めることで、押しっぱなしの移動中に届く古い結果を捨てられる。
    pub fn request_content(&mut self) {
        let Some(node) = self.tree().selected_node().cloned() else {
            self.content = ContentState::Empty;
            return;
        };
        if node.is_dir() {
            self.content = ContentState::Empty;
            return;
        }
        let generation = self.next_generation();
        self.search.clear();
        match self.mode {
            Mode::Tree => {
                let abs = self.root.join(&node.path);
                self.content = ContentState::Loading {
                    path: node.path.clone(),
                };
                self.pool.submit(TaskRequest::LoadText {
                    generation,
                    path: node.path,
                    abs,
                });
            }
            Mode::Diff => {
                let Some(change) = self.changes.find(&node.path).cloned() else {
                    self.content = ContentState::Empty;
                    return;
                };
                self.content = ContentState::Loading {
                    path: node.path.clone(),
                };
                self.pool.submit(TaskRequest::ComputeDiff {
                    generation,
                    change,
                    spec: self.diff_spec,
                });
            }
        }
    }

    /// 現在のソート順で並べたファイル一覧。パス順は取得時点で整列済み。
    fn sorted_changes(&self) -> Vec<&FileChange> {
        let mut files: Vec<&FileChange> = self.changes.files.iter().collect();
        if self.change_sort == ChangeSort::Kind {
            files.sort_by(|a, b| {
                a.kind
                    .order()
                    .cmp(&b.kind.order())
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        files
    }

    /// 変更ファイル一覧から diff モード用のツリーを組み直す。
    pub fn rebuild_change_tree(&mut self) {
        let previous = self
            .change_tree
            .selected_node()
            .map(|n| n.path.clone())
            .filter(|p| !p.as_os_str().is_empty());
        let mut tree = TreeModel::new();
        let files = self.sorted_changes();

        if self.hierarchical_changes {
            let mut dirs: HashMap<PathBuf, u32> = HashMap::new();
            for change in &files {
                let parent_path = change.path.parent().unwrap_or(Path::new(""));
                let parent = ensure_dir(&mut tree, &mut dirs, parent_path);
                let depth = tree.node(parent).depth + u16::from(parent != TreeModel::ROOT);
                let name = file_name(&change.path);
                let id = tree.push_child(
                    parent,
                    Node::file(name, change.path.clone(), depth, Some(parent)),
                );
                tree.set_status(id, Some(change.kind));
            }
        } else {
            for change in &files {
                let label = change.path.to_string_lossy().into_owned();
                let id = tree.push_child(
                    TreeModel::ROOT,
                    Node::file(label, change.path.clone(), 0, Some(TreeModel::ROOT)),
                );
                tree.set_status(id, Some(change.kind));
            }
        }

        self.change_tree = tree;
        if let Some(previous) = previous
            && let Some(id) = self.change_tree.find_by_path(&previous)
        {
            self.change_tree.reveal(id);
        }
    }

    /// tree モードのツリーに Git ステータスを反映する。
    pub fn apply_status_to_fs_tree(&mut self) {
        let map: HashMap<&Path, ChangeKind> = self
            .changes
            .files
            .iter()
            .map(|c| (c.path.as_path(), c.kind))
            .collect();
        for id in 0..self.fs_tree.node_count() as u32 {
            let status = map.get(self.fs_tree.node(id).path.as_path()).copied();
            self.fs_tree.set_status(id, status);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// 中間ディレクトリを必要な分だけ作り、その id を返す。
fn ensure_dir(tree: &mut TreeModel, dirs: &mut HashMap<PathBuf, u32>, path: &Path) -> u32 {
    if path.as_os_str().is_empty() {
        return TreeModel::ROOT;
    }
    if let Some(id) = dirs.get(path) {
        return *id;
    }
    let parent_path = path.parent().unwrap_or(Path::new(""));
    let parent = ensure_dir(tree, dirs, parent_path);
    let depth = tree.node(parent).depth + u16::from(parent != TreeModel::ROOT);
    let id = tree.push_child(
        parent,
        Node::dir(file_name(path), path.to_path_buf(), depth, Some(parent)),
    );
    tree.expand(id);
    dirs.insert(path.to_path_buf(), id);
    id
}
