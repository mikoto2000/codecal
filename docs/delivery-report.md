# CodeCal 実装・検証報告

2026-11-21、Windows 11 x64で実装・検証しました。

## 1. 実装機能

- Codexホームのsessions / archived_sessionsの読み取り専用JSONL取り込み
- セッション・ターン・cwdによるプロジェクト識別、依頼文とターン別トークン取得
- SQLite永続化、重複除去、追記・切り詰め・同サイズ置換・アーカイブ移動対応
- 不正JSON・UTF-8・欠損フィールド・未知イベントの診断と継続処理
- 週次カレンダー、重複区間の横並び、日付移動、現在時刻線、プロジェクト絞り込み
- 月次ヒートマップ、固定しきい値、日詳細、実時間/延べ時間切り替え、週表示への移動
- ターン詳細、ローカルソース参照、設定、プロジェクト名・色、ライト/ダーク/OSテーマ
- notify監視、debounce、30秒の補完走査、進捗と画面更新
- Windows Local保存、旧Roamingからの安全な自動移行、保存先上書き指定

## 2. アーキテクチャ

Tauri 2 / Rustが読み取り・SQLite・集計・監視を担当し、React / TypeScript / Vite / Tailwind CSSがカレンダーを表示します。Rustのparser / store / activity / storageはWebViewから独立しています。カレンダーはReact/CSS Grid、日付処理はLuxonとchrono-tz。フロントエンドに任意のファイル操作・ネットワーク・シェル権限は渡しません。

詳細: architecture.md / database-schema.md。

## 3. ログ調査の結果

実ログでsession_meta、turn_context、task_started、task_complete、turn_aborted、UserMessage、response_itemのメッセージ・ツールイベント、token_usage_recordを確認しました。正確なターン識別にはsession idとturn idを使います。ターン失敗互換イベントは未観測で、合成入力で対応します。再開やアーカイブ操作自体の挙動を観測したと主張せず、IDと正規化イベントの一意制約で再出現を許容します。

終了イベントなしでは現在実行中か異常停止か判定できません。終了不明として表示し、時刻を推測して確定集計しません。調査内容は codex-log-format.md に記録しました。秘密を含む実ログをテストやGitへ転記していません。

## 4. 稼働時間

開始・終了が対応するUTC半開区間を使います。延べ時間は区間長の合計、実稼働時間帯は重複区間の和集合です。プロジェクト別は延べ時間。指定IANAタイムゾーンの日付境界で按分し、夏時間の23時間・25時間の日も実ミリ秒で処理します。未確定ターンは時間集計から除外します。

詳細: activity-calculation.md。

## 5. テスト・品質チェック

| チェック                        | 結果                        |
| ------------------------------- | --------------------------- |
| Rust integration                | 18件成功                    |
| Vitest / React Testing Library  | 16件成功                    |
| Playwright native E2E           | 1シナリオ成功（最終13.0秒） |
| TypeScript型チェック            | 成功                        |
| ESLint                          | 成功                        |
| Prettier                        | 成功                        |
| Rust fmt                        | 成功                        |
| Rust clippy -D warnings         | 成功                        |
| Vite production build           | 成功                        |
| Tauri Windows x64 release build | 成功                        |

合計35件。ネイティブE2Eでは本物のWindows実行ファイルにWebView2 CDPで接続し、Rust IPC・SQLiteまで通しています。取り込み、週/月、詳細/ログ、追記自動反映、再起動、並列プロジェクトの新規検出（実4時間/延べ5時間）、テーマ保存を検証しました。通常のブラウザーをモックしたテストではありません。

実ログ検証は初回約403MB・189ファイルを約16.6秒で取り込みました。最終の通常プロファイル確認では190ファイル・15プロジェクト・2,343ターン・11日分を認識し、画面でも15プロジェクトのフィルタを確認しました。この作業自体もログへ追記されるため件数は変動します。

RoamingからLocalへの実DB移行も成功。旧DBの移行前後のSHA256が一致し、旧ファイルを保持したことを確認しました。合成テストではコミット済みWALの保持と既存Local DBの上書き防止を確認しています。

## 6. Windowsビルドと保存先

- 実行ファイル: src-tauri/target/release/codecal.exe
- インストーラー: src-tauri/target/release/bundle/nsis/CodeCal_0.1.0_x64-setup.exe
- 種別: Windows x64、NSIS、未署名、約2.84MiB
- インストーラー SHA256: `948370397E51C4B7B9879E128065F9CA4B7BF396539D58F203C748C59FFA9F75`
- DB既定: `%LOCALAPPDATA%/dev.codecal.desktop/codecal.db`
- 保存先上書き: `CODECAL_DATA_DIR`

macOSは `~/Library/Application Support/dev.codecal.desktop/codecal.db`、Linuxは `${XDG_DATA_HOME:-~/.local/share}/dev.codecal.desktop/codecal.db` が設計上の既定です。両OSの起動は未検証です。

## 7. ドキュメント・スクリーンショット

README.md、architecture.md、codex-log-format.md、activity-calculation.md、database-schema.md、development.md、testing.mdを整備しました。screenshots/week.png と month.png は合成ログを使った実Tauri画面です。

## 8. Git履歴

Conventional Commitsで、parser、重複時間集計、永続化・同期、カレンダー計算、UI、ネイティブ検証で判明した権限修正、同期の回帰修正、Local保存への移行、最終報告を分けて記録しました。機能のテストを先に作成してRedを確認し、小さなGreenとリファクタリングを重ねています。実行ログやDBはコミットしていません。

## 9. 制限・残る検証

- クリーンな別PCでのインストーラー導入、コード署名、Linux/macOSの起動・配布は未検証です。
- GitHub Actions設定は作成しましたが、リモートへのpushやGitHubホスト上の実行はしていません。
- 未終了ログの現在性は推測しません。連続ターンの結合表示は今後の改善です。
- 元ファイルは増分読み取りですが、派生テーブルは正規化キャッシュから再構築します。極端に大きい履歴にはセッション単位再構築と期間ページングが改善候補です。
- ファイルの作成時刻・先頭を維持しながら中間を書換え、同時に追記する特殊な変更は検出に限界があります。
- 歴史的な暦日全体の消失には未対応。通常のDSTはテスト済みです。
- 詳細の全制約と実行方法はREADME.md / testing.mdを参照してください。
