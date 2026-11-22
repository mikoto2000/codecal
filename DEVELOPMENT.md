# 開発ガイド

CodeCalはReact / TypeScript / Vite / Tailwind CSSと、Tauri 2 / Rust / SQLiteで構成するWindows・macOS・Linux向けアプリです。ログの外部送信、テレメトリー、任意コマンド実行機能はありません。利用手順は [README](README.md) を参照してください。

## 開発環境と起動

Node.js 24.22.1、npm 11.19.1、Rust 1.99.1と、お使いのOS向けのTauriビルド環境を用意します。
Node.jsは `.nvmrc`、npmは `package.json` の `packageManager`、Rustは `rust-toolchain.toml` に検証済みバージョンを記録しています。rustupは作業ディレクトリの設定ファイルからRustとrustfmt/clippyを選択します。

```sh
nvm install
nvm use
npm install --global npm@11.19.1
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy
rustup show
```

`nvm` は利用している場合だけ実行してください。Windows版nvmでは `.nvmrc` の値を引数に渡すか、Node.js 24.22.1を直接導入できます。ツールチェーンを更新するときは、上記ファイル、`package.json` の `engines`、CI・リリース両ワークフローの固定値とこのガイドを同じPRで更新し、全OSのCIを確認してください。リリースワークフローは固定値を使うため、これらのファイルを持たない既存タグの手動ビルドでも同じツールを導入します。

- Windows：Rust MSVC、Visual Studio C++ Build Tools、Windows SDK、WebView2。
- macOS：Xcode Command Line ToolsなどのTauri開発要件。
- Linux：C/C++ビルドツール、WebKitGTK 4.1、GTKおよび関連ライブラリ。CIで導入する具体的なパッケージは [.github/workflows/ci.yml](.github/workflows/ci.yml) を参照してください。

OSごとの準備は [Tauri公式の前提条件](https://v2.tauri.app/start/prerequisites/) を参照してください。GUIの起動・操作はWindows 11 x64で検証しています。他OS・CPUのビルド・テストはCIで実行する構成ですが、GUIの起動・操作の検証とは別です。

```sh
npm ci
npm run tauri dev
```

`npm run dev` はフロントエンドだけを `127.0.0.1:1420` で起動します。ログ取り込みにはTauri IPCが必要なので、通常のブラウザーだけでは実データを読み込めません。

## テストとビルド

```sh
npm run typecheck
npm run lint
npm run format:check
npm test
npm run test:release
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --locked
npm run build
npm run tauri build
```

以下はTauriのローカル出力名です。GitHub Releaseへの添付時は `scripts/release.mjs` がOS・CPUを含む名前に改名します。Windowsの既定ビルドでは `src-tauri/target/release/codecal.exe` と `src-tauri/target/release/bundle/nsis/CodeCal_1.0.0_x64-setup.exe` を生成します。インストールせず実行ファイルを直接起動することもできます。macOS・Linux向けのバンドル指定と全6構成のビルド方法は [リリース手順](docs/releasing.md) を参照してください。

Rustコアのみは、デスクトップ機能を外して検証できます。

```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --locked
```

### WindowsネイティブE2E

```sh
npm run tauri build
npm run test:e2e
```

E2EはWindowsの実行ファイルとWebView2に直接接続します。ビルドを先に実行し、9223番ポートを空けてください。他のネイティブE2Eと同時実行しないでください。macOS・LinuxのGUI E2Eは未実装です。詳細は [テストガイド](docs/testing.md) を参照してください。

## 合成ログとデータの隔離

```sh
node scripts/create-sample.mjs
```

設定で `.local/sample-codex` の絶対パスを指定すると合成ログを試せます。再実行時は既存ファイルを上書きせずにスキップし、作成件数とスキップ件数を表示します。不足しているサンプルだけを追加できます。

`CODEX_HOME` を合成ログディレクトリ、`CODECAL_DATA_DIR` を空の専用保存ディレクトリに指定すると、通常の履歴から独立してテストできます。`CODECAL_DATA_DIR` はDBとウィンドウ状態の保存先を変更します。実ログのプロンプト・ツール出力・DBはGitへ追加しません。`.local/` と `*.db` はignore対象です。

通常のDBはTauriのユーザー別ローカルアプリデータディレクトリに保存します。Windowsで旧Roaming DBから移行する際は、SQLite backupでコミット済みWALを含む整合したスナップショットを作り、既存Local DBを上書きせず公開します。元のDBは削除しません。

## 開発の進め方

振る舞いを小さく分け、失敗テスト → 失敗理由確認 → 最小実装 → 全テスト → リファクタリング → 再テスト → Conventional Commitの順に進めます。実装済みのテストが落ちた場合は原因を修正し、削除や無効化で回避しません。

開発時にはparserの未認識イベント、区間計算の空実装、storeの空スナップショット、calendar関数の空実装、UIの空コンポーネントでRedを確認しています。追加ケースとして、開始前の依頼文の対応付け、未来スキーマ拒否、同サイズのログ書換えも失敗を確認して修正しました。E2Eではイベント購読権限不足を追記の自動反映テストが検出しました。

ログ形式を拡張するときは、値の入っていないキー構造だけを調査記録に残し、匿名化した最小サンプルでテストを追加してください。

## リポジトリ構成

| パス                        | 内容                                                  |
| --------------------------- | ----------------------------------------------------- |
| `src-tauri/src`             | parser / activity / store / desktop                   |
| `src-tauri/tests`           | 合成ログを使ったRust結合テスト                        |
| `src`                       | React UI、カレンダー計算、型付きIPCアダプター、Vitest |
| `tests/e2e`                 | Windows WebView2に直接接続するPlaywrightテスト        |
| `scripts/create-sample.mjs` | 機密を含まないデモログ生成                            |
| `scripts/release.mjs`       | タグ・バージョン・成果物の検証                        |
| `docs`                      | 設計、計算定義、ログ調査、検証結果                    |

`src-tauri/gen/` のスキーマはTauriが生成するためGitでは追跡しません。初回は `cargo check --manifest-path src-tauri/Cargo.toml --locked` または `npm run tauri build` で生成できます。生成ファイルを直接編集せず、設定やcapabilitiesを変更して再生成してください。

## CIとリリース

[通常CI](.github/workflows/ci.yml) はpushとpull requestで動きます。

- Windows x64：型、lint、format、Vitest、リリース補助処理のテスト、Rust fmt/clippy/test、Vite・Tauriビルド、ネイティブE2E。
- Linux・macOSのx64 / ARM64：型、lint、format、Vitest、リリース補助処理のテスト、デスクトップ機能を含むRust fmt/clippy/test、Viteビルド。

[リリースワークフロー](.github/workflows/release.yml) は `v*` タグのpush、または既存タグを指定した手動起動で実行し、Windows・macOS・Linuxのx64 / ARM64向け成果物を生成します。公開、署名の扱い、失敗時の復旧は [リリース手順](docs/releasing.md) を参照してください。

ロックファイルには公開レジストリのURLを使用し、ローカルミラーにしか到達できないURLを含めないでください。テキストの改行は `.gitattributes` でLFに統一しています。

## 集計・取り込みの仕様と制限

- 開始・終了イベントが対応するターンだけを集計します。UTCで保存し、指定タイムゾーンの日付境界で日別に按分します。延べ時間はターンの合計、実稼働時間帯は区間の和集合です。
- 未終了ログから「実行中」と「異常終了」を区別せず、終了不明とします。終了イベントしかないターンは開始時刻を推測して作成しません。旧形式のセッション全体トークン累積値からターン使用量を推測しません。
- 1MiBを超える行は診断してスキップし、末尾の改行前の行は次回の同期まで待機します。依頼文の保存は16Ki文字まで、元ログの該当箇所の表示は最大64KiBです。
- 元ファイルの読み取りは増分ですが、同期後の派生テーブルは正規化イベントから再構築します。非常に長い履歴の再集計・全ターンのUI転送には今後ページングが必要です。
- ファイル識別には作成時刻、更新時刻、長さ、先頭最大4KiBのハッシュを使います。作成時刻・先頭を維持して既存の中間部分を書き換え、同時に追記する特殊な変更は検出できない場合があります。通常の追記・置換・切り詰め・移動は対応しています。
- 自動更新はファイル監視と30秒の補完走査を使います。日別インデックスはキャッシュし、ビュー移動では履歴全体を再計算しません。
- まとめ表示は同じプロジェクト・同じ日・間隔5分以内を単位とし、元のターンを保持します。親の依頼とサブエージェントの対応による結合は未実装です。
- 夏時間の日は実経過時間に合わせて列内の時刻目盛りを個別表示します。通常日の共通時刻軸とは位置が異なります。
- Windows x64で起動とネイティブE2Eを検証しています。クリーンな別端末へのインストール、他OS・CPUのGUI起動・操作は未検証です。ビルドと実行の検証範囲は区別して記録してください。

## 設計・検証資料

- [アーキテクチャ](docs/architecture.md)
- [Codexログ形式](docs/codex-log-format.md)
- [稼働時間の計算](docs/activity-calculation.md)
- [データベース](docs/database-schema.md)
- [テスト・検証](docs/testing.md)
- [初期開発の検証記録](docs/delivery-report.md)
- [れいの実行履歴](docs/rei-history.md)
- [リリース手順](docs/releasing.md)

## Claude Code ログ

Claude Code の対応内容、時間推定、SDK調査、安全性の判断は [調査記録](docs/claude-logs.md) を参照してください。**実環境での動作未検証** です。

実ログと隔離する場合は `CLAUDE_CONFIG_DIR` を専用の空ディレクトリに向けてから起動します。その `projects` がなければ既定で無効です。ネイティブE2Eも専用パスだけを参照します。

## DBの自動マイグレーション

`src-tauri/src/migrations.rs` が既知スキーマの起動時移行を管理します。v0→v1の基本テーブル、v1→v2の取得元・ネイティブIDとれいのカーソル、v2→v3の互換性検証を単一のIMMEDIATEトランザクションで処理します。既存カラムを確認するため、古い実行ファイルがバージョン番号だけを書き戻したDBも重複カラムエラーを起こしません。書き込みロック取得後にもバージョンを再確認します。

今回確認した旧DBはv3で、v2の共通テーブルに `claude_files(path,fingerprint)` が追加された形式でした。このキャッシュは任意の互換テーブルとして全レコードを保持します。旧Claudeイベントが明示的な開始・終了形式で保存されている場合は、その境界をそのまま使い、メッセージ間隔による再推定を行いません。既知のv3を下位バージョンへ書き換える処理はありません。未対応の将来バージョンや、列構成が異なる既存DBは変更せず拒否します。

移行前バックアップはSQLite backup APIでWALを含む整合したスナップショットを作り、一時ファイルから上書きなしで公開します。バックアップ失敗時は移行しません。新規DBと `:memory:` はバックアップ不要です。自動移行に失敗しても元DBを削除・作り直すことはありません。`tests/migrations.rs` は合成DBでデータ保持・v3互換・v0/WAL・再実行・失敗時のロールバック・バックアップ異常・将来スキーマ保護を検証し、ネイティブE2Eでもv2と既知v3からの起動を確認します。

## DB準備中の画面

CodeCal はウィンドウとイベント購読を先に準備し、フロントエンドが起動画面を描画した後、`bootstrap` の `spawn_blocking` ワーカーでDB準備・移行を開始します。`Store::open_with_progress` はバックアップ前に移行開始を通知し、UIは `startup-progress` を受けて「CodeCal がデータベースを移行しています…」とスピナーを表示します。既知の現行DBを確認するだけの場合、CodeCal は「CodeCal がデータベースを確認しています…」と表示します。Coreは通知コールバックだけを持ち、TauriやUIに依存しません。

起動処理のMutexはStrictModeや同時呼び出しによる二重移行を防ぎ、ウィンドウ状態プラグインは従来どおりウィンドウ生成前に登録します。DB準備前の監視処理は取り込みを開始しません。DB準備に失敗した場合、CodeCal は画面にエラーと再試行を表示し、成功済みのDB接続は再利用します。

ネイティブE2Eは合成DBの書き込みロックを保持し、移行待ちの間にもウィンドウ・主語付きメッセージ・スピナーが表示され、animation frameが進むことを確認します。ロック解除後のカレンダー表示、将来スキーマエラーから修復後の再試行、既存のウィンドウ復元も検証します。製品コードにテスト用の待機時間は入れません。
