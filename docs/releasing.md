# リリース

`.github/workflows/release.yml` は `v*` タグのpushで起動し、GitHub Releaseにインストーラーと `SHA256SUMS` を添付します。AMD版はAMD64（x64）、ARM版はARM64です。

## 実行方法

ワークフローを含むコミットをGitHubリポジトリへpushした後、そのコミットにタグを付けます。以下は通常リリースの例です。

```sh
git tag v1.0.0
git push origin v1.0.0
```

タグは `vMAJOR.MINOR.PATCH` 形式です。`v0.2.0-rc.1` などのSemVerプレリリースも利用でき、自動的にGitHubのPre-releaseになります。`vnext` などバージョンとして解釈できないタグでは検証ジョブが失敗し、公開しません。

ビルド時にタグから `v` を除いた値を `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` に反映します。変更は各ランナー内だけで行い、リポジトリへ書き戻しません。

## 配布対象

各構成は対応するCPUのGitHubホストランナーでビルドします。対象の定義は `.github/release-targets.json` にまとめています。

| OS      | CPU   | ランナー         | 添付ファイル（`<version>` はタグから `v` を除いた値） |
| ------- | ----- | ---------------- | ----------------------------------------------------- |
| Windows | x64   | windows-2025     | `CodeCal_<version>_windows_x64-setup.exe`             |
| Windows | ARM64 | windows-11-arm   | `CodeCal_<version>_windows_arm64-setup.exe`           |
| macOS   | x64   | macos-15-intel   | `CodeCal_<version>_macos_x64.dmg`                     |
| macOS   | ARM64 | macos-15         | `CodeCal_<version>_macos_arm64.dmg`                   |
| Linux   | x64   | ubuntu-22.04     | `CodeCal_<version>_linux_x64.deb` / `.AppImage`       |
| Linux   | ARM64 | ubuntu-22.04-arm | `CodeCal_<version>_linux_arm64.deb` / `.AppImage`     |

インストーラー8ファイルと、それらのSHA-256を記録した `SHA256SUMS` を添付します。LinuxはWebKitGTK 4.1などTauriの実行時依存が必要です。WindowsはWebView2 Runtimeを使用します。Windows ARM64のアプリ本体はネイティブARM64ですが、NSISインストーラー自体はx86で動作します（[TauriのWindows配布説明](https://v2.tauri.app/distribute/windows-installer/)）。

Windowsのコード署名は未設定です。macOSはアドホック署名を使い、Developer ID署名・公証は行いません。これらの信頼済み署名を必要とする配布には、別途証明書と署名設定を追加してください（[macOS署名の公式手順](https://v2.tauri.app/distribute/sign/macos/)）。

## 公開と失敗時の動作

1. タグ形式、型、lint、format、フロントエンドとリリース補助処理のテストを検証します。
2. 6構成を並列ビルドし、各構成でRustコアテストを実行します。成果物の不足・重複・空ファイルを拒否し、GitHub Actions Artifactとして7日間保存します。
3. 全構成が成功した場合だけ、8ファイルが揃っていることを再検証してチェックサムを生成します。
4. Releaseを下書きで作成し、すべてのファイルをアップロードしてから公開します。リリースノートはGitHubが自動生成します。

必要なトークンは標準の `GITHUB_TOKEN` のみです。公開ジョブだけに `contents: write` を付与します。リポジトリ・組織の設定で、使用するActionsとランナーが許可されている必要があります。GitHubホストランナーの提供状況は[公式一覧](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)を参照してください。

ビルド失敗時は公開しません。アップロードや公開処理が失敗した場合は下書きが残る場合があります。Actionsから失敗ジョブを再実行すると既存の下書きへ再アップロードできます。Artifactの保存期限が過ぎた場合は全ジョブを再実行してください。公開済みReleaseの上書きは拒否するため、修正には新しいバージョンタグを使用します。

## ローカル検証

```sh
npm run test:release
```

Node標準テストでタグ検証、全6構成、5ファイルのバージョン整合、全8成果物の収集、欠損・重複・空ファイルの拒否、チェックサムとGitHub出力を検証します。バージョン設定と成果物処理は `scripts/release.mjs` に実装しています。

2026-11-21にWindows x64でワークフローと同じTauriビルド引数によるNSIS生成、実インストーラーの収集、実マニフェストのコピーに対するプレリリース版設定と `cargo metadata --locked`、actionlintによる両ワークフローの検証を確認しました。GitHub上の実行、残り5構成のビルドと起動、署名・公証、別端末へのインストールは未検証です。この作業ではタグのpushとRelease公開を行っていません。
