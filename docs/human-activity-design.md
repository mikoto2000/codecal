# Rei 人間アクティビティ可視化: Phase 0 設計

2026-11-26。設計と段階実装の記録。**Phase 2までの読取/設定/同期/IPC/削除を実装。日週のブロックはPhase 3、月と集計はPhase 4、全経路の性能ゲートはPhase 5で追加・検証する。**
CodeCal は活動種別と観測推定時間だけを表示する。具体的な活動内容・仕事・生産性・勤怠を推定しない。

## 調査基準と差異

`git fetch origin` 後の CodeCal main は `7d0c49c2854297d6a2c6b9f30ad9fcd3a714ba8f`。
Rei HEAD は `42c40ea8f`。依頼の基準から実装差分はない。両方で AGENTS.md は見つからなかった。
Phase 1 作業中の再確認では Rei HEAD が `84813a825` へ進んでいたが、指定された6クラスは `42c40ea8f` から差分がなかった。Reiへの変更は行っていない。
未追跡の `docs/rei-human-activity-design.md` は利用者の資料として元ディレクトリに保持し、本書は別名にした。

調査した実装:

| 実装                                                        | 確認結果 / 対応                                                                                                                                                                      |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Rei `SqliteActivityStore`                                   | `activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT)`。開始は Unix ms。`replace` は id と captured_at が一致する行の payload を更新する。追記カーソルだけでは不足 |
| Rei `ActivityRecord`                                        | `durationEstimate` は long の秒。非負検証のみで上限なし。`duplicate` は重複画面の再観測であり除外しない                                                                              |
| Rei `ClassificationDiagnostics`, `EntertainmentDisposition` | 保存済み `detection.diagnostics.entertainmentDisposition` の3 enum。信頼度や理由は参照しない                                                                                         |
| Rei `SemanticSessionPolicy`                                 | 次観測と日付境界で切詰め、短い切替を平滑化する。CodeCal は次観測の切詰めのみ採用し、日跨ぎを保持して表示時に分割                                                                     |
| Rei `DailySummaryAggregator`                                | 判定欠落は UNCERTAIN。分類補完・テーマ/プロジェクト推定などは再利用しない                                                                                                            |
| CodeCal `rei.rs`                                            | `agent_events` / JSONL の AI 専用読取。payload を serde_json に渡す処理は人間用に再利用しない                                                                                        |
| CodeCal `migrations.rs`, `storage.rs`                       | 現行 v3。既知構造検証・移行前 backup・トランザクション。Local 移行は SQLite backup API                                                                                               |
| CodeCal `desktop.rs`                                        | 単一 Store Mutex、spawn_blocking、notify、600ms/最大5秒 debounce、30秒補完走査。人間の長時間全確認中にこの Mutex を保持してはならない                                                |
| CodeCal React                                               | 全 AI snapshot、Luxon 日別分割、汎用 `packSegments`、AIの5分まとめ。人間用期間取得と別集計を追加する                                                                                 |

既存案からの必須変更: UI 表示は「非娯楽」、元判定と continuity_id を保存しない。ID・ソース・元判定を IPC/詳細へ出さない。
サイドバー時間はフィルタ前。コピーの重複除去には確定した実効区間も使う。パスだけのソース ID は採用しない。

## 原本境界とプライバシー

専用 `rei_human` モジュールを追加する。read-only + query_only、busy timeout、整合した read transaction。
`activity_sessions`、AI用テーブル・旧JSONLは人間用読取の条件にしない。原本の schema/checkpoint/VACUUM/backup は実行しない。
JSON 関数の有無と必要な列を確認し、未対応なら固定エラーコードを返す。原本のスキーマから任意 SQL を構築しない。

SQL の入れ子 CASE で型→payload バイト長1MiB→json_valid→duration の integer 型の順に検証する。
duration と固定 enum に正規化した kind だけを投影し、元判定値も Rust に渡さない。id/captured_at の型と ID 長も検証する。
無効行はコード別件数だけを記録し、新規は除外・既存は最後の正常値を保持する。
SQLite例外の原文、パス、行ID、入力値を人間用エラーへ連結しない。

SHA-256 の入力は native_id、captured_at、duration_ms、正規化 kind の決定的な符号化のみ。
禁止情報を変更してもハッシュは変わらない。原本 payload、原判定、内容、推論、画像、URL、アプリ名等は汎用 Event/Turn/diagnostics に流さない。
フロントエンドのレコードは `{start,end,kind}`。表示キーは応答内インデックスとリクエスト世代から作る。
詳細は種別、日付、開始、推定終了、推定継続時間、推定の説明だけ。
人間データを既存の AI export/source_excerpt/テレメトリーへ追加しない。

## 区間・重複・集計

1. ソース内で `(captured_at,id)` 順。同時刻は ID 辞書順最後を代表、種別矛盾は件数診断する。
2. 秒→ms と加算を checked 演算。開始/終了は西暦1〜9999のUTC範囲を許可する。負数、型不一致、overflow、日時範囲外は除外。
3. Rei に上限はないため、任意の24時間上限で捨てない。24時間超を `long_estimate` 件数として診断し、有効な日時範囲なら保持する。
4. 次の異なる開始時刻で候補終了を min。0秒は点として表示し、後続を区切るが和集合には寄与しない。
5. 表示範囲より先に実効終了を確定。分類フィルタで延長しない。未観測空白は残す。
6. コピーは `(native_id,allowed_hash,start,effective_end,kind)` 一致だけを排除。後続の異なるコピーの長い区間は残す。
7. UTCのまま計算し、IANA zone の暦日境界で表示・集計を分割する。DST日を24時間と仮定しない。

最小化した観測と実効終了をキャッシュし、正常同期の同一トランザクションで代表と後続開始時刻による切詰めを確定する。
少数変更では変更前後の時刻と前観測、大量変更では開始索引の順次走査を使い、確定規則は共通にする。
元から削除された行は保持するため、区間計算の観測集合も保持したキャッシュ全体とする。
総時間/種別時間はそれぞれ区間の和集合。分類間の重複をどちらかへ寄せず、内訳合計が総時間を超える説明を表示する。
AI集計の時間・件数は既存計算のまま。点の観測も過去からの長い観測も期間取得で扱う。

まとめ表示は同一ソース/種別/表示日で端点一致だけ。フロントへソースを渡さず、バックエンドに grouping と timezone を指定して結合する。
日・週の AI と人間を識別可能な union 表示モデルで `packSegments` に渡す。
人間の色は amber/purple/gray、文字「人間」と種別・枠線で識別する。
月は日ごとの実時間長を分母とする種別独立3バーを使う。重複内訳を24時間の排他的内訳として積まない。

## DB・ソース管理

v4へ additive migration。`human_activity_sources`: source_id、正規化パス、ファイル識別、reader version、同期状態/世代。
`human_activity_records`: source_id/native_id 複合PK、captured_at、duration_ms、kind、content_hash、effective_end。
期間検索用 `(source_id,captured_at)` と `(source_id,effective_end)` の索引。任意の長時間区間も overlap query で取得する。
初回/v0/v1/v2/v3 移行、rollback、未知版拒否、AI/project/config保持をテストする。

ソースは正規化した論理絶対パスと物理ファイル識別の組で分離する。Windowsは開いたファイルの volume/file ID、Unixは device/inode を使う。
選択したパスの junction/symlink を初回に解決して固定せず、各照合時に選択パスの現在の実体を確認する。
接続とファイル識別の対応を同期前後で確認する。置換時は旧接続を破棄して新 incarnation を作る。識別取得失敗は成功扱いせず既存キャッシュを維持する。
同一パスでも置換された DB を過去の incarnation に混ぜない。通常表示は現在パスの現在 incarnation のみ。
別パスへ移動/コピーしたソースは別管理。同一元ファイルの継続更新と、識別まで保存した特殊な外部復元は区別できない制約が残る。

Windows の bundled SQLite 標準 VFS は DB を `FILE_SHARE_READ | FILE_SHARE_WRITE` で開き、`FILE_SHARE_DELETE` を指定しない。
したがって接続中の物理ファイル置換は OS に拒否され得る。独自 VFS や原本コピーによって回避せず、機能 OFF で接続を閉じ、置換後の再 ON で新しい物理 ID を検出する。
この手順の置換・キャッシュ分離をネイティブテストする。Unix では接続中に置換できるため、読取の前後でも物理 ID を照合する。
Windows の識別には lock 内に既存の `windows-sys 0.61.2` のファイル ID API を直接利用し、新規クレートは追加しない。

## 同期・キャンセル

専用 worker と継続 read connection。同一接続の data_version、DB/WAL 通知、物理識別、30秒補完を併用。
初回/手動/ソース変更は全確認。自動は同一接続で version 不変なら省略する。AIだけの更新も全確認が必要。
1ソース1ジョブ、通知は dirty flag に集約、通知を600msデバウンスし、自動確認は前回完了から最小1秒、完了後に dirty を再確認する。
全確認は rowid キーページ256行、read transaction を使い、許可フィールドだけをディスク上の一時 SQLite へ格納する。
初回カーソルは明示的な未指定値とし、rowid の最小値・0・最大値を含めて走査する。
最小化済み一時DBとキャッシュを native_id 順にマージ比較して、追加・更新だけをディスク上の TEMP 表へ蓄積する。原本も一時DBもアプリDBへ ATTACH しない。
メモリはページと診断カウンタに制限し、Store Mutex を全確認中には保持しない。読み終わった時だけ cache を単一 transaction で upsert。
保持接続上の version を読み取り開始前に記録し、完了後に増えていれば次回再確認する。同期中の通知を失わない。

設定保存/削除は世代を増やす。256行境界・確定前に世代照合し、設定保存成功後の新世代以外は適用しない。
ページ/観測/確定前のキャンセル境界と30秒の同期期限を設ける。timeout/locked/中断は正常キャッシュを保ち、成功時刻を更新しない。単一SQLite文内では境界まで待つため、最大payloadでの取消し時間もPhase 5で実測する。
長い read transaction による WAL 保持は性能試験する。許容できない場合は整合性を崩すページ別 snapshot に変更せず、Phase 0/5 のゲートで止める。
AI 同期は独立して継続し、人間の不存在や失敗を AI 失敗へ伝播させない。

## 設定・取得・削除

`human_activity_enabled` は serde default false、rei_enabled と独立。rei_root は共有。
人間 ON で現在ソースのキャッシュ表示と同期、初回種別は全ON。フィルタは期間・ビュー・同期・設定開閉で維持。
機能 OFF で読み取り/自動同期/期間IPCを停止し、画面/詳細/集計から即時除去、接続を閉じる。キャッシュは保持する。
AI/人間両方OFFなら Rei監視を解除する。Rei自体の観測は停止しない。

期間取得は `[from,to)`、最大62日。範囲外開始の overlap と終端確定済み区間を含める。
応答上限は50,000レコード・8MiB。超過を固定エラーで明示し、狭い期間への変更を案内する。無通知切捨てはしない。
UI は日付/ビュー/同期/zone/設定/削除にリクエスト世代を対応させ、古い応答を捨てる。
失敗時は当該範囲を空にして再試行を提供する。バックエンドも設定世代を確認する。

現在/全ソース削除は確認ダイアログ後、機能OFF保存・世代更新・worker停止・cache削除を実行する。
再ONで原本から再取得する旨を説明する。AI/原本には書き込まない。
専用削除は論理削除であり、安全消去を約束しない。secure_delete、WAL truncate、空きページ回収は CodeCal DB にだけ適用する。
新規のCodeCal管理バックアップとLocal移行コピーは公開前に人間キャッシュを除外し、VACUUMする。既存バックアップは変更しない。Phase 1のコア試験とPhase 2のnative移行試験で除外を確認する。
OS/ディスク/外部バックアップの残存は消去保証の対象外。

## 性能ゲートと実装順

### 日・週表示（Phase 3）

「まとめ表示」は現在選択中の一つのソースについて、同じ表示日・種別で端点が接続する正の区間だけを結合する。空白・別種別・0秒の点は結合しない。
表示範囲へ切り詰めてから既存AI集計と共通の日境界処理で分割し、DSTと欠けた暦日を扱う。結合後の出力に50,000件上限を適用する。「記録単位」では確定済み区間をそのまま返す。

人間の種別フィルタとプロジェクトフィルタは独立し、サイドバーの時間は各フィルタを適用する前の値を表示する。人間の種別内の重複は和集合で数える。
カレンダーはAIと人間を共通の配置処理へ渡し、時間帯が重なるブロックを横に分ける。人間は種別色・破線・人間ラベルを併用し、点も操作できる。短い区間の見た目の最小高さは配置計算と一致させる。
人間の詳細は種別・日付・開始・推定終了・推定時間だけを組み立て、観測ID、ソース情報、元判定値、AI側の情報を渡さない。期間・表示単位・フィルタ・設定世代の変更で詳細を閉じる。
同一パスの新しい物理ソースが同期失敗した場合も期間を再取得し、旧ソースの表示を残さない。同じ物理ソースの読み取り失敗では直前の正常キャッシュを利用する。

[予備計測](human-activity-performance.md) と [テストリスト](human-activity-test-list.md) を参照。
Phase 0 は設計と SQLite プローブのみ。実装が存在しない経路の性能は未計測とする。
Phase 1 は Rust 読取/時間計算/DB、Phase 2 は worker/設定/IPC/削除、Phase 3 は日週、Phase 4 は月/集計、Phase 5 は全経路の性能/プライバシー/回帰。
各 Phase で専用 worktree、細粒度TDDコミット、全回帰、自己レビュー、PR/CI/通常マージ、マージ後CI、main更新、後片付けを完了する。
前段の必須検証に問題が残れば次へ進めない。Reiコード・DB・設定・サービスは変更しない。
