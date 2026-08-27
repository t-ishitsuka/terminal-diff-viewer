use std::path::{Path, PathBuf};

use crate::git::ChangeKind;

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    /// ルートからの相対パス。ルート自身は空。
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: u16,
    pub parent: Option<u32>,
    /// None は未読み込み。ディレクトリの遅延展開に使う。
    pub children: Option<Vec<u32>>,
    pub expanded: bool,
    pub status: Option<ChangeKind>,
}

impl Node {
    pub fn dir(name: impl Into<String>, path: PathBuf, depth: u16, parent: Option<u32>) -> Self {
        Self {
            name: name.into(),
            path,
            is_dir: true,
            depth,
            parent,
            children: None,
            expanded: false,
            status: None,
        }
    }

    pub fn file(name: impl Into<String>, path: PathBuf, depth: u16, parent: Option<u32>) -> Self {
        Self {
            name: name.into(),
            path,
            is_dir: false,
            depth,
            parent,
            children: Some(Vec::new()),
            expanded: false,
            status: None,
        }
    }
}

/// ノードの木と、展開状態を反映した描画用の平坦リストを分けて持つ。
/// 描画は可視スライスのみを参照するため、ノード数に依存しない。
pub struct TreeModel {
    nodes: Vec<Node>,
    visible: Vec<u32>,
    selected: usize,
    offset: usize,
    dirty: bool,
    /// ファイル名の絞り込み。小文字化した部分一致で判定する。
    filter: Option<String>,
}

impl Default for TreeModel {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeModel {
    pub fn new() -> Self {
        let root = Node {
            name: String::new(),
            path: PathBuf::new(),
            is_dir: true,
            depth: 0,
            parent: None,
            children: None,
            expanded: true,
            status: None,
        };
        Self {
            nodes: vec![root],
            visible: Vec::new(),
            selected: 0,
            offset: 0,
            dirty: true,
            filter: None,
        }
    }

    pub const ROOT: u32 = 0;

    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn push_child(&mut self, parent: u32, node: Node) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(node);
        let p = &mut self.nodes[parent as usize];
        p.children.get_or_insert_with(Vec::new).push(id);
        self.dirty = true;
        id
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn children_count(&self, id: u32) -> usize {
        self.nodes[id as usize]
            .children
            .as_ref()
            .map_or(0, Vec::len)
    }

    pub fn expanded_dir_paths(&self) -> Vec<PathBuf> {
        self.nodes
            .iter()
            .filter(|n| n.is_dir && n.expanded && n.parent.is_some())
            .map(|n| n.path.clone())
            .collect()
    }

    pub fn children_loaded(&self, id: u32) -> bool {
        self.nodes[id as usize].children.is_some()
    }

    pub fn mark_loaded(&mut self, id: u32) {
        let n = &mut self.nodes[id as usize];
        if n.children.is_none() {
            n.children = Some(Vec::new());
        }
        self.dirty = true;
    }

    pub fn set_status(&mut self, id: u32, status: Option<ChangeKind>) {
        self.nodes[id as usize].status = status;
    }

    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    /// 絞り込みを設定する。一致するノードとその祖先だけが可視になる。
    pub fn set_filter(&mut self, filter: Option<String>) {
        self.filter = filter.filter(|f| !f.is_empty()).map(|f| f.to_lowercase());
        self.dirty = true;
    }

    /// 絞り込み時の可視ノード収集。子孫に一致があれば祖先も残す。
    fn collect_filtered(&self, id: u32, needle: &str, out: &mut Vec<u32>) -> bool {
        let node = &self.nodes[id as usize];
        let hit = node.name.to_lowercase().contains(needle);
        let start = out.len();
        out.push(id);
        let mut child_hit = false;
        if let Some(children) = &node.children {
            for child in children {
                child_hit |= self.collect_filtered(*child, needle, out);
            }
        }
        if hit || child_hit {
            true
        } else {
            out.truncate(start);
            false
        }
    }

    fn rebuild(&mut self) {
        let mut visible = Vec::new();
        match self.filter.clone() {
            Some(needle) => {
                let roots = self.nodes[0].children.clone().unwrap_or_default();
                for child in roots {
                    self.collect_filtered(child, &needle, &mut visible);
                }
            }
            None => {
                let mut stack: Vec<u32> = Vec::new();
                if let Some(children) = &self.nodes[0].children {
                    stack.extend(children.iter().rev());
                }
                while let Some(id) = stack.pop() {
                    visible.push(id);
                    let n = &self.nodes[id as usize];
                    if n.is_dir
                        && n.expanded
                        && let Some(children) = &n.children
                    {
                        stack.extend(children.iter().rev());
                    }
                }
            }
        }
        self.visible = visible;
        self.dirty = false;
        if self.selected >= self.visible.len() {
            self.selected = self.visible.len().saturating_sub(1);
        }
    }

    pub fn visible(&mut self) -> &[u32] {
        if self.dirty {
            self.rebuild();
        }
        &self.visible
    }

    pub fn visible_len(&mut self) -> usize {
        self.visible().len()
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn selected_id(&mut self) -> Option<u32> {
        let index = self.selected;
        self.visible().get(index).copied()
    }

    pub fn selected_node(&mut self) -> Option<&Node> {
        let id = self.selected_id()?;
        Some(self.node(id))
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.visible_len();
        if len == 0 {
            return;
        }
        let next = (self.selected as isize + delta).clamp(0, len as isize - 1);
        self.selected = next as usize;
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.visible_len().saturating_sub(1);
    }

    /// 選択行が画面内に収まるようスクロール位置を調整する。
    pub fn scroll_into_view(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        let len = self.visible_len();
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
        let max_offset = len.saturating_sub(height);
        self.offset = self.offset.min(max_offset);
    }

    pub fn expand(&mut self, id: u32) {
        let n = &mut self.nodes[id as usize];
        if n.is_dir && !n.expanded {
            n.expanded = true;
            self.dirty = true;
        }
    }

    pub fn collapse(&mut self, id: u32) {
        let n = &mut self.nodes[id as usize];
        if n.is_dir && n.expanded {
            n.expanded = false;
            self.dirty = true;
        }
    }

    pub fn toggle(&mut self, id: u32) {
        let n = &mut self.nodes[id as usize];
        if n.is_dir {
            n.expanded = !n.expanded;
            self.dirty = true;
        }
    }

    pub fn parent_of(&self, id: u32) -> Option<u32> {
        self.nodes[id as usize].parent.filter(|p| *p != Self::ROOT)
    }

    pub fn select_node(&mut self, id: u32) {
        if self.dirty {
            self.rebuild();
        }
        if let Some(pos) = self.visible.iter().position(|v| *v == id) {
            self.selected = pos;
        }
    }

    pub fn find_by_path(&self, path: &Path) -> Option<u32> {
        self.nodes
            .iter()
            .position(|n| n.path == path)
            .map(|i| i as u32)
    }

    /// 対象ノードの祖先を全て展開し、選択する。
    pub fn reveal(&mut self, id: u32) {
        let mut cur = self.nodes[id as usize].parent;
        while let Some(p) = cur {
            self.nodes[p as usize].expanded = true;
            cur = self.nodes[p as usize].parent;
        }
        self.dirty = true;
        self.select_node(id);
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> TreeModel {
        let mut t = TreeModel::new();
        let src = t.push_child(
            TreeModel::ROOT,
            Node::dir("src", PathBuf::from("src"), 0, Some(0)),
        );
        t.push_child(
            src,
            Node::file("main.rs", PathBuf::from("src/main.rs"), 1, Some(src)),
        );
        t.push_child(
            TreeModel::ROOT,
            Node::file("Cargo.toml", PathBuf::from("Cargo.toml"), 0, Some(0)),
        );
        t
    }

    #[test]
    fn collapsed_directory_hides_children() {
        let mut t = build();
        assert_eq!(t.visible_len(), 2);
    }

    #[test]
    fn expanding_reveals_children_in_order() {
        let mut t = build();
        let src = t.find_by_path(Path::new("src")).unwrap();
        t.expand(src);
        let names: Vec<&str> = t
            .visible()
            .to_vec()
            .iter()
            .map(|id| t.node(*id).name.as_str())
            .collect();
        assert_eq!(names, vec!["src", "main.rs", "Cargo.toml"]);
    }

    #[test]
    fn reveal_expands_ancestors() {
        let mut t = build();
        let file = t.find_by_path(Path::new("src/main.rs")).unwrap();
        t.reveal(file);
        assert_eq!(t.selected_node().unwrap().name, "main.rs");
    }

    #[test]
    fn selection_is_clamped() {
        let mut t = build();
        t.move_selection(100);
        assert_eq!(t.selected_index(), 1);
        t.move_selection(-100);
        assert_eq!(t.selected_index(), 0);
    }

    #[test]
    fn scroll_follows_selection() {
        let mut t = build();
        t.select_last();
        t.scroll_into_view(1);
        assert_eq!(t.offset(), 1);
        t.select_first();
        t.scroll_into_view(1);
        assert_eq!(t.offset(), 0);
    }
}
