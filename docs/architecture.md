# アーキテクチャ

```text
read-only JSONL → bounded BufReader → normalized events → SQLite transactions
                                                 ↓ replay by session/turn
                                       projects / sessions / turns / intervals
                                                 ↓ UTC + IANA zone aggregation
                                      Tauri commands/events → React calendars
```

RustコアはTauriから分離しています。`parser` は1行の正規化、`store` はマイグレーション・増分走査・重複除去・ターン再構築、`activity` はUTC区間の和集合と日別分割を担当します。`desktop` はアプリ保存先、IPC、監視を担当します。`--no-default-features` でWebViewなしのコアをテストできます。

ReactはIPCから受け取ったデータを安全なテキストとして描画します。日付移動・フィルタ・重複配置・表示期間にクリップした集計を `calendar.ts` に分離しています。バックエンド集計とUI計算の両方に日跨ぎ・重複テストがあります。フロントエンドから任意パスの読み取りやコマンド実行はできません。ログ参照はDBに記録済みのターンIDから出典を解決します。

読み込みは `spawn_blocking` でUIスレッドから外します。独立した4ファイルまでRayonで並行解析し、DBは単一Mutexとトランザクションで直列化します。監視はnotify、600ms debounce、更新が続く場合は5秒以内、30秒の再走査で補完します。進捗と更新はローカルTauriイベントです。

## ライブラリ選定

2026-11-21の実環境でnpmレジストリとCargoの解決結果を確認し、安定リリースをロックファイルに固定しました。React 19系、Vite 8系、Tailwind 4系、Vitest 5系、Tauri 2系、rusqlite 0.38、chrono / chrono-tz、notify 8、Luxon 3を採用。正確なバージョンは package-lock.json / Cargo.lock を正とします。Node 24とRust 1.99.1 MSVCでビルドを検証しています。

FullCalendar Standardの非Premiumプラグインは [公式ライセンス](https://fullcalendar.io/license) でMITと確認しました。今回は編集やドラッグが不要であり、未知終了のマーカー・日ごとのDST目盛り・全重複区間の表示を小さな依存で制御するため、React/CSS Gridの代替カレンダーを採用しました。FullCalendarのtimeGridWeek自体の実装検証は採用外のため行っていません。

## プライバシー

ネットワーク通信プラグイン、ファイルシステムプラグイン、シェルプラグインは導入していません。Tauri権限はローカルイベント購読/解除のみです。CSPは自身のアセットとIPCに限定しています。原本は `File::open` の読み取りのみ。DBは依頼文（上限16Ki文字）・必要なイベント・オフセットを保持し、ツール引数・出力・推論本文を保存しません。DBも秘密を含み得るローカルデータです。

## 次の改善

現在は同期時に正規化キャッシュから派生テーブルを再構築します。巨大履歴にはセッション単位の再構築と期間限定IPCを追加できます。OS固有の保存場所と監視処理はdesktop層に閉じ、パーサーはPath/UTCを使用します。

## ローカル保存先

端末固有ログの派生DBには `app_local_data_dir()` を使用します。WindowsはAppData/Local、macOSはLibrary/Application Support、LinuxはXDG_DATA_HOME（既定 .local/share）です。Windowsの旧Roaming保存先からは、LocalにDBがない場合だけSQLite backup APIでWALを含む整合したコピーを作り、上書き禁止の確定処理を使います。旧DBは削除しません。CODECAL_DATA_DIRを明示した場合は指定先だけを使い、通常プロファイルから移行しません。
