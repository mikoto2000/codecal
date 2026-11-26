# 起動直後のスピナー

- [x] JavaScriptの停止で旧版の空白を再現し、DB準備とは別区間と確認
- [x] HTML/CSSだけで初期スピナーを表示し、Reactへの引継ぎで空白を作らない
- [x] module遅延・失敗時も案内と再読込を操作できる
- [x] WindowsではWebView前にネイティブ進行表示を描画し、×でDB初期化前に正常終了
- [x] ウィンドウは初期画面の用意後に表示し、位置・サイズ・最大化を維持
- [x] 初期画面・React・DBの時間を内容やパスを含めず計測
- [x] light/dark・動き軽減・キーボード・CSPと製品ビルドの確認
- [x] 全回帰・製品ビルド・native回帰

運用終了条件: PR CI・通常merge・postCI・main更新・専用worktree/branchの通常削除。結果はPRと作業記録で確認する。

検証: Rust139件（release/all-targets、human-benchmarkを含む）、Vitest127件、Python17件、release9件。nativeは既存19件のうち18件が成功し、移行中の試験が引継ぎ時の二重statusを検出した。Reactのlayout effectで描画前に引き継ぐ修正後、移行・初期表示・早期取消し・位置/サイズ/最大化を含む5件が成功し、計21ケースを確認した。旧ビルドのJavaScript停止で空白となるRed、非表示ランチャー条件でnative進行表示が隠れるRedも確認してから修正した。

型検査・ESLint・Prettier・cargo fmt・Clippy・NSIS製品ビルドが成功。実ユーザーの原本は使わず、Reiは変更していない。画面例と区間計測は[切り分け記録](startup-spinner.md)を参照。
