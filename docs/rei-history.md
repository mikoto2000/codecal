# れいの実行履歴

Codexと同じく、開発かどうかは分類せず、明示的な開始と終了を持つ実行を集計します。ネットワーク接続・認証・れい側の変更は不要です。

## 入力と正規化

`REI_DATA_DIR` またはOS標準のデータディレクトリを既定値とし、設定で変更できます。`storage.db` の `agent_events` を読み取り専用・query_only接続で読みます。SQLiteの通常の読み取りトランザクションにより、WAL内のコミット済みイベントも同じスナップショットで参照します。元DBのマイグレーション・checkpoint・書き込みは行いません。

旧形式の `projects/<projectId>/events/events.jsonl` も対象です。1MiBを超えるレコードは診断してスキップし、JSONLの未改行末尾は次回まで待機します。不正なイベントの本文を診断へ出力しません。対応するイベントschema versionは1です。

| イベント                                  | 正規化      |
| ----------------------------------------- | ----------- |
| AGENT_RUN_STARTED / agent.run.started     | start       |
| AGENT_RUN_COMPLETED / agent.run.completed | completed   |
| AGENT_RUN_FAILED / agent.run.failed       | failed      |
| AGENT_RUN_CANCELLED / agent.run.cancelled | interrupted |

時刻は外側のtimestamp（Unix秒の小数、またはRFC3339文字列）をUTCミリ秒に変換します。payloadのduration、会話の更新時刻、ファイル更新時刻から実行時間を推測しません。開始がない終了は区間を作らず、終了欠落・開始より前の終了は終了不明として扱います。

実行の対応付けにはprojectIdとrunIdを使い、元sessionId・turnIdも保持します。内部ではprojectIdとrunIdから独立した実行用セッションを作り、開始と終了のsessionIdの欠落・違いに影響されず対応付けます。元の会話セッションIDはnative_session_idに保持します。内部セッションID・イベントIDは `rei:` の名前空間を使用し、Codexと衝突しないようにします。

`projects.json` のid/name/pathからプロジェクトを解決します。同じ正規化パスならCodexと同じプロジェクトへ集約します。パス不明時はプロジェクトID別の `rei-project:<id>` を使用します。設定済みの表示名と色は上書きしません。

SQLiteの `turns` に対応するrunIdがあればrequestを補完し、後から保存された依頼文も同期します。保存は16Ki文字までです。会話テーブルの不整合などで依頼文を読めなくても、実行イベントの取り込みは継続し、補足情報の診断を別に記録します。旧JSONLだけの場合は新たな依頼文を補完しませんが、取り込み済みの依頼文は保持したままプロジェクト情報を更新します。completionTokensは総トークン数とは異なるため、既存のトークン使用量には流用しません。

## 増分同期と出典

`rei_sync` は元DBパス・projectIdごとにsequence、その位置のevent ID、ファイル作成識別情報とプロジェクト台帳のハッシュを保持します。sequence後退・anchor不一致・ファイル識別情報や台帳の変更時は再走査します。開始・終了イベントだけを256件ずつ取得し、正規化イベント・出典・同期位置をCodeCal側の同じトランザクションで確定します。正規化イベントが変わった場合だけ派生テーブルを再構築します。再構築が必要なことを示す設定 `_activity_dirty` をイベントと同じトランザクションで保存し、派生テーブルの置換と同時に解除します。再構築前に中断した場合は、次回DBオープン時にも復旧します。読み取り処理の版が変わった場合も再走査します。

`rei_file_sync` はJSONLのバイト位置、先頭ハッシュ、ファイル識別情報と更新時刻を保持します。置換・切り詰め・同期済みファイルの同サイズ書換えを検知すると先頭から読み直します。DBと旧JSONLはprojectIdと元event IDで重複を除き、出典はそれぞれ保持します。元履歴の削除、DB復元、保存先変更、取り込み無効化で、取り込み済みのイベントを消しません。不正SQLiteレコードの診断はプロジェクト別に保持し、増分同期で該当レコードを再読しなくても消しません。再走査で問題が解消したことを確認した際に解除します。既存記録の中間部分だけを同じIDのまま改変する操作は検出できない場合があります。

`rei_event_sources` はevent_id/path/project/native_id/offsetを保持します。SQLiteではoffsetはNULLです。詳細ではDBに記録された出典だけを解決し、SQLiteはproject IDとevent IDで照会、JSONLはオフセットとevent IDで照合します。一方の出典が消えた場合は別の出典を試します。表示は最大64KiBです。

自動同期はCodexとれいの両ディレクトリを監視します。SQLite本体・WAL・プロジェクト台帳・JSONLの変更を検知し、600ms debounce、最大5秒、30秒の補完走査を使います。一方の取得元が読めなくても他方は同期し、問題は診断へ記録します。

## 検証

`src-tauri/tests/rei.rs` は合成SQLite/JSONLで、WAL読み取り、増分、重複除去、ID欠落、並列・時計巻き戻り・完了/失敗/中断、後からの依頼文補完、DB復元、v1移行、取得元エラー分離、補足情報の読み取り失敗、診断の保持、プロジェクト情報更新、旧JSONLの同サイズ書換え、変更なし同期と中断後の派生データ復旧を検証します。Vitestでは設定と取得元別の詳細表示、native E2EではSQLiteのWAL取り込み・出典表示・追記の自動反映・再起動を検証します。
