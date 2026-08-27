use crate::ui::text::TextOpts;

/// 既定値。設定ファイル読み込みは M5 で追加する (docs/06-implementation-plan.md)。
#[derive(Clone, Debug)]
pub struct Config {
    /// 左ペインの比率。右ペインは 10 - tree_ratio。
    pub tree_ratio: u16,
    pub show_status_bar: bool,
    pub text: TextOpts,
    /// 差分表示で既定を全行表示にするか。
    pub full_file: bool,
    /// 折り畳み時に変更箇所の前後へ残す行数。
    pub fold_context: usize,
    pub inline_words: bool,
    pub max_file_bytes: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tree_ratio: 3,
            show_status_bar: true,
            text: TextOpts::default(),
            full_file: true,
            fold_context: 3,
            inline_words: true,
            max_file_bytes: 2 * 1024 * 1024,
        }
    }
}
