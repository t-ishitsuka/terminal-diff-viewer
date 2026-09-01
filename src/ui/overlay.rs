use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme::Theme;
use crate::app::{App, Overlay};

const HELP: &[(&str, &str)] = &[
    ("q / Ctrl-c", "終了"),
    ("?", "このヘルプ"),
    ("m", "モードを巡回 (tree → diff → log)"),
    ("t / d / L", "tree / diff / log モードを直接指定"),
    ("Tab", "ペイン間のフォーカス移動"),
    ("r", "リロード"),
    ("< / >", "左ペイン幅の調整"),
    ("", ""),
    ("j / k", "上下移動・スクロール"),
    ("Ctrl-d / Ctrl-u", "半画面スクロール"),
    ("Ctrl-f / Ctrl-b", "1 画面スクロール"),
    ("g / G", "先頭 / 末尾"),
    ("h / l", "ツリー: 折畳/展開  内容: 横スクロール"),
    ("Enter", "ファイルを開く / 省略部分を展開"),
    ("z", "ツリー: 展開トグル  内容: 折り畳みトグル"),
    ("u", "side-by-side / unified 切替"),
    ("w", "行折り返しトグル"),
    ("", ""),
    ("]c / [c", "次 / 前の変更箇所"),
    ("]f / [f", "次 / 前の変更ファイル"),
    ("", ""),
    ("/", "ツリー: 名前で絞り込み  内容: 検索"),
    ("n / N", "次 / 前の検索一致"),
    ("I", "ignore 対象の表示トグル (tree)"),
    ("T", "階層 / フラット表示トグル (diff)"),
    ("S", "並び順トグル: パス / 変更種別 (diff)"),
    ("s", "比較対象トグル: 作業ツリー / staged / index"),
    ("a", "選択中のファイルを stage"),
    ("U", "選択中のファイルを unstage"),
    ("R", "ref 間比較を指定 (例 HEAD~3..HEAD)"),
];

pub fn draw(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    if app.overlay != Overlay::Help {
        return;
    }
    let width = 56u16.min(area.width.saturating_sub(2));
    let height = (HELP.len() as u16 + 2).min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    let lines: Vec<Line> = HELP
        .iter()
        .map(|(key, desc)| {
            if key.is_empty() {
                Line::raw("")
            } else {
                Line::from(format!("  {key:<16} {desc}"))
            }
        })
        .collect();

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::new()
                .borders(Borders::ALL)
                .border_style(theme.dim)
                .title(" キーバインド "),
        ),
        popup,
    );
}
