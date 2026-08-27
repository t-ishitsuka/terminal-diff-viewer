use std::path::PathBuf;

use clap::Parser;

/// ターミナルでディレクトリツリーと git 差分を閲覧する。
#[derive(Parser, Debug)]
#[command(name = "tdv", version, about)]
pub struct Cli {
    /// 表示対象のディレクトリ。省略時はカレントディレクトリ。
    pub path: Option<PathBuf>,

    /// 差分計算を行うファイルサイズの上限 (バイト)。
    #[arg(long)]
    pub max_file_bytes: Option<u64>,

    /// East Asian Ambiguous 文字を全角として扱う。
    #[arg(long)]
    pub ambiguous_wide: bool,

    /// タブの表示幅。
    #[arg(long)]
    pub tab_width: Option<usize>,
}
