# SQLite データモデル

アプリデータ下の codecal.db に保存します。foreign_keys=ON、journal_mode=WAL、busy_timeout=10秒。スキーマバージョンはPRAGMA user_versionとschema_migrationsで管理します。現行は4です。v0〜v3は構造を確認し、トランザクションでv4へ移行します。既知の互換v3には専用テーブルを追加し、新しいバージョンや非互換構造のDBは変更せず拒否します。旧ターンの取得元は `codex` とします。移行前にはWALを含む `codecal.db.before-v4.bak` を作成し、既存ファイルがあれば `.1`、`.2` と連番を付けて保存します。旧 `.before-v3.bak` も変更しません。既存のプロジェクト名・色・設定を維持します。

| テーブル           | 主な列・制約                                                                                                                                  |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| projects           | id PK (正規化cwdのSHA256)、cwd UNIQUE、name、color、last_seen                                                                                 |
| sessions           | id PK (ログのsession id)、project_id FK、start、last_event                                                                                    |
| turns              | id PK (session:turn)、session_id FK、turn_id、project_id FK、start、end nullable、status、prompt、tokens nullable、UNIQUE(session_id,turn_id) |
| activity_intervals | id PK、project_id FK、turn_id UNIQUE FK ON DELETE CASCADE、start、end nullable、status                                                        |
| events             | id PK (session + 正規化イベントのSHA256)、session_id、ts、event JSON、session/ts索引                                                          |
| event_sources      | event_id FK、path、offset、line、PRIMARY KEY(event_id,path)                                                                                   |
| sync_state         | path PK、offset、line、head、head_len、identity、session_id、updated                                                                          |
| diagnostics        | path PK、message（生ログは含めない）                                                                                                          |
| settings           | key PK、value TEXT（設定はJSON、破損設定の退避は元の文字列）                                                                                  |
| rei_event_sources  | event_id FK、path、project、native_id、offset、PRIMARY KEY(event_id,path)                                                                     |
| rei_sync           | path、project、sequence、anchor、identity、PRIMARY KEY(path,project)                                                                          |
| rei_file_sync      | path PK、offset、head、head_len、identity                                                                                                     |
| schema_migrations  | version PK                                                                                                                                    |

cwdが変わるセッションではturn_contextでターンのproject_idを更新します。プロジェクト表示名・色は再同期でも上書きしません。Windowsドライブパスは区切り文字を統一し、小文字化してプロジェクト識別します。存在しない過去の作業パスも解析できます。POSIXパスの大小文字は保存します。

## 増分取り込み

読み取り位置は改行まで正常に読み終えたバイト位置です。不正な完全行は診断した上で先へ進み、末尾の改行前の断片は次回まで待ちます。前回の先頭部分のハッシュ、作成時刻、更新時刻、ファイル長を比較します。切り詰め・置換なら当該出典を解除して最初から再解析します。元ファイルは変更しません。

移動後のファイルが以前の作成時刻・先頭ハッシュを保持し、古いパスが存在しなければ、DB上の出典とカーソルを新パスへ移します。コピーされたアーカイブは解析しても、イベントの主キーとsession/turnの一意制約により二重計上しません。複数出典を保持し、一方を切り詰めても他方のイベントを失わないようにします。削除されたログの履歴は保持します。

ファイル単位のイベント・出典・カーソル更新はトランザクションです。派生テーブルの再構築もトランザクションで置き換えます。原本からの読み取りは増分、派生再構築はキャッシュからの再生です。

## RoamingからLocalへの移行

Windowsの既定保存先は `%LOCALAPPDATA%/dev.codecal.desktop/codecal.db` です。旧RoamingにDBがあり、LocalにDBがなければ、読み取り専用接続とSQLite backup APIで一時DBへスナップショットを取り、既存ファイルを上書きしない操作でLocalへ確定します。WALに残るコミット済みデータも含まれます。旧DBは残し、既存Local DBがあればそちらを優先します。環境変数による保存先指定では、この移行を実施しません。

## れい対応（v2）

`turns` に `source`（codex / rei）、`native_session_id`、`native_turn_id` を追加します。SQLite・旧JSONLの出典は `rei_event_sources`、プロジェクト別sequenceは `rei_sync`、JSONLの読み取り位置は `rei_file_sync` に保存します。正規化イベントと区間はCodexと共通です。詳細は [れいの実行履歴](rei-history.md) を参照してください。

イベント更新と同じトランザクションで `settings._activity_dirty` を設定し、派生テーブルの再構築と同時に解除します。変更がない同期では再構築せず、中断後はDBオープン時にも未反映の更新を再生します。

## Claude対応と設定の復旧

`turns.source` は `codex` / `rei` / `claude`。Claudeの `details`（親セッションID・gitブランチ）は正規化イベントに保持し、snapshotでは対象セッションのイベントを一括走査して参照し、ターンごとの再探索を行いません。旧v3に任意で存在する `claude_files(path,fingerprint)` は保持しますが、新規DBには作成しません。

DB内の `settings.config` が不正なJSONまたは無効な設定なら、起動時に元の値を `_config_backup_<時刻>_<連番>` というキーへ退避し、既定値への置換と同じトランザクションで保存します。復旧した事実と退避キーを画面へ表示します。履歴やプロジェクト設定は変更しません。3要素のしきい値が現行形式です。旧4要素の設定は互換読み込みとして先頭3要素を使用します。

## 人間アクティビティ専用キャッシュ（v4）

設定・同期・期間IPC・削除はPhase 2で接続しました。AI のテーブル・snapshot に人間の観測を追加しません。

| テーブル               | 列と用途                                                                                                                                                                      |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| human_activity_sources | source_id PK（32byte digest BLOB）、source_path（選択した論理絶対パス、内部のみ）、file_identity、reader_version、last_sync、sync_status、diagnostics（固定コードと件数のみ） |
| human_activity_records | source_id FK / native_id の複合PK、captured_at（UTC ms）、duration_ms、kind（3種別）、content_hash（32byte digest BLOB）、effective_end（非代表の同時刻観測はNULL）           |

`human_activity_records` は WITHOUT ROWID とし、`(source_id,captured_at,native_id)`、`(source_id,effective_end)` に索引を持ちます。
判定文字列・元JSON・画面や活動の具体的内容は保存しません。ハッシュは native_id、開始、推定継続時間、正規化種別だけから生成します。
ソースごとの区間確定をキャッシュ反映と同じトランザクションで行い、削除された観測・不正更新の前回正常値は保持します。
変更が少ない場合は変更時刻と直前の観測だけを再計算し、大量更新時は開始索引を順に読みます。

移行前バックアップと Local 保存先へのコピーは、作成した私有コピーから人間用2テーブルの行を除外して VACUUM します。
コピー元・Rei 原本・AI履歴は変更しません。旧バックアップは上書きせず保持します。
このキャッシュ除外はCodeCalが新しく作るコピーの方針で、OSや利用者が別に作ったコピーの安全消去を保証しません。

`settings.config.human_activity_enabled` は既定falseで、`rei_enabled`とは独立します。明示的な履歴削除では選択論理パスの全ファイル世代または全ソースを対象とし、専用テーブルの行削除と設定OFFを同じtransactionで確定します。OFFにするだけなら行は保持します。削除後のVACUUM・WAL truncate・管理staging回収は成否を別に通知し、物理的な完全消去とは区別します。
