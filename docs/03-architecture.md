# 03. アーキテクチャ

## 1. 設計方針

| 方針 | 理由 |
| --- | --- |
| UI スレッドで I/O を行わない | Git 走査 / ファイル読み込み / 差分計算はいずれも数百 ms に達しうる。NFR-03 (16ms 応答) を守るため全てワーカーへ逃がす |
| 状態更新を単方向にする | 入力 → `Action` → 状態更新 → 描画、の一方向。描画関数は状態を書き換えない |
| Git 実装を trait で隔離する | gix は 1.0 未満で API が版ごとに変わる。将来の差し替えとバージョン追従のコストを局所化する |
| 差分の「対象指定」を型で抽象化する | v1 は作業ツリー vs HEAD のみだが、v2 の ref 間比較を分岐追加で吸収できるようにする |
| 表示用データを事前計算しない | ハイライトや語単位差分は可視範囲のみ計算し、結果をキャッシュする |

## 2. モジュール構成

```
src/
├── main.rs               エントリポイント、端末セットアップ / 復旧
├── cli.rs                コマンドライン引数 (clap)
├── config.rs             設定ファイル読み込み、既定値
├── app/
│   ├── mod.rs            イベントループ
│   ├── state.rs          AppState 定義
│   ├── action.rs         KeyEvent → Action への変換 (キーマップ)
│   └── update.rs         Action + TaskResult → 状態遷移
├── ui/
│   ├── mod.rs            draw エントリ
│   ├── layout.rs         レイアウト計算 (5:15 / 5:7.5:7.5、縮退規則)
│   ├── tree_pane.rs
│   ├── content_pane.rs   tree モードの単一表示
│   ├── diff_pane.rs      diff モードの左右対比表示
│   ├── status_bar.rs
│   ├── overlay.rs        ヘルプ / 検索入力
│   ├── theme.rs          配色定義、色数フォールバック
│   └── text.rs           表示幅計算、切り詰め、タブ展開
├── git/
│   ├── mod.rs            GitBackend trait、DiffSpec
│   ├── gix_backend.rs    gix による実装
│   └── model.rs          ChangeSet / FileChange / BlobContent
├── vfs/
│   ├── walker.rs         ignore クレートによるディレクトリ走査
│   └── tree.rs           TreeModel (ノード、展開状態、可視行の平坦化)
├── diff/
│   ├── mod.rs
│   ├── align.rs          行整列 (04-diff-model.md)
│   ├── inline.rs         語単位差分
│   └── model.rs          AlignedDiff / RowPair / Row
├── highlight/
│   └── mod.rs            syntect ラッパ。ワーカー側で全行を一括色付けする
└── task/
    ├── mod.rs            ワーカープール、リクエスト / 結果チャネル
    └── message.rs        TaskRequest / TaskResult
```

## 3. 状態モデル

```rust
pub struct AppState {
    pub repo: Option<RepoContext>,   // Git リポジトリ外でも tree モードは動く
    pub root: PathBuf,               // 表示ルート
    pub mode: Mode,                  // Tree | Diff
    pub focus: Focus,                // Tree | Content
    pub overlay: Overlay,            // None | Help | Search(Input) | Filter(Input)
    pub layout: LayoutState,         // 左ペイン比率、折り返し、side-by-side/unified
    pub fs_tree: TreeModel,          // tree モード用
    pub change_tree: TreeModel,      // diff モード用 (ChangeSet から構築)
    pub changes: ChangeSet,          // 変更ファイル一覧
    pub content: ContentState,       // 右ペインの内容
    pub search: SearchState,
    pub notice: Option<Notice>,      // 一時メッセージ (エラー含む)
    pub generation: Generation,      // 実行中タスクの世代番号
}

pub enum ContentState {
    Empty,
    Loading { path: PathBuf },
    Text(TextView),                  // tree モード
    Diff(DiffView),                  // diff モード
    Unsupported { path: PathBuf, reason: UnsupportedReason },
    Failed { path: PathBuf, error: String },
}

pub enum UnsupportedReason {
    Binary { size: u64 },
    TooLarge { size: u64, limit: u64 },
    InvalidUtf8,
}
```

`Unsupported` と `Failed` を分けているのは、前者が仕様上の正常な結果 (FR-13 / FR-14) であり、後者が異常であるため。表示の色と文言を変える。

### 3.1 TreeModel

ツリーは「ノードの木」と「画面に描画する平坦な行リスト」を分離して持つ。

```rust
pub struct TreeModel {
    nodes: Vec<Node>,          // アリーナ。親子は index 参照
    root: NodeId,
    visible: Vec<NodeId>,      // 展開状態を反映した描画順の平坦リスト
    selected: usize,           // visible 内の位置
    offset: usize,             // スクロール位置
    dirty: bool,               // visible の再構築が必要か
}
```

展開 / 折り畳み / フィルタ変更で `dirty` を立て、描画直前に `visible` を再構築する。描画は `visible[offset..offset+height]` のみを参照するため、ノード数に依存しない。

## 4. Git 抽象

```rust
pub trait GitBackend: Send + Sync {
    fn head(&self) -> Result<HeadInfo>;
    fn changes(&self, spec: &DiffSpec) -> Result<ChangeSet>;
    fn load(&self, side: Side, change: &FileChange) -> Result<BlobContent>;
}

/// 差分の比較対象。起動後にキーで切り替える。
pub enum DiffSpec {
    WorktreeVsHead,     // staged と unstaged を統合
    StagedVsHead,       // stage 済みの変更のみ
    WorktreeVsIndex,    // 未 stage の変更のみ
    Range { from: String, to: String },  // 任意 ref 間。解決はワーカー側
}

pub enum Side { Old, New }

pub struct FileChange {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,   // リネーム時のみ
    pub kind: ChangeKind,            // Added | Modified | Deleted | Renamed | Untracked
    pub old_id: Option<ObjectId>,    // HEAD 側のオブジェクト ID
    pub stat: Option<LineStat>,      // +n / -n。遅延計算
}

pub struct BlobContent {
    pub bytes: Arc<[u8]>,
    pub kind: ContentKind,           // Text { lines } | Binary | TooLarge
}
```

### 4.1 gix での実装方針

| 操作 | 実装 |
| --- | --- |
| リポジトリ探索 | `gix::discover(cwd)` |
| 変更一覧 | `Repository::status()` で得た Platform を反復し、`tree_index` と `index_worktree` の両方の項目をパス単位で統合する |
| HEAD 側の内容 | `Repository::head_tree()` からパスを辿り blob を取得。リネーム時は `old_path` を使う |
| index 側の内容 | `Repository::index()` の `entry_by_path` で blob の id を引き、`find_object` で読む |
| 作業ツリー側の内容 | ファイルシステムから直接読む |
| 未追跡ファイル | `index_worktree` の untracked 項目。HEAD 側は空として扱う |
| stage / unstage | index を `File::at` 相当で読み、エントリを差し替えて `File::write` で書き戻す。書き込みは gix が `index.lock` を取って行う |

`tree_index` (HEAD↔index) と `index_worktree` (index↔作業ツリー) を統合して 1 つの `ChangeKind` に落とし込む規則:

| tree_index | index_worktree | 統合結果 |
| --- | --- | --- |
| なし | Modified | Modified |
| Modified | なし | Modified |
| Modified | Modified | Modified |
| Addition | なし / Modified | Added |
| Addition | Removed | 変更なし扱い (一覧から除外) |
| Deletion | — | Deleted |
| なし | Removed | Deleted |
| — | Untracked | Untracked |
| Rewrite / Rename | * | Renamed (+ 内容差分) |

> gix の `status` は index を経由するため、「作業ツリー vs HEAD」は上表の統合で表現する。統合規則はテーブル駆動のユニットテストで固定している。

`StagedVsHead` は `tree_index` の観測だけ、`WorktreeVsIndex` は `index_worktree` の観測だけを見る (統合しない)。比較対象ごとに左右の取得元も変わる:

| 比較対象 | 旧側 | 新側 |
| --- | --- | --- |
| WorktreeVsHead | HEAD のツリー | 作業ツリー |
| StagedVsHead | HEAD のツリー | index |
| WorktreeVsIndex | index | 作業ツリー |
| Range { from, to } | from のツリー | to のツリー |

index を書き換える操作 (stage / unstage) は次の手順で行う。エントリを変えるとツリーキャッシュが古くなるため、書き戻す前に落とす。

1. index を所有権のあるコピーとして読む
2. 対象パスのエントリを stage を問わず取り除く
3. stage なら作業ツリーの内容を blob として書き、モードと stat を付けてエントリを積む。unstage なら HEAD のエントリを積む (HEAD に無ければ積まない)
4. `sort_entries` で並びを戻し、ツリーキャッシュを落として書き戻す

作業ツリーのバイトをそのまま blob にするため、`.gitattributes` の clean フィルタ (改行変換、Git LFS) は適用されない。

## 5. 並行処理

### 5.1 チャネル構成

```
 [入力スレッド]  crossterm::event::read()
        │ AppEvent::Input(KeyEvent | Resize)
        ▼
   ┌─────────────────┐        TaskRequest        ┌──────────────┐
   │  メインループ   │ ────────────────────────▶ │ ワーカープール│
   │ (状態更新+描画) │ ◀──────────────────────── │  (N スレッド)│
   └─────────────────┘   AppEvent::Task(result)  └──────────────┘
```

- 全イベントを単一の `AppEvent` チャネルへ集約し、メインループは 1 つの receiver をブロッキング受信する。select が不要になり、アイドル時の CPU 使用率が 0 になる
- ワーカーは `std::thread` ベース。非同期ランタイムは導入しない (I/O は全てブロッキングのファイル / メモリ操作であり、タスク数も少ない)
- ワーカー数は `available_parallelism()` を上限 4 で制限

### 5.2 タスク種別

| タスク | 契機 | 結果 |
| --- | --- | --- |
| `ScanStatus` | 起動時 / リロード / 比較対象の切り替え | `ChangeSet` |
| `ReadDir` | ディレクトリ展開 | 子ノード一覧 |
| `LoadText` | tree モードでファイル選択 | `BlobContent` |
| `ComputeDiff` | diff モードでファイル選択 | `AlignedDiff` |
| `Highlight` | 本文の表示後、可視範囲 → 全文の順 (差分は左右それぞれ) | 行ごとのスタイル |
| `Stage` | `a` / `U` の押下 | index 書き換えの成否 |

### 5.3 陳腐化の破棄

ユーザーがツリーを高速に移動すると、完了前のタスクが積み上がる。

- `AppState.generation` を持ち、選択ファイルが変わるたびに加算する
- `TaskRequest` に発行時の `generation` を載せ、結果受信時に現在値と一致しない場合は破棄する
- 加えて `AtomicBool` のキャンセルフラグを共有し、ワーカー側は差分計算のループ内で定期的に確認して早期離脱する

これにより、押しっぱなしのカーソル移動でも UI は詰まらない。

### 5.4 キャッシュ

| 対象 | キー | 破棄 |
| --- | --- | --- |
| HEAD 側 blob | `ObjectId` | LRU (既定 64 エントリ / 合計 32MB 上限) |
| 作業ツリー側の内容 | `(path, mtime, size)` | LRU + リロードで全破棄 |
| `AlignedDiff` | `(old_id, new_key)` | LRU (既定 16 エントリ) |
| ハイライト結果 | `(cache_key, 行範囲)` | 内容キャッシュに従属 |

## 6. 描画

- ratatui の `Terminal::draw` を、状態が変化したときのみ呼ぶ (`dirty` フラグ管理)
- 差分ペインは `AlignedDiff.rows` の可視スライスのみを `Line` へ変換する。10 万行のファイルでも 1 フレームの変換対象は端末高さ分に限られる
- 全角文字の桁揃えは `ui/text.rs` に集約する。`unicode-width` で表示幅を求め、切り詰めは幅基準で行う。East Asian Ambiguous 文字の扱いは設定で切り替える (既定は狭幅)
- タブ文字は表示幅 (既定 4) で空白展開してから幅計算する
- 行折り返しは 1 論理行を複数の画面行へ展開する。side-by-side では左右で折り返し行数が変わるため、少ない側を余白行で埋めてペア行の対応を保つ (`ui/diff_pane.rs`)

## 7. 端末の初期化と復旧

- raw mode + alternate screen への出入りは `ratatui::try_init()` / `ratatui::restore()` を使う。`try_init()` はパニックフックの設定も行うため、パニック時にも端末が復旧する
- raw mode 中は `Ctrl-c` がキーイベントとして届くため、通常終了と同じ経路を通る。`SIGTERM` を受けた場合の復旧はシグナルハンドラが必要であり、v1 では未対応 (端末が壊れた状態で残りうる)

## 8. エラー処理

- ドメイン層 (`git` / `diff` / `vfs`) は `thiserror` で型付きエラーを返す
- アプリ層は `anyhow` で文脈を付与する
- ファイル単位のエラーはアプリを終了させず、`ContentState::Failed` としてペイン内に表示し、原因文字列を残す
- 起動時のエラー (リポジトリ不在、パス不正) は端末初期化前に標準エラーへ出力して終了する

## 9. 設定

`$XDG_CONFIG_HOME/tdv/config.toml` (未設定なら `~/.config/tdv/config.toml`) を読む。存在しない場合は既定値で動作する。`--config <PATH>` で明示指定でき、その場合はファイルが無ければエラーにする。

未知のキーや範囲外の値は起動前にエラーとして報告する (端末初期化より前に標準エラーへ出す)。設定を黙って無視すると、効いていない理由が分からなくなるため。

```toml
[ui]
tree_ratio = 5              # 左ペインの比率 (全体を 20 とした値。右は 20 - tree_ratio)。1〜16
show_status_bar = true
tab_width = 4
ambiguous_width_wide = false
syntax_highlight = true
max_highlight_lines = 20000 # これを超える行数は色付けしない

[diff]
full_file = true            # 既定で全行表示
fold_context = 3            # 折り畳み時の前後行数
inline_words = true         # 語単位ハイライト
max_file_bytes = 2097152    # これを超えると差分計算しない

[theme]
palette = "red-green"       # または "blue-orange"
syntax = "tdv-dark"         # tdv-light や syntect 付属のテーマも指定できる
```

キーマップの上書きは設けない ([01](01-requirements.md) §4.3)。利用者が単独で既定のキーで足りるため、`[keys]` は追加しない。
