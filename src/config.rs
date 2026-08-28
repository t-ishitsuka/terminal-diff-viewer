use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::highlight::DEFAULT_THEME;
use crate::ui::text::TextOpts;
use crate::ui::theme::Palette;

#[derive(Clone, Debug)]
pub struct Config {
    /// 左ペインの比率 (20 分率)。右ペインは 20 - tree_ratio。
    pub tree_ratio: u16,
    pub show_status_bar: bool,
    pub text: TextOpts,
    /// 差分表示で既定を全行表示にするか。
    pub full_file: bool,
    /// 折り畳み時に変更箇所の前後へ残す行数。
    pub fold_context: usize,
    pub inline_words: bool,
    pub max_file_bytes: u64,
    pub syntax_highlight: bool,
    pub syntax_theme: String,
    /// これを超える行数のファイルは色付けしない。
    pub max_highlight_lines: usize,
    pub palette: Palette,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tree_ratio: 5,
            show_status_bar: true,
            text: TextOpts::default(),
            full_file: true,
            fold_context: 3,
            inline_words: true,
            max_file_bytes: 2 * 1024 * 1024,
            syntax_highlight: true,
            syntax_theme: DEFAULT_THEME.to_string(),
            max_highlight_lines: 20_000,
            palette: Palette::RedGreen,
        }
    }
}

/// 既定の設定ファイルの場所。存在しなければ既定値で動作する。
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("tdv").join("config.toml"))
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let explicit = path.is_some();
        let Some(path) = path.map(Path::to_path_buf).or_else(default_path) else {
            return Ok(Self::default());
        };
        if !path.exists() {
            if explicit {
                bail!("設定ファイルが見つからない: {}", path.display());
            }
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("{} を読み込めない", path.display()))?;
        let file: FileConfig =
            toml::from_str(&text).with_context(|| format!("{} の解析に失敗", path.display()))?;
        file.into_config()
            .with_context(|| format!("{} の設定値が不正", path.display()))
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    ui: Option<UiSection>,
    diff: Option<DiffSection>,
    theme: Option<ThemeSection>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct UiSection {
    tree_ratio: Option<u16>,
    show_status_bar: Option<bool>,
    tab_width: Option<usize>,
    ambiguous_width_wide: Option<bool>,
    syntax_highlight: Option<bool>,
    max_highlight_lines: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DiffSection {
    full_file: Option<bool>,
    fold_context: Option<usize>,
    inline_words: Option<bool>,
    max_file_bytes: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ThemeSection {
    palette: Option<String>,
    syntax: Option<String>,
}

impl FileConfig {
    fn into_config(self) -> Result<Config> {
        let mut cfg = Config::default();
        if let Some(ui) = self.ui {
            if let Some(ratio) = ui.tree_ratio {
                if !(1..=16).contains(&ratio) {
                    bail!("ui.tree_ratio は 1〜16 の範囲で指定する (指定値: {ratio})");
                }
                cfg.tree_ratio = ratio;
            }
            if let Some(v) = ui.show_status_bar {
                cfg.show_status_bar = v;
            }
            if let Some(v) = ui.tab_width {
                if v == 0 {
                    bail!("ui.tab_width は 1 以上で指定する");
                }
                cfg.text.tab_width = v;
            }
            if let Some(v) = ui.ambiguous_width_wide {
                cfg.text.ambiguous_wide = v;
            }
            if let Some(v) = ui.syntax_highlight {
                cfg.syntax_highlight = v;
            }
            if let Some(v) = ui.max_highlight_lines {
                cfg.max_highlight_lines = v;
            }
        }
        if let Some(diff) = self.diff {
            if let Some(v) = diff.full_file {
                cfg.full_file = v;
            }
            if let Some(v) = diff.fold_context {
                cfg.fold_context = v;
            }
            if let Some(v) = diff.inline_words {
                cfg.inline_words = v;
            }
            if let Some(v) = diff.max_file_bytes {
                cfg.max_file_bytes = v;
            }
        }
        if let Some(theme) = self.theme {
            if let Some(name) = theme.palette {
                cfg.palette = Palette::parse(&name).with_context(|| {
                    format!("theme.palette は red-green か blue-orange (指定値: {name})")
                })?;
            }
            if let Some(name) = theme.syntax {
                let known = crate::highlight::theme_names();
                if !known.contains(&name) {
                    bail!(
                        "theme.syntax が不明 (指定値: {name})。利用可能: {}",
                        known.join(", ")
                    );
                }
                cfg.syntax_theme = name;
            }
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config> {
        toml::from_str::<FileConfig>(text)?.into_config()
    }

    #[test]
    fn empty_file_yields_defaults() {
        let cfg = parse("").unwrap();
        assert_eq!(cfg.tree_ratio, Config::default().tree_ratio);
    }

    #[test]
    fn values_are_applied() {
        let cfg = parse(
            r#"
            [ui]
            tree_ratio = 4
            tab_width = 8
            [diff]
            full_file = false
            fold_context = 5
            [theme]
            palette = "blue-orange"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.tree_ratio, 4);
        assert_eq!(cfg.text.tab_width, 8);
        assert!(!cfg.full_file);
        assert_eq!(cfg.fold_context, 5);
        assert_eq!(cfg.palette, Palette::BlueOrange);
    }

    #[test]
    fn unknown_key_is_reported() {
        let error = parse("[ui]\nunknown_key = 1\n").unwrap_err().to_string();
        assert!(error.contains("unknown_key"), "{error}");
    }

    #[test]
    fn out_of_range_tree_ratio_is_reported() {
        let error = parse("[ui]\ntree_ratio = 17\n").unwrap_err().to_string();
        assert!(error.contains("tree_ratio"), "{error}");
    }

    #[test]
    fn unknown_palette_is_reported() {
        let error = parse("[theme]\npalette = \"pink\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("palette"), "{error}");
    }

    #[test]
    fn explicitly_specified_missing_file_is_an_error() {
        let missing = Path::new("/nonexistent/tdv/config.toml");
        assert!(Config::load(Some(missing)).is_err());
    }

    #[test]
    fn load_reads_an_actual_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[ui]\ntree_ratio = 8\n[theme]\npalette = \"blue-orange\"\n",
        )
        .unwrap();
        let cfg = Config::load(Some(&path)).unwrap();
        assert_eq!(cfg.tree_ratio, 8);
        assert_eq!(cfg.palette, Palette::BlueOrange);
    }
}
