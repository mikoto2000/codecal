# Claude Code ログ対応の調査・設計記録

## 検証範囲

実環境での動作未検証。実際の個人ログ・認証情報は読まず、指定SDKの形式に合わせた合成JSONLで検証する。

## 既存機能の調査と統合

- `store.rs::sync_progress/read_delta` は Codex の `sessions` / `archived_sessions` をリンク追跡なしで探索する。1MiBの行上限、未完了末尾の保留、UTF-8検査、先頭ハッシュ・作成/更新時刻・長さによる差分カーソルを持つ。`sync_files` を抽出し、Claudeでも同じ保存・置換・重複排除処理を利用する。
- `parser::Event`、`sessions`、`turns`、`activity_intervals` を共用する。既存の `source` に `claude` を追加し、セッションIDは `claude:<UUID>`、子は `claude:<UUID>:agent-<id>`。最小の付加情報 `details` は既存のイベントJSONに保存し、DBスキーマの変更は不要。
- プロジェクトは既存の `normalize_path(cwd)` のSHA-256。Windowsの大文字小文字・区切りを統一する。worktreeをリポジトリ単位に統合する処理はもともとなく、Claudeもcwd単位で扱う。Git実行は不要。
- 既存の時間計算は明示的な開始・終了の対応が前提で、メッセージ間隔からの推定はない。このためClaudeの変換だけを `activity_events` に置き、5分以内の連続した活動を共通の開始・終了イベントへ変換する。ツールのないアシスタント応答は終点。セッション全体の経過時間を使わない。区間ごとの種別を作業内容として表示し、自由文の概要・プロンプトは抽出しない。
- `activity::aggregate` のタイムゾーン日付分割・和集合・延べ時間を共用する。親子や複数セッションを実時間に単純加算しない。延べ時間には既存仕様どおり並列区間も含む。
- 週は `Snapshot.turns` → `indexActivity` → `groupActivity/packSegments`、月は同じフロントのアクティビティ索引を使用する。既存UIを保ち、取得元と詳細メタデータ・設定を追加する。
- 設定に既定値付きのClaude有効フラグとprojectsパスを追加する。既存のファイル監視・30秒補完走査へ接続する。他取得元の読み取り失敗でもClaudeを続ける。
- テストはRust結合テスト、VitestのUI/計算テスト、Windows WebView2のPlaywright。新規はすべて合成データで、実ログやDBを追跡しない。

## 参考SDKの調査（移植前の評価）

対象: [NexA-LLC/claude-cli-sdk-rs](https://github.com/NexA-LLC/claude-cli-sdk-rs/tree/679be0382b880a46e9888a1f5876a263155523c1)、SHA `679be0382b880a46e9888a1f5876a263155523c1`。対象コードは `crates/claude-agent-rust-sdk/src/sessions/`。

| ファイル       | 確認した処理と採否                                                                                                                                                                                                           |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `disk.rs`      | `read_sessions_from_dir` / `list_all_sessions` のUUIDログ探索と `collect_agent_files` のsubagents構造を参考にした。全体読み込み・CLI呼び出しを伴う取得APIは移植しない。                                                      |
| `lite.rs`      | `read_session_lite`、`parse_session_info_from_lite` のcwd・gitBranch・時刻・概要の由来を確認。head/tail文字列走査と自由文抽出は移植せず、正規JSONの各行からメタデータだけ取得する。                                          |
| `chain.rs`     | `parse_transcript_entries`、user/assistantの分類、tool_use/tool_result、サブエージェントの親情報を確認。本文保持、会話の親UUIDチェーン再構築、IndexMapは不要。活動履歴では過去の枝も消さず読む。                             |
| `paths.rs`     | `config_home_dir` / `projects_dir` の環境変数優先順位を適用。逆変換できないプロジェクト名は推測せずcwdを使う。`worktree_paths` / `run_with_limit` は外部Git実行があるため移植しない。NFC変換やプロジェクト名ハッシュも不要。 |
| `timestamp.rs` | `parse_iso_millis` のUTCミリ秒表現を確認。手書き日時パーサーは移植せず既存chronoのRFC3339検証を使う。タイムゾーンのない日時は受け入れない。                                                                                  |
| `options.rs`   | 一覧/メッセージのページング、worktree、fork/importなどのオプションはSDK利用API用。CodeCalの設定とカーソルがあるため不要。                                                                                                    |

必要な既存依存はserde_json、chrono、walkdir、SQLite、sha2のみ。SDK全体・新規クレートは追加しない。上流のMIT/Apache二者択一からMITを採用し、原文のArthur JeanとAnthropicのMIT全文、出典SHA、変更点を `THIRD_PARTY_NOTICES` に保持した。対象6ファイルに追加のNexA著作権表示はなかった。

## セキュリティ評価と対策

- 上流のディスク解析経路の確認ではAPI通信・認証情報取得は不要。ただし `paths.rs` にはGitプロセス起動があるため取り込まない。CodeCalのClaudeリーダーはファイル読み取りだけを行い、CLI/API/MCP/設定変更/ログ変更/外部通信を含まない。ログの文字列はコマンドに渡さない。
- 上流の全体読み込み・再帰探索・リンク追跡可能な `is_dir/is_file` をそのまま使うとメモリや探索範囲の問題がある。CodeCalでは指定projects直下のプロジェクトとUUIDセッション、session/subagents配下のagentログだけを最大12階層で探索する。相対パスの親要素・不正ID・非絶対cwdを拒否し、cwdはファイルを開く用途に使わない。
- シンボリックリンク・Windows reparse point（junctionを含む）は探索せず診断する。読み取り前にもパスの祖先を検査する。意図的に別プロセスが検査とopenの間でディレクトリを差し替える攻撃への完全な原子性は標準ライブラリのパスAPIでは保証しない。管理下にあるローカルログディレクトリを対象とする。
- 行は1MiB、ファイルは256MiB上限。Claudeは1回のファイル読み取りを約8MiBまたは1万イベントで区切り、既存の最大4ファイルの並列処理を利用する。長い行の廃棄中もメモリは一定に保つ。巨大ファイルは診断してスキップし、未改行の巨大行はカーソルを進めない。
- serde_json既定のネスト上限を維持する。不正JSON・UTF-8・時刻・必須項目不足は分類と行/オフセットだけを診断し、本文・パース例外の入力断片を出さない。診断件数もファイルあたり20件に制限する。
- 読み取り開始時のファイル長でスナップショットを制限し、追記は次回同期する。途中の切り詰めで読み取りが失敗した場合はそのカーソルを確定せず再試行する。追記・置換・切り詰め・コピーは共通カーソルとイベントIDで処理する。
- メッセージ本文・ツール名/引数/結果・推論本文を保持しない。UIには固定のイベント種別を表示する。詳細の抜粋もClaudeは保存済みの最小イベントだけを表示し、元の本文を再読しない。cwd・ブランチ・ID・時刻・ログパスは機能に必要なローカルメタデータとして残す。

## 残る制限

5分を超える処理は過小評価する。本文だけのアシスタントメッセージを終点とするため、途中応答の後の内部処理は推測しない。`progress/system/attachment`、任意深度のサブエージェント階層、sidecarの親Agent IDや自由文概要は集計に使わない。子の親セッションはログ配置から特定する。既存の派生テーブル全体再構築と全履歴のUI転送の制約は継続する。先頭4KiBと作成時刻を維持した中間書換え＋追記の特殊なケースは既存方式の検出対象外。これらは実環境未検証という制約と区別する。
