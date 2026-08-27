# terminal-diff-viewer (tdv)

ターミナル上でディレクトリツリーと git 差分をエディタ的に閲覧する TUI アプリケーション。

差分表示中も **ファイル全体を読める** ことが他の diff ビューアとの違い。変更箇所の前後数行に視野が限定されないため、変更の妥当性を周辺コードから判断できる。

## 使い方

```sh
cargo run -- [PATH]         # tree モードで起動
cargo run -- [PATH] --diff  # diff モードで起動
cargo build --release       # target/release/tdv
```

`PATH` を省略するとカレントディレクトリを対象にする。Git リポジトリ外でも tree モードは動作する。

主なキー: `m` モード切替 / `Tab` ペイン移動 / `j` `k` 移動 / `Enter` 開く / `]c` `[c` 変更箇所ジャンプ / `]f` `[f` 変更ファイル移動 / `z` 折り畳み / `u` unified 表示 / `r` リロード / `?` ヘルプ / `q` 終了

## 実装状況

| 機能 | 状態 |
| --- | --- |
| tree モード (遅延展開・ignore 準拠・ステータス記号) | 実装済み |
| diff モード (作業ツリー vs HEAD、全文 side-by-side) | 実装済み |
| 語単位ハイライト / 変更箇所ジャンプ / 折り畳み / unified 切替 | 実装済み |
| バイナリ・巨大ファイル・CRLF・末尾改行欠落の扱い | 実装済み |
| シンタックスハイライト | 未着手 |
| 検索 (ファイル名 / 内容) | 未着手 |
| 設定ファイル | 未着手 (既定値と CLI 引数のみ) |

詳細は [docs/06-implementation-plan.md](docs/06-implementation-plan.md) の進捗表を参照。

## ドキュメント

| ファイル | 内容 |
| --- | --- |
| [01-requirements.md](docs/01-requirements.md) | 背景 / 目的 / スコープ / ユースケース / 非機能要件 |
| [02-ui-spec.md](docs/02-ui-spec.md) | 画面レイアウト / キーバインド / 状態遷移 |
| [03-architecture.md](docs/03-architecture.md) | モジュール構成 / 状態モデル / 並行処理設計 |
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
cargo test     # 単体 30 + 統合 5
cargo clippy --all-targets
```

統合テストは一時ディレクトリに実 Git リポジトリを作り、`git` コマンドの結果と照合する。
