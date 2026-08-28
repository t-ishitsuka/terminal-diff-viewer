# 05. 技術調査

調査日: 2026-08-27。バージョンは調査時点の crates.io 公開値。

## 1. クレート選定一覧

| 用途 | 採用 | バージョン | 公開日 |
| --- | --- | --- | --- |
| TUI フレームワーク | `ratatui` | 0.30.2 | 2026-06-19 |
| 端末バックエンド | `crossterm` | 0.29.0 | 2025-04-05 |
| Git 操作 | `gix` | 0.87.1 | 2026-08-24 |
| 差分アルゴリズム | `gix-imara-diff` (gix 経由、直接依存には入れない) | 0.2.5 | — |
| ディレクトリ走査 / ignore | `ignore` | 0.4.33 | 2026-08-04 |
| シンタックスハイライト | `syntect` (pure Rust 構成) | 5.3.0 | 2025-09-27 |
| 設定ファイル | `toml` / `serde` | 1.1 / 1.0 | — |
| 表示幅計算 | `unicode-width` | 0.2 系 | — |
| CLI 引数 | `clap` (derive) | 4 系 | — |
| エラー | `thiserror` / `anyhow` | 2 系 / 1 系 | — |
| キャッシュ | `lru` | 0.12 系 | — |
| ファイル監視 (v2) | `notify` | 8.2.0 (安定版) | 2025-08-03 |

バージョン欄が「系」表記のものは調査時点で個別確認していない。実装着手時に `cargo add` の解決結果で確定する。

## 2. TUI: ratatui

**採用理由**: Rust の TUI ライブラリとして事実上の標準。イミディエイトモード描画で状態管理を自分で持つ設計であり、[03-architecture.md](03-architecture.md) の単方向データフローと相性が良い。

**0.30 系で把握しておくべき破壊的変更**:

| 変更 | 影響 |
| --- | --- |
| MSRV が 1.86.0 へ | Nix の Rust ツールチェーンを 1.86 以上に固定する必要がある |
| `Alignment` → `HorizontalAlignment` へ改名 | 命名のみ |
| `WidgetRef` の blanket 実装が削除。参照に対して `Widget` を実装する方式へ | 自作ウィジェット (差分ペイン) の実装方針に影響 |
| `Backend` trait に関連 `Error` 型と `clear_region` が追加 | 独自バックエンドを作らないため影響なし |
| crossterm のバージョンを feature flag (`crossterm_0_28` / `crossterm_0_29`) で選択 | 既定は最新版。`crossterm` を直接依存に入れる場合はバージョンを揃える |
| `Layout::try_areas` / `Rect::layout_vec` 追加 | レイアウト計算で利用 |

**自作ウィジェットの必要性**: 差分ペインは行番号列・マーカー列・語単位ハイライトを持ち、左右で行を対応させる。標準の `Paragraph` では表現できないため、`ui/diff_pane.rs` に独自ウィジェットを実装する。可視範囲のみ `Line` を組み立てる方式にする ([03](03-architecture.md) §6)。

## 3. Git: gix

### 3.1 選択肢の比較

| 観点 | gix (gitoxide) | git2 (libgit2) | git CLI ラップ |
| --- | --- | --- | --- |
| 依存 | pure Rust | C ライブラリ (libgit2) のビルドが必要 | 外部 `git` 実行ファイル |
| Nix 環境との相性 | 良好。`cargo build` のみで完結 | `pkg-config` / システムライブラリの解決が必要 | git のバージョン差で出力が変わる |
| ビルド時間 | 依存クレート数は多いが Rust のみ | libgit2 のコンパイルが加算 | ほぼゼロ |
| API 安定性 | **1.0 未満。版ごとに破壊的変更あり** | 安定 | 安定 (出力形式は非保証) |
| 性能 | status 走査は並列化されており高速 | 十分 | プロセス起動コストが毎回発生 |
| 出力の忠実さ | git 準拠を目標として実装 | git 準拠 | git そのもの |

**採用: gix**。NFR-06 (C ツールチェーン非依存) を最優先とし、Nix 3 台への配布を単純化する。

**リスクと緩和**: gix は 1.0 未満で API が版ごとに変わる。`Cargo.toml` でバージョンを固定 (`=0.87.1`) し、`Cargo.lock` をコミットする。加えて [03-architecture.md](03-architecture.md) §4 の `GitBackend` trait で隔離し、追従時の修正範囲を `gix_backend.rs` 1 ファイルに閉じる。この trait は git2 / CLI 実装への差し替え経路でもある。

### 3.2 gix の実装状況で確認した点

公式の `crate-status.md` から:

- `gix-status`: index ↔ 作業ツリーの差分 (リネーム追跡・未追跡ファイル検出を含む)、index ↔ index の差分は実装済み。sparse-index / split-index の高速化と fsmonitor は未実装
- `gix-diff`: tree ↔ tree、tree/index ↔ 作業ツリーの差分は実装済み。リネーム追跡も対応。**テキスト / バイナリのパッチ生成は未完**
- `gix` (トップレベル): リポジトリ探索、rev-parse、rev-walk は実装済み。checkout / merge / rebase 等のワークフロー系は未完

**判断**: v1 が必要とするのは「変更ファイル一覧」と「blob 内容の取得」だけであり、いずれも実装済みの範囲に収まる。パッチ生成が未完である点は問題にならない。本アプリは unified patch を経由せず、blob 内容から自前で行整列を行う ([04-diff-model.md](04-diff-model.md)) ためで、むしろ全文表示の要件に対して都合が良い。

未実装のワークフロー系機能は、v1 の非スコープ (stage / commit 等) と一致している。v2 で書き込み操作を入れる場合は、その時点で gix の実装状況を再確認する必要がある。

### 3.3 使用する API (実装で確定)

gix 0.87.1 のソースを直接確認し、以下を使用する。

- `gix::discover(path) -> Repository` — リポジトリ探索。`Repository::into_sync()` で `ThreadSafeRepository` へ変換し、ワーカースレッドでは `to_thread_local()` で開き直す
- `Repository::status<P>(&self, progress: P) -> Result<status::Platform<P>, status::Error>` — **引数は index ではなく progress**。`gix::progress::Discard` を渡す
- `status::Platform::untracked_files(UntrackedFiles::Files)` — 未追跡ファイルをディレクトリに畳まず個別に列挙する
- `status::Platform::into_iter(None) -> status::Iter` — `Item::TreeIndex(gix::diff::index::ChangeRef)` と `Item::IndexWorktree(status::index_worktree::Item)` が任意の順で流れてくる
- `Repository::head_tree() -> Tree`、`Tree::peel_to_entry_by_path(&mut self, path)` — HEAD 側 blob の取得
- `Repository::workdir()` — 作業ツリーのパス (旧称 `work_dir()` も残っている)

feature flag は実測で以下に確定した。`extras` / `default` は不要な機能まで有効化するため使わない。

```toml
gix = { version = "=0.87.1", default-features = false, features = [
  "status", "blob-diff", "dirwalk", "revision", "parallel", "max-performance-safe", "sha1",
] }
```

`max-performance` (zlib-ng) ではなく `max-performance-safe` を選ぶことで C 依存を避けている。この構成で依存クレートは 112、クリーンビルドは約 14 秒 (実測)。

## 4. 差分アルゴリズム: imara-diff

**採用理由**: gix が内部で使用しており、gix 経由で同一バージョンを利用できる。Histogram アルゴリズムは Myers より 10〜100% 高速と公称されており、git の `--histogram` と同系統で差分の見た目も git に近い。

**確認した API (0.2 系)**:

```rust
let input = InternedInput::new(before, after);           // 行単位で intern
let mut diff = Diff::compute(Algorithm::Histogram, &input);
diff.postprocess_lines(&input);                          // インデント基準のスライダー調整
for hunk in diff.hunks() {                               // hunks(&self) -> HunkIter
    // hunk.before: Range<u32>, hunk.after: Range<u32>
}
diff.count_additions();  // -> u32
diff.count_removals();   // -> u32
diff.is_removed(idx);    // トークン単位の判定。語単位差分で使用
```

`postprocess_lines` は差分の位置を可読な境界へ寄せる処理で、`}` だけの行がずれて表示される類の問題を減らす。適用を既定とする。

**依存の入れ方 (実装で確定)**: gix が使うのは crates.io の `imara-diff` ではなく、gitoxide 側でメンテされるフォーク **`gix-imara-diff` 0.2.5** である。`gix::diff::blob` が `pub use imara_diff::*` で全体を再エクスポートしているため、`gix::diff::blob::{Algorithm, Diff, InternedInput, Token, sources}` として利用できる。`imara-diff` を直接依存に加えるとバージョンが分かれるため追加しない。

本アプリは行を自前で分割する (`LineTable`) ため、`sources::byte_lines` ではなく `InternedInput::default()` + `update_before` / `update_after` に自前のイテレータを渡している。`&[u8]` は `Eq + Hash + AsRef<[u8]>` を満たすため、そのままトークン型に使える。

**代替案**: `similar` クレートは API が扱いやすく、語単位差分やグルーピングのユーティリティを内蔵する。ただし本アプリは gix を採用済みで imara-diff が推移依存に入るため、差分ライブラリを 2 つ抱えることになる。採用しない。

## 5. シンタックスハイライト: syntect

**採用理由**: Sublime Text の構文定義を使い、対応言語が広い。ハイライトの粒度が行単位で扱えるため、TUI の行ベース描画と噛み合う。公称性能は「最も複雑な構文とテーマでも 100ms 未満」。

**ビルド構成**: 既定の `onig` (C の Oniguruma) ではなく、pure Rust の `fancy-regex` バックエンドを選ぶ。NFR-06 を満たすため。デフォルト機能を切って必要な機能のみ有効化する。

**実装上の注意 (重要)**: syntect の `HighlightLines` は先頭行から順に状態を送る設計であり、途中行から再開できない。全文表示のため 10 万行のファイルを開く可能性がある本アプリでは、可視範囲だけをハイライトする方法を考える必要がある。

当初は「一定行数ごとに `ParseState` / `HighlightState` のスナップショットを保存し、可視範囲を直近のスナップショットから再生する」方針を検討した。**実装ではこれを採らず、ワーカースレッドでファイル全体を一括して色付けし、上限行数 (既定 20000 行、`ui.max_highlight_lines`) を超えるものは色付けしない方式にした。**

理由:

- パーサは先頭から順に走らせる必要があるため、遅延処理にしても総計算量は変わらない。前倒しにするだけで済む
- 色付けはワーカー側で行うため、結果が届くまでは素のテキストが表示され、UI は待たされない
- スナップショット方式は状態管理が増え、可視範囲の変化ごとに再生が必要になる。上限行数で保護する方が単純で、挙動も読みやすい

上限を超えるファイルは色付けなしで全文表示される。差分の閲覧そのものは妨げない。

**本文と色付けを別タスクにする**: 当初は `LoadText` / `ComputeDiff` の中で色付けまで済ませていた。5000 行のファイルを実測すると、差分計算 1.4ms に対して色付けが左右 2 面で約 690ms を占め、NFR-02 (100ms) を大きく超えていた。色付けを本文表示後の別タスク (`Highlight`) へ分け、素のテキストを先に描画して色を後から重ねる。分離後は差分描画 1.8ms、色付け完了 414ms (左右並列)。計測手順は [06](06-implementation-plan.md) §2.1。

**テーマ**: syntect 付属の base16 系テーマは色を割り当てるスコープが少なく、実際に走らせるとキーワードと文字列以外はほぼ既定色のまま残る。エディタで見慣れた密度にならないため、One Dark 相当のスコープ規則を持つテーマ (`tdv-dark` / `tdv-light`) を `syntect::highlighting::Theme` として直接組み立て、`ThemeSet` に登録している。

規則は syntect が実際に割り当てるスコープを列挙して決めた。特に `meta.generic` (ジェネリック引数の型名) と `storage.type.numeric` (プリミティブ型) はどの既定テーマも拾っておらず、Rust のコードで型名の多くが無色になっていた。

**代替案の検討**:

| 案 | 評価 |
| --- | --- |
| `tree-sitter-highlight` | 構文木ベースで精度が高いが、言語ごとに個別の grammar クレートが必要で、その多くが C コードを含む。NFR-06 に反し、対応言語を増やすほど依存が膨らむ。不採用 |
| ハイライトなし | 実装は最も軽いが、差分の可読性が大きく落ちる。不採用 |
| `syntect-tui` (syntect のスタイルを ratatui のスタイルへ変換する薄い層) | 変換を自前で書いても数十行のため、依存追加の要否は実装時に判断する |

構文定義とテーマはバイナリへ埋め込む (`default-syntaxes` 相当)。起動時のファイル読み込みを避け、NFR-01 に寄与する。

## 6. ディレクトリ走査: ignore

`.gitignore` / `.ignore` / `.git/info/exclude` およびグローバル gitignore を解釈する。ripgrep と同じ実装であり、除外規則の挙動が git と一致する。

**使い方**: `WalkBuilder` を使うが、`max_depth(1)` で 1 階層ずつ呼び出し、ツリーの遅延展開に合わせる。起動時に全階層を走査すると 1 万ファイル規模で NFR-01 を満たせない。

## 7. 表示幅

`unicode-width` で表示幅を求める。East Asian Ambiguous 文字 (`§`, `±`, 一部の罫線素片など) は端末とフォントの設定で幅が変わるため、`width()` と `width_cjk()` を設定で切り替えられるようにする (既定は狭幅)。

書記素クラスタ (絵文字の ZWJ 連結、結合文字) の扱いが必要なら `unicode-segmentation` の追加を検討する。v1 ではコード表示が主用途のため、必要になった時点で判断する。

## 8. ファイル監視 (v2)

`notify` の安定版は 8.2.0 (2025-08-03)。9.0 系は調査時点で RC (9.0.0-rc.4, 2026-05-02) であり、v1 では採用しない。v2 でファイル自動追従を入れる際に、その時点の安定版を確認する。

自動追従を入れる場合の注意: LLM の編集は短時間に大量のイベントを発生させるため、デバウンス (200〜500ms) が必須になる。また `.git/index` の変更も監視対象に含めないと、ステージ操作が反映されない。

## 9. 先行事例

| ツール | 特徴 | 本アプリとの差異 |
| --- | --- | --- |
| `delta` (git-delta) | git の pager。シンタックスハイライト付きの inline / side-by-side 表示 | pager であり、ファイルツリーと対話的なナビゲーションを持たない |
| `difftastic` | 構文木ベースの差分。整形のみの変更を無視できる | 出力は非対話。全文表示ではない |
| `diffnav` | delta にファイルツリーを付けた pager | 全文表示ではなく、pager 前提 |
| `ftdv` | ratatui 製。diffnav と lazygit に着想を得たツリー + 差分ビューア | 構成が近い。差分描画を外部ツール (delta / bat 等) に委譲する設計 |
| `gitui` | Rust 製の対話的 Git クライアント | Git 操作全般が対象。差分は変更箇所中心の表示 |

**本アプリの位置づけ**: 「ツリー + side-by-side 差分」という構成自体は先行事例があるが、**差分表示中にファイル全体を読める**点が差別化要素になる。差分描画を外部ツールに委譲せず自前で持つのは、全文整列とスクロール同期を成立させるため ([04-diff-model.md](04-diff-model.md))。

## 10. 検証状況

**実装で検証済み**:

- 各クレートの最新バージョンと公開日
- ratatui 0.30 の破壊的変更点と MSRV (rustc 1.98.0 でビルド確認)
- gix の feature flag の名称と最小構成 (§3.3)
- `Repository::status` の引数形 — docs.rs の要約は誤りで、実際は progress を取る
- gix が使う差分実装が `gix-imara-diff` フォークであること
- `gix::status` の Item 種別と、作業ツリー vs HEAD への統合規則 (統合テストで実リポジトリと照合)
- 依存 112 クレート、クリーンビルド約 14 秒

- syntect を `regex-fancy` 構成で組み込み、`onig` / `cc` が依存に入らないことを `Cargo.lock` で確認
- MSRV は 1.88 (let-chains の利用による)。`cargo +1.88.0 check --all-targets` で確認済み
- Nix パッケージ (`flake.nix`) がサンドボックス内でテストを含めてビルドできること

**未検証** (該当機能の実装時に要確認):

- NFR-01 / NFR-02 の性能目標の達成可否 (大規模リポジトリでの実測が必要)
- リリースビルドのバイナリサイズの妥当性

## 出典

- [ratatui - crates.io](https://crates.io/crates/ratatui) / [v0.30.0 highlights](https://ratatui.rs/highlights/v030/) / [BREAKING-CHANGES.md](https://github.com/ratatui/ratatui/blob/main/BREAKING-CHANGES.md)
- [gix - crates.io](https://crates.io/crates/gix) / [gitoxide crate-status.md](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md) / [gix::status docs](https://docs.rs/gix/latest/gix/status/index.html) / [gix::Repository docs](https://docs.rs/gix/latest/gix/struct.Repository.html) / [gix::diff::blob docs](https://docs.rs/gix/latest/gix/diff/blob/index.html)
- [imara-diff - crates.io](https://crates.io/crates/imara-diff) / [imara_diff::Diff docs](https://docs.rs/imara-diff/latest/imara_diff/struct.Diff.html)
- [crossterm - crates.io](https://crates.io/crates/crossterm)
- [syntect - crates.io](https://crates.io/crates/syntect) / [syntect GitHub](https://github.com/trishume/syntect/) / [syntect-tui - crates.io](https://crates.io/crates/syntect-tui)
- [ignore - crates.io](https://crates.io/crates/ignore)
- [notify versions - crates.io](https://crates.io/crates/notify/versions)
- [tree-sitter-highlight - lib.rs](https://lib.rs/crates/tree-sitter-highlight)
- [ftdv (File Tree Diff Viewer)](https://github.com/wtnqk/ftdv) / [awesome-diff-tools](https://github.com/mmueller2012/awesome-diff-tools) / [Terminal Trove: diff tools](https://terminaltrove.com/categories/diff/)
