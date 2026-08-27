# terminal-diff-viewer (tdv)

ターミナル上でディレクトリツリーと git 差分をエディタ的に閲覧する TUI アプリケーション。

差分表示中も **ファイル全体を読める** ことが他の diff ビューアとの違い。変更箇所の前後数行に視野が限定されないため、変更の妥当性を周辺コードから判断できる。

## 使い方

```sh
cargo run -- [PATH]    # 起動 (常に tree モード)
cargo build --release  # target/release/tdv
nix build              # Nix パッケージとしてビルド
```

`PATH` を省略するとカレントディレクトリを対象にする。Git リポジトリ外でも tree モードは動作する。

起動方法は 1 つのみ。モードは起動後に `m` (トグル) または `t` / `d` で切り替える。

### 主なキー

| キー | 動作 |
| --- | --- |
| `m` / `t` / `d` | モード切替 (トグル / tree / diff) |
| `Tab` | 左右ペインのフォーカス移動 |
| `j` `k` / `Ctrl-d` `Ctrl-u` / `g` `G` | 移動・スクロール |
| `Enter` `l` | ディレクトリ展開 / ファイルを開く / 省略部分を展開 |
| `]c` `[c` | 次 / 前の変更箇所 |
| `]f` `[f` | 次 / 前の変更ファイル |
| `/` | ツリー: 名前で絞り込み  内容: 検索 |
| `n` `N` | 次 / 前の検索一致 |
| `z` | ツリー: 展開トグル  内容: 折り畳みトグル |
| `u` | unified 表示トグル |
| `I` / `T` | ignore 表示 / 階層表示のトグル |
| `r` | リロード |
| `?` / `q` | ヘルプ / 終了 |

### コマンドライン引数

```
tdv [PATH] [--config <PATH>] [--no-highlight] [--max-file-bytes <N>]
           [--ambiguous-wide] [--tab-width <N>]
```

## 設定

`$XDG_CONFIG_HOME/tdv/config.toml` (未設定なら `~/.config/tdv/config.toml`) を読む。無ければ既定値で動作する。未知のキーや範囲外の値は起動前にエラーとして報告する。

```toml
[ui]
tree_ratio = 3              # 左ペインの比率 (右は 10 - tree_ratio)。1〜8
show_status_bar = true
tab_width = 4
ambiguous_width_wide = false
syntax_highlight = true
max_highlight_lines = 20000

[diff]
full_file = true            # 既定で全行表示
fold_context = 3
inline_words = true
max_file_bytes = 2097152

[theme]
palette = "red-green"       # または "blue-orange" (赤緑の識別が難しい場合)
syntax = "base16-ocean.dark"
```

## 実装状況

| 機能 | 状態 |
| --- | --- |
| tree モード (遅延展開・ignore 準拠・ステータス記号) | 実装済み |
| diff モード (作業ツリー vs HEAD、全文 side-by-side) | 実装済み |
| 語単位ハイライト / 変更箇所ジャンプ / 折り畳み / unified 切替 | 実装済み |
| シンタックスハイライト (syntect) | 実装済み |
| 検索 (内容) / 絞り込み (ファイル名) | 実装済み |
| 設定ファイル / Nix パッケージ | 実装済み |
| バイナリ・巨大ファイル・CRLF・末尾改行欠落の扱い | 実装済み |
| キーマップの設定上書き | 未実装 |

詳細は [docs/06-implementation-plan.md](docs/06-implementation-plan.md) の進捗表を参照。

## ドキュメント

| ファイル | 内容 |
| --- | --- |
| [01-requirements.md](docs/01-requirements.md) | 背景 / 目的 / スコープ / ユースケース / 非機能要件 |
| [02-ui-spec.md](docs/02-ui-spec.md) | 画面レイアウト / キーバインド / 状態遷移 / 配色 |
| [03-architecture.md](docs/03-architecture.md) | モジュール構成 / 状態モデル / 並行処理設計 / 設定 |
| [04-diff-model.md](docs/04-diff-model.md) | 全文表示型 side-by-side diff の行整列アルゴリズム |
| [05-tech-research.md](docs/05-tech-research.md) | クレート選定調査 / バージョン / trade-off / 出典 |
| [06-implementation-plan.md](docs/06-implementation-plan.md) | 進捗 / マイルストーン / 受け入れ基準 |

## 前提合意事項

- diff 対象は **作業ツリー vs HEAD** (未コミット変更)。staged / unstaged は統合して扱う
- Git バックエンドは **gix (gitoxide)**。C ツールチェーン非依存で `cargo build` のみでビルドできる
- リモート監視はアプリ側で扱わない。各接続先ホストにインストールし、オーケストレータのセッション内で起動する

## 未確定事項

- v1 のスコープを「閲覧専用」と仮置きしている。stage/unstage 等の書き込み操作、外部エディタ連携の要否は未確定
- バイナリ名を `tdv` と仮置き

## 開発

```sh
cargo test                 # 単体 46 + 統合 13
cargo clippy --all-targets
nix build                  # サンドボックス内でテストごとビルド
```

MSRV は 1.88 (let-chains を使うため)。統合テストは一時ディレクトリに実 Git リポジトリを作り、`git` コマンドの結果と照合する。
