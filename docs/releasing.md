# リリース

`.github/workflows/release.yml` は `v*` タグのpushで起動し、GitHub Releaseにインストーラーと `SHA256SUMS` を添付します。AMD版はAMD64（x64）、ARM版はARM64です。

## 実行方法

タグを作成する前に、対象コミットのCI成功を確認し、[日本語の概要](#日本語の概要必須)の下書きを用意してください。ワークフローを含むコミットをGitHubリポジトリへpushした後、そのコミットにタグを付けます。以下は通常リリースの例です。

```sh
git tag v1.0.0
git push origin v1.0.0
```

新規リポジトリでは先に既定ブランチをpushし、ワークフローが登録されてからタグを別途pushしてください。既存タグに対する実行記録がない場合は、タグを削除・変更せず、既定ブランチのワークフローを手動起動できます。

```sh
gh workflow run release.yml --ref main -f tag=v1.0.0
```

手動起動も指定タグのソースをビルドし、タグpushと同じ並行実行グループを使います。

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
4. Releaseを下書きで作成し、すべてのファイルをアップロードしてから公開します。変更一覧やPRリンクはGitHubが自動生成します。
5. 公開後、担当者がリリースノートの先頭に[日本語の概要](#日本語の概要必須)を追加し、表示を確認します。ワークフローの成功だけでリリース完了とせず、この確認まで実施してください。

必要なトークンは標準の `GITHUB_TOKEN` のみです。公開ジョブだけに `contents: write` を付与します。リポジトリ・組織の設定で、使用するActionsとランナーが許可されている必要があります。GitHubホストランナーの提供状況は[公式一覧](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)を参照してください。

ビルド失敗時は公開しません。アップロードや公開処理が失敗した場合は下書きが残る場合があります。Actionsから失敗ジョブを再実行すると既存の下書きへ再アップロードできます。Artifactの保存期限が過ぎた場合は全ジョブを再実行してください。公開済みReleaseの上書きは拒否するため、修正には新しいバージョンタグを使用します。

## 日本語の概要（必須）

通常リリースとプレリリースの両方で、リリースノートの先頭に `## 概要` を置き、そのバージョンの主な変更と利用者への影響を日本語で説明します。現在のワークフローはこの概要を自動挿入しないため、タグ作成前に下書きを用意し、公開後の本文更新までをリリース担当者の作業に含めます。

概要は、実際に含まれる新機能・改善・不具合修正を簡潔にまとめます。次の雛形の説明文を実際の内容に置き換え、該当しない分類は削除してください。必要な移行手順や互換性への影響があれば記載し、ビルド成功と実機での動作確認は区別します。

```md
## 概要

vX.Y.Zでは、主な変更と利用者への影響を説明します。

- **新機能**：追加した機能と、できるようになったこと。
- **改善・修正**：改善した操作や、解消した不具合。
```

GitHubが生成した `What's Changed`、PRリンク、コントリビューター一覧、`Full Changelog` は残し、その前に概要を追加します。日本語の概要だけで本文全体を置き換えないでください。既に概要がある場合は、その内容を編集し、同じ節を重複追加しません。[v1.0.1のリリースノート](https://github.com/mikoto2000/codecal/releases/tag/v1.0.1)を記載例にできます。

例えば、次のPowerShell 7の手順で更新できます。`$tag` を今回のタグに置き換え、先に `.local/release-notes/<tag>-overview.md` をUTF-8で作成してください。このファイルには日本語の概要だけを記載します。

```powershell
$tag = "v1.0.2"
$overviewPath = ".local/release-notes/$tag-overview.md"
$notesPath = ".local/release-notes/$tag.md"
$release = gh release view $tag --json body | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw "リリースノートを取得できませんでした" }
if ($release.body.StartsWith("## 概要")) {
  throw "概要は追加済みです。重複させず、既存の概要を編集してください"
}
$overview = Get-Content -LiteralPath $overviewPath -Raw -Encoding utf8
$notes = $overview.TrimEnd() + "`n`n" + $release.body
[IO.File]::WriteAllText($notesPath, $notes, [Text.UTF8Encoding]::new($false))
gh release edit $tag --notes-file $notesPath
if ($LASTEXITCODE -ne 0) { throw "リリースノートを更新できませんでした" }
gh release view $tag --json body --jq .body
```

更新後はReleaseページを開き、先頭の日本語概要が対象バージョンの変更に対応していることと、自動生成された変更一覧・リンクが残っていることを確認してください。本文の追記にタグの付け直しや配布ファイルの再アップロードは不要です。

## ローカル検証

```sh
npm run test:release
```

Node標準テストでタグ検証、全6構成、5ファイルのバージョン整合、全8成果物の収集、欠損・重複・空ファイルの拒否、チェックサムとGitHub出力を検証します。バージョン設定と成果物処理は `scripts/release.mjs` に実装しています。

2026-11-21にWindows x64でワークフローと同じTauriビルド引数によるNSIS生成、実インストーラーの収集、実マニフェストのコピーに対するプレリリース版設定と `cargo metadata --locked`、actionlintによる両ワークフローの検証を確認しました。GitHub上の実行、残り5構成のビルドと起動、署名・公証、別端末へのインストールは未検証です。この作業ではタグのpushとRelease公開を行っていません。
