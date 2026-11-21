# Codex ログ形式の調査

2026-11-21、ローカル sessions (184ファイル) / archived_sessions (4ファイル) を確認。各ディレクトリ先頭12ファイル、計7,537行の構造のみ集計した。値・プロンプト・ツール出力は転記していない。形式は公開固定仕様とみなさない。

- 外側: timestamp (RFC3339), type, payload。
- session_meta: id と session_id、timestamp、cwd、cli_version。再開・コピーを考慮し、同一IDのメタデータの複数回出現を許容する（再開操作そのものは未観測）。
- turn_context: turn_id、cwd。セッション内でも cwd が変わり得るためプロジェクトをターンに紐づける。
- event_msg.task_started: turn_id、started_at (Unixミリ秒)。
- event_msg.task_complete: turn_id、started_at、completed_at、duration_ms。完了状態と外側時刻を記録する。
- event_msg.turn_aborted: turn_id、root_turn_id、reason、completed_at。中断として扱う。
- event_msg.item_completed: item.type / item.content を持つ。ユーザー依頼は message / user_message 形式も許容。
- response_item.message: role、content のテキスト配列。内部メタデータに turn_id が存在する場合がある。
- token_usage_record: turn_id、turn_token_usage、thread_token_usage。turn_token_usage はターン累積値として上書きする。
- event_msg.token_count: info.total_token_usage と last_token_usage。ターン識別のない旧形式の累積値を正確なターン使用量として割り当てない。
- ツール: response_item.function_call / custom_tool_call と各 output。稼働停止判定には使わない。
- その他: world_state、reasoning などは稼働計算に不要。未知種別はスキップして診断件数を残す。

task_failed / turn_failed は互換入力として扱うが今回の標本では未観測。エラーのツール結果だけではターン失敗と判定しない。アーカイブ先にも同じJSONL構造が存在するが、アーカイブ操作の実行自体は観測していない。同一セッションIDと正規化イベントのハッシュで重複を防ぐ。終了イベントなしでは現在実行中かクラッシュ後か判断できず unknown とする。時刻の間隔で終了を推測しない。

ログは読み取り専用。テストはすべて合成JSONL。DBは必要な正規化情報と依頼文（最大16Ki文字）、出典オフセットのみ保存し、ツール出力・認証情報・推論本文を複製しない。依頼文にも秘密が含まれ得る。生ログは詳細操作時にローカルで最大64KiBだけ読み出す。
