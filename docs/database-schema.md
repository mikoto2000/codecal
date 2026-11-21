# SQLite データモデル

アプリデータ下の codecal.db に保存します。foreign_keys=ON、journal_mode=WAL、busy_timeout=10秒。スキーマバージョンはPRAGMA user_versionとschema_migrationsで管理します。現行は2、新しいバージョンのDBはダウングレードせず拒否します。v1からの移行はトランザクションで行い、既存ターンの取得元は `codex` とします。既存のプロジェクト名・色・設定を維持します。

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
| settings           | key PK、value JSON                                                                                                                            |
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
