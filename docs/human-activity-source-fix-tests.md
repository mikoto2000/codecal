# 修正テストリスト

- [x] storage.dbにAI履歴のみ、memory.dbに観測がある実環境相当の配置で同期・日週月取得できる
- [x] storage.dbに観測があれば優先し、2ファイルの履歴を混在させない
- [x] 不正/読取不能なstorage.dbから別DBへ黙って切り替えない
- [x] 同期後に保存先が切り替わっても別ファイルのdata_versionを同一視しない
- [x] memory.db/WALの通知で自動同期し、人間OFFでは監視しない
- [x] 現在ソース削除は保存先2ファイルの全世代だけを消し、AI/他Root/原本は保持する
- [x] 一時領域の回収・OFF・欠落時キャッシュ保持・再ON再取込を両ファイルで検証
- [x] 読取専用スキーマ調査だけで保存先を識別し、本文・画像・他テーブルの内容は取得しない
- [x] 固定エラーコードごとに利用者向けの原因表示を行い、原本の値・パスをエラーへ出さない
- [x] Rust/Vitest/Python/release/native全回帰、fmt/Clippy/type/lint/format/build

運用の終了条件: PR/CI/通常merge/postCI/main同期/専用worktree・branch削除。実績はPRと作業記録に残す。

検証: Rust127 / Vitest110 / Python17 / release9 / Windows native18の281ケース。Nativeは全体17成功・設定保存待機の不足を修正後に対象2ケースを同順で再確認。実環境の照会はスキーマと型・件数等の管理情報に限定し、再現データはすべて合成。
