# 開発ガイド

Windows 11 x64、Node 24、Rust MSVC stable、C++ Build ToolsとWindows SDK、WebView2を用意します。npm ci後に npm run tauri dev。ネイティブを含まない表示開発用は npm run dev (127.0.0.1:1420) ですがIPCの実データには接続しません。

## TDD

振る舞いを小さく分け、失敗テスト → 失敗理由確認 → 最小実装 → 全テスト → リファクタリング → 再テスト → Conventional Commitの順に進めます。実装済みのテストが落ちた場合は原因を修正し、削除や無効化で回避しません。

開発時にはparserの未認識イベント、区間計算の空実装、storeの空スナップショット、calendar関数の空実装、UIの空コンポーネントでRedを確認しています。追加ケースとして、開始前の依頼文の対応付け、未来スキーマ拒否、同サイズのログ書換えも失敗を確認して修正しました。E2Eではイベント購読権限不足を追記の自動反映テストが検出しました。

## 構成

- src-tauri/src: parser / activity / store / desktop
- src-tauri/tests: 合成ログを使ったRust結合テスト
- src: React UI / calendar計算 / IPC型付きアダプター / Vitest
- tests/e2e: Windows WebView2に直接接続するPlaywrightテスト
- scripts/create-sample.mjs: 機密を含まないデモログ生成
- docs: 設計、計算定義、実ログ調査、検証結果

ログ形式を拡張するときは、値の入っていないキー構造だけを調査記録に残し、匿名化した最小サンプルでテストを追加してください。

## Git・CI

ローカルはConventional Commits。GitHub Actionsのwindows-latestで型、lint、format、Vitest、Rust fmt/clippy/test、Vite、TauriビルドとネイティブE2Eを実行する構成です。リモートリポジトリへのpushとホスト上でのCI実行は本作業では行いません。

リリース補助処理のテストは `npm run test:release` で実行し、通常CIとリリースの両方で検証します。`v*` タグから6構成をビルドして公開する手順は [リリース](releasing.md) を参照してください。

## データの隔離

CODEX_HOMEを合成ログディレクトリ、CODECAL_DATA_DIRを空の専用保存ディレクトリに指定すると、通常の履歴から独立してテストできます。実ログのプロンプト・ツール出力・DBはGitへ追加しません。.local/ と *.db はignore対象です。
