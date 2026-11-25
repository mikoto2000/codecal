# Phase 2 テストリスト

添付依頼を優先する。実データは使わず、各小変更をRed→Green→整理してコミットする。

- [x] 旧設定と初期設定は人間機能OFF、AI設定との4通りを独立して保存
- [x] 保存失敗・キャンセルで有効設定を変えず、再起動で保持
- [x] 同一接続のdata_version・WAL・手動全確認・物理置換・ソース再選択
- [x] workerの単一起動、通知集約、最小間隔、30秒補完、失敗再試行
- [x] 原本走査中は既存Store Mutexを保持せず、世代・期限で中断
- [x] OFFで原本接続・監視・期間IPC停止、正常キャッシュ保持
- [x] 専用期間IPCは現在のソースだけ、62日・件数上限・固定診断
- [x] 設定変更/日付/ビュー/zone/同期の古い応答を破棄
- [x] 現在/全ソース削除を確認し、設定OFF・AI/原本保持・旧job復活防止
- [x] 管理staging残存の回収と論理削除/物理回収の説明
- [x] 合成秘密文字列がIPC/診断/異常系/設定画面へ漏れない
- [x] 全回帰・型/lint/fmt/Clippy・ビルド・native E2E・自己レビュー
- [ ] PR/CI/通常マージ/マージ後CI/main更新/worktreeとbranch削除

日週のブロック・詳細はPhase 3、月集計はPhase 4、反復性能/RSS/長時間WAL等はPhase 5。

ローカル結果: Rust107件（runtime14件に30秒補完試験を含む）、Vitest85件（期間フック6件、設定画面2件を追加）、Python9件、release9件、Windows native E2E16件の計226件が成功。fmt/Clippy/typecheck/lint/Prettier/Vite/Tauri+NSISも成功。全件で合成データのみを使用。
自己レビューで別期間待機中の旧データ再表示と管理領域のリンク切れ誤判定を発見し、それぞれRed→Greenで修正。PR以降の完了証跡はGitHubと作業記録に記録する。
