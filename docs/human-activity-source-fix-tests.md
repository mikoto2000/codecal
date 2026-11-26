# 修正テストリスト

- [ ] storage.dbにAI履歴のみ、memory.dbに観測がある実環境相当の配置で同期・日週月取得できる
- [ ] storage.dbに観測があれば優先し、2ファイルの履歴を混在させない
- [ ] 不正/読取不能なstorage.dbから別DBへ黙って切り替えない
- [ ] 同期後に保存先が切り替わっても別ファイルのdata_versionを同一視しない
- [ ] memory.db/WALの通知で自動同期し、人間OFFでは監視しない
- [ ] 現在ソース削除は保存先2ファイルの全世代だけを消し、AI/他Root/原本は保持する
- [ ] 一時領域の回収・OFF・欠落時キャッシュ保持・再ON再取込を両ファイルで検証
- [ ] 読取専用スキーマ調査だけで保存先を識別し、本文・画像・他テーブルの内容は取得しない
- [ ] 固定エラーコードごとに利用者向けの原因表示を行い、原本の値・パスをエラーへ出さない
- [ ] Rust/Vitest/Python/release/native全回帰、fmt/Clippy/type/lint/format/build、PR/CI/merge/postCI/cleanup
