# 人間アクティビティ性能検証

## Phase 0: SQLite方式の予備計測

2026-11-26、Windows ネイティブ PowerShell、Python 3.12.10 / SQLite 3.49.1。
標準ライブラリのみ。実アクティビティや Rei の原本は使わない。
ソースは一時ディレクトリに作成した WAL DB。各行に ID/開始 ms/60秒推定、正規化対象の判定、漏洩検出用の合成秘密文字列を持つ。
本番の rusqlite bundled SQLite と Python SQLite の性能・バージョンは同一とは限らない。

再現方法 (リポジトリルート):

```powershell
New-Item -ItemType Directory -Force .local/tmp | Out-Null
$env:TMP = (Resolve-Path .local/tmp).Path
$env:TEMP = $env:TMP
python -B -m unittest discover -s scripts -p human_activity_probe_test.py
python -B scripts/human_activity_probe.py --sizes 10000 100000 1000000
```

Windows の制限付き環境では OS Temp への SQLite 書込が拒否されたため、書込許可のある `.local/tmp` を指定した。
この設定は実行したプロセス環境だけに適用する。実 DB のパスを受け取るオプションは提供しない。

9テスト成功。最初に未実装モジュールによる Red を確認してから投影を実装した。
自己レビューで SQLite の signed 64bit を超える JSON 整数が REAL として返る問題を、失敗テストで確認して修正した。
秘密文字列を含む未知判定、本文、不正JSON、巨大payload、durationの型不一致が抽出結果に出ないこと、3種別、欠落値、WAL、read-only、同一接続data_version、読み取り中コミットの snapshot 分離を検証する。
本番 cache/IPC/UI/export/backup の漏洩試験はまだ存在せず、Phase 1〜5 で追加する。

### 実測値

単位 ms、単回 warm/local 測定。整数型の追加検証後に再測定した値。専用targetのreleaseビルドも並行実行中のため負荷の影響を含む。
走査は SQL 投影結果を256行ずつ Python に取り出し、合成秘密文字列がないことも確認する。
追加後と更新後、AIのみ更新後は全投影を再実行している。キャッシュ適用・ハッシュ・期間検索を含まない。

| 元行数    | 初回投影 | 変更なし version 確認 | 1件追加後 | 1件更新後 | AIのみ更新後 | reader中のwriter commit |
| --------- | -------- | --------------------- | --------- | --------- | ------------ | ----------------------- |
| 10,000    | 36.63    | 0.098                 | 37.54     | 30.88     | 39.25        | 1.947                   |
| 100,000   | 293.42   | 0.030                 | 279.26    | 274.00    | 277.64       | 2.713                   |
| 1,000,000 | 3171.50  | 0.037                 | 3060.96   | 3452.76   | 3488.20      | 2.881                   |

| 元行数    | source DB bytes | WAL bytes | pinned read中の1 commitによるWALファイル長増分 |
| --------- | --------------- | --------- | ---------------------------------------------- |
| 10,000    | 4,096           | 2,752,192 | 12,360                                         |
| 100,000   | 20,692,992      | 4,350,752 | 0                                              |
| 1,000,000 | 225,890,304     | 4,350,752 | 0                                              |

DB容量は WAL に未反映のページを含まない。WALファイル長0増分でも既存領域にフレームが書かれており、書込がなかったという意味ではない。
読み取り snapshot の途中の追加は旧 snapshot に現れず、読み取り終了後に見えた。
単一commitの測定から長時間読み取りのWAL肥大化や継続書込性能への影響は判断できない。

### 本番経路の目標

この端末相当のローカル SSD、合成1M行、通常の約200 byte payloadを基準とし、Phase 5 で反復測定して中央値/p95を記録する。
予備抽出が約3.2秒なので初回同期10秒、変更後全確認10秒、変更なし確認100msを暫定予算とする。
大きい有効 payload では1MiB制限内でもCPU/IOが増えるため、別途負荷試験し、30秒の同期期限と中断後のキャッシュ保持を検証する。

| 経路                                   | 目標                                                         |
| -------------------------------------- | ------------------------------------------------------------ |
| 初回・追加・更新・AIのみ更新の1M全同期 | p95 10秒以内、30秒でキャンセル可能                           |
| 自動変更なし                           | p95 100ms以内                                                |
| 日/週/月期間取得                       | p95 100/200/500ms以内                                        |
| IPC                                    | 8MiB/50,000行まで。超過は明示エラー、切捨てなし              |
| 同期によるRSS増分                      | 128MiB以内、ディスク上 staging、256行ページ                  |
| UI操作                                 | 同期中の操作応答 p95 100ms以内                               |
| 原本writer                             | 同時readなしに対するcommit p95遅延が2倍以内かつ追加100ms以内 |
| WAL                                    | read中の増分・回収時刻を記録。reader解放後も増え続けない     |

DB容量、応答サイズ、同時更新の追従時間は測定して文書化する。計測の再現条件・ハードウェアとばらつきを残す。
目標未達なら query plan/staging/index/ロック保持を改善し、通らないまま完了扱いにしない。
AIだけの更新が頻繁な場合の全走査コストは既知の制約。Rei側の変更sequence追加は将来案とし今回変更しない。

### Phase 1 Rust 基盤の予備計測

`cargo run --release --locked --manifest-path src-tauri/Cargo.toml --example human_benchmark -- 10000 100000 1000000` で再現する。
原本のパスを受け取らず、一時ディレクトリ内に合成DBを生成する。約160 byteのpayload、12文字の連番ID、1分間隔・60秒の観測を使用した。
Reiの実際のUUID分布、大きいpayload、反復p95、UI/IPCはこの測定の対象外。

単位ms、Windowsローカルでの単回測定。他の検証の負荷を含む。

| 行数      | 初回staging | 初回適用 | 変更なし確認 | 1件追加 | 1件更新 | AIのみ更新 |
| --------- | ----------- | -------- | ------------ | ------- | ------- | ---------- |
| 10,000    | 62.20       | 116.69   | 0.190        | 133.01  | 63.43   | 68.25      |
| 100,000   | 412.05      | 681.82   | 0.184        | 542.99  | 719.59  | 556.93     |
| 1,000,000 | 5978.11     | 9412.80  | 0.204        | 6459.64 | 6749.74 | 6525.17    |

100万行の初期実装は初回約21.8秒、更新約29.2秒だった。prepared insert、最小化済み2ストリームの比較、変更時刻と直前区間だけの再計算、初回の索引一括構築で改善した。
改善後も初回は約9.1〜16.3秒とばらつく。暫定の10秒p95目標は未達・未検証であり、Phase 5の性能ゲートを通過したとは扱わない。

上表の100万行で日取得2.68ms、週16.58ms。42日分は50,000行を超えて `period_too_dense` を返した。無通知の切捨てはしない。月表示の集計経路はPhase 4で別途検証する。
キャッシュDBは252,358,656 byte、WALは253,870,312 byte。初期実装のDB514,625,536 byteから、32byte BLOBハッシュとWITHOUT ROWID等で縮小した。
履歴末尾と空の将来日の検索が約1秒かかる走査も発見し、期間と直前の代表1件へ索引で絞るよう修正した。
修正前は末尾日919.18ms、空の将来日1073.75ms、修正後は5.13ms、0.35msだった。
修正後の100万行再計測では初回staging5753.77ms+適用10496.25ms、追加10802.97ms、更新9879.01ms、AIのみ8347.98ms、変更なし0.320ms。
日4.86ms/102,270 byte、週31.61ms/715,710 byte、42日分は188.17msで密度上限エラー。native E2Eを並行実行した負荷下の値であり、反復・分離測定はPhase 5に残る。

### Phase 1時点で残した後続ゲート

Phase 1時点ではworker同期、IPC、RSS、UI応答、連続writer、WAL、取消し、UUID反復p95が未計測だった。Phase 5の結果は下記に記録する。
Phase 0 のプローブをこれらの成功証拠として扱わない。

## Phase 5: 本番コア経路の反復計測

2026-11-26、Windows build 26300 x64、Core i9-12900（16 core / 24 logical）、RAM 128GiB、Samsung 990 PRO NVMe 4TB。Rust 1.99.1、bundled SQLite 3.51.1、Node 24.22.1 / npm 11.19.1、Python 3.12.10。全データは合成で、Rei原本・設定・サービスは変更していない。

通常200byteのpayload（うち禁止内容の合成マーカーはSQLから取り出さない）、1分間隔・60秒推定、連番IDとSHA-256から生成したUUID v4形式ID。各件数・各ID分布で5回、各回は新規source/cacheと新規接続。全体で30回。fixture生成直後なのでOSファイルキャッシュはwarmであり、再起動直後やOS cache flushを伴うcold測定ではない。他のbuild/testは同時実行しなかった。OS・Defender・Reiの設定は変更していない。

初回は最小化staging・ハッシュ・区間確定・キャッシュ適用とtransaction commitを含む。追加/更新/AIのみは1変更後の同じ全同期経路で、SQL投影だけの値ではない。変更なしは保持接続のdata_version確認。月は専用の42日要約経路。rawの42日分が50,000行を超える場合は明示エラーとなり、月要約の成功と混同しない。

単位ms。各セルは中央値 / p95。nearest-rank法でN=5のp95は最大実測値。母集団の95%を統計的に保証するものではなく、余裕の小さい初回経路は端末・負荷に依存する。

| 件数 / ID            |              初回 |    変更なし |             1追加 |             1更新 |            AIのみ |
| -------------------- | ----------------: | ----------: | ----------------: | ----------------: | ----------------: |
| 10000 / sequential   |   287.14 / 437.72 | 0.09 / 0.11 |   271.15 / 334.40 |   295.30 / 379.61 |   349.41 / 379.11 |
| 10000 / uuid         |   318.41 / 379.56 | 0.09 / 0.21 |   344.36 / 424.55 |   310.28 / 452.10 |   284.02 / 329.79 |
| 100000 / sequential  |  956.71 / 1109.22 | 0.10 / 0.12 |   645.78 / 778.14 |   679.44 / 698.44 |   631.85 / 750.71 |
| 100000 / uuid        |  966.02 / 1042.33 | 0.11 / 0.16 |   863.22 / 943.02 |   773.76 / 914.36 |   813.44 / 893.08 |
| 1000000 / sequential | 6325.65 / 6698.39 | 0.12 / 0.13 | 4381.97 / 4701.50 | 4328.52 / 6442.83 | 3956.95 / 5561.94 |
| 1000000 / uuid       | 9067.07 / 9858.30 | 0.14 / 0.19 | 5606.49 / 7080.92 | 5319.79 / 5968.54 | 5127.20 / 5359.19 |

| 件数 / ID            |            日 |             週 |        月要約 |
| -------------------- | ------------: | -------------: | ------------: |
| 10000 / sequential   |   1.76 / 1.97 |  11.91 / 13.78 |   2.50 / 3.06 |
| 10000 / uuid         |   2.25 / 3.89 |  15.00 / 29.87 |   2.46 / 3.78 |
| 100000 / sequential  |   1.89 / 1.97 |  12.83 / 13.61 | 14.18 / 20.59 |
| 100000 / uuid        |   3.68 / 4.38 |  22.34 / 24.52 | 15.89 / 20.96 |
| 1000000 / sequential |   2.18 / 2.28 |  14.15 / 14.67 | 14.29 / 15.08 |
| 1000000 / uuid       | 19.58 / 29.41 | 84.06 / 142.79 | 18.91 / 29.28 |

全6条件で全コア予算を通過。100万UUIDの初回p95は9858.30msで、10秒目標に対する余裕は約142ms。測定対象外のcold起動や別端末でも同値と保証しない。

最終30回のコアプロセス観測peak RSSは連番83,972,096 byte、UUID84,324,352 byte（約80.4MiB）。これはプロセス全体なので同期によるRSS増分の128MiB上限に対する保守的な上限にもなる。20ms間隔を要求した監視だが実際の平均間隔は約48〜51ms。WorkingSetはOSのプロセスpeak値を監視中に読み、PrivateBytesはサンプル最大値。WebView2を含む画面全体のメモリとは別である。

100万sequential: cache本体最大224,071,680 byte、WAL最大225,413,472 byte。日IPC DTOは最大102,270 byte、週は715,710 byte、月要約は7,292 byte。DB本体とWALは同じページを含むので論理データ量として単純合算しない。

100万uuid: cache本体最大275,132,416 byte、WAL最大276,773,392 byte。日IPC DTOは最大102,270 byte、週は715,710 byte、月要約は7,292 byte。DB本体とWALは同じページを含むので論理データ量として単純合算しない。

### ボトルネックと改善

改善前の同一200byte合成100万UUID単回では初回42.88秒（staging20.52秒 + 適用22.36秒）、追加19.32秒、更新17.27秒、AIのみ17.08秒、月563.67msで予算未達だった。これはN=1の探索値であり改善後p95との厳密な統計比較ではない。物理ファイルIDの1000回確認は約14msで主因ではなかったため、識別確認を削除する最適化は行っていない。

最小化stagingをheapへ追加して原本snapshot解放後にnative ID索引を構築、集計用の小さい被覆索引、不要な終了時刻索引とハッシュ複製の削除、時刻順かつ空キャッシュへのSQLite内一括挿入、32MiBのworker page cacheで改善した。通常の差分方式は削除・不正更新された正常キャッシュを保持する。行形式とuser_version=4は変更せず、旧索引はtransactionで修復する。

一括挿入は非公開HumanStageだけを読み取り専用ATTACHする。原本のATTACHはしない。URIのreadonly確認・書込拒否・切離し・失敗時rollbackを試験した。画面の起動/設定反映が追加で強制走査を要求していた箇所も修正した。

### 再現手順

```powershell
New-Item -ItemType Directory -Force .local/tmp | Out-Null
$env:TMP = (Resolve-Path .local/tmp).Path
$env:TEMP = $env:TMP
cargo build --release --locked --manifest-path src-tauri/Cargo.toml --examples
$bench = (Resolve-Path src-tauri/target/release/examples/human_benchmark.exe).Path
./scripts/measure-human-benchmark.ps1 -BenchmarkExe $bench -IdKind sequential -Rounds 5 -Label final
./scripts/measure-human-benchmark.ps1 -BenchmarkExe $bench -IdKind uuid -Rounds 5 -Label final
python -B scripts/human_performance_report.py --check docs/performance/human-phase5-core.jsonl
```

独立したCARGO_TARGET_DIRを使う場合はexeパスも合わせる。前2コマンドの新しい出力JSONLをreportへ渡せば再計測結果を判定できる。上の最後の行はコミット済み実測値の再集計。実行中のCodeCalのexeを上書きしない。生データは [core JSONL](performance/human-phase5-core.jsonl)、[集計JSON](performance/human-phase5-core-report.json)。

### 実worker・継続writer・WAL

同じ100万UUID・200byte payloadを5試行。実際のHumanRuntimeを使い、desktopのAppStateと同じく画面用Storeを生存させた。初回、5回の自動変更なし確認、継続writer中の手動強制同期、snapshot後の更新の追従、42日要約を確認。通常の全確認は原本snapshot終了後に私有staging索引を構築する。

worker初回: 中央値9065.61 / p95 9579.45ms。継続writer中の強制同期: 中央値5362.34 / p95 5862.04ms。変更なしの実処理25サンプル: 中央値0.186 / p95 0.347ms。月要約: 中央値26.86 / p95 44.06ms。全予算通過。通知の600ms debounceと最小1秒間隔の待機はunchanged_queue_msとして別に保存し、実処理の100ms予算と混同しない。

writerは同じ合成原本のagent_eventsへ1行をcommitし、commit後に10ms休止する。SQLite WALの既定autocheckpoint・同期設定を維持し、各trialのbaselineと同時read時で同じ条件を使った。各baseline/同時readで200commit以上を採取した。p95の比が2倍以下かつ差が100ms以下であることを各trialで判定する。

| 試行 | baseline commit数 | 同時read commit数 | baseline p95 ms | 同時read p95 ms |    比 | 最大WAL byte |
| ---- | ----------------: | ----------------: | --------------: | --------------: | ----: | -----------: |
| 1    |               200 |               271 |          23.167 |          17.500 | 0.755 |    1,940,552 |
| 2    |               200 |               265 |          15.046 |          14.070 | 0.935 |    1,915,832 |
| 3    |               200 |               257 |          22.268 |          17.696 | 0.795 |    1,882,872 |
| 4    |               200 |               254 |          13.278 |          15.180 | 1.143 |    1,870,512 |
| 5    |               200 |               256 |          12.287 |          16.379 | 1.333 |    1,878,752 |

全5試行でwriter予算を通過し、snapshotより後に入った更新も次の確認で取り込めた。WALは生成fixtureの巨大な初期WALを測定前にfixture writerでtruncateしてから測った。baselineと同時readのファイル長、解放後のPASSIVE checkpointのlog/checkpointed frame数を生データに記録した。全試行でbusy=0かつ全frameをcheckpointでき、fixture writerのTRUNCATE後は0byteになった。原本をCodeCal readerがcheckpointした結果ではない。製品は原本のcheckpoint・WAL設定変更を一切しない。reader解放後に自動で直ちにファイル長0になるという保証でもない。

初期の接続条件が異なる探索用worker計測では初回11.53秒があった。計測コードが画面用接続を閉じており、最後の接続終了時のWAL処理を毎ジョブ含んでいたため、本番と同じ接続寿命に修正して5試行を再測定した。元の探索値は作業証跡に保持し、上の5試行へ混ぜていない。予算は10秒のまま変更していない。

生データは [worker JSONL](performance/human-phase5-worker.jsonl)、[判定JSON](performance/human-phase5-worker-report.json)。commit時間の全サンプル、待機時間、WALの値を含む。再現例:

```powershell
cargo build --release --locked --manifest-path src-tauri/Cargo.toml --example human_worker_benchmark --features human-benchmark
$worker = (Resolve-Path src-tauri/target/release/examples/human_worker_benchmark.exe).Path
1..5 | ForEach-Object { ./scripts/measure-human-benchmark.ps1 -BenchmarkExe $worker -IdKind uuid -Sizes 1000000 -Label "worker-main-$_" }
python -B scripts/human_performance_report.py --worker --check docs/performance/human-phase5-worker.jsonl
```

専用workerは1スレッドでジョブを直列化する。通知の集約と同期中更新の再確認はRust統合テスト、ファイル監視からの自動反映はnativeテストでも検証する。上のwriter試験では明示したfollow-up requestで後続更新を検証している。

### 大きいpayload・取消し・期限と再試行

約1MiB（1,047,972 byte）の有効JSONを1000行追加した専用合成原本を使用。通常の正常キャッシュを先に作り、100ms後に取消しを要求すると102.91msで終了した。SQLiteの各行を読み進める前にも取消しを照合するため、256行ページ全体の巨大JSON処理を待たない。正常キャッシュは保持された。

本番の30秒期限は変更せず、取消しcallbackだけを各回20ms遅らせて、巨大な数十GiBのfixtureを作らずに実時間の期限を踏む試験も実行した。30043.17msで中断し、正常キャッシュを保持した。これは自然なRei負荷で必ず30秒に達するという測定ではなく、期限処理の実行試験である。通常callbackで再実行すると2347.27msで1001行を取り込めた。合成プロセスのpeak RSSは20,475,904 byte。原本payloadをRustへ取り出さず、禁止内容をstaging/cacheへ複写しない。

生データは [取消しJSONL](performance/human-phase5-cancel.jsonl)。再現:

```powershell
$cancel = (Resolve-Path src-tauri/target/release/examples/human_cancel_benchmark.exe).Path
./scripts/measure-human-benchmark.ps1 -BenchmarkExe $cancel -NoSizeArguments -Label payload-cancel
```

単一SQLite文の途中を強制停止する方式ではなく、行・ページ・索引構築・commit前の境界で中断する。期限はsoft deadlineであり、上記のように境界まで数十ms越える場合がある。キャッシュの原子性を保ってrollbackし、Rei原本への停止・書き込み・checkpointは行わない。

### Windowsネイティブ・実IPC・画面応答

専用のTauri release exe、CODECAL_DATA_DIR / CODEX_HOME / REI_DATA_DIR / WebView2 profileを隔離し、100万件の合成原本を使用した。既存のユーザー用CodeCalは停止・上書きしていない。初回の設定保存からworker完了と画面処理まで7712ms。起動/設定保存による重複した強制走査がないこともrevision=1で確認した。

同期中の「次へ/前へ」30操作は、DOMの表示日が変わり、カレンダーのaria-busyがfalseになるまでをWebView内で計測した。30操作すべてがworker busy中に開始し、p95 48.10ms（最大106.00ms）。100msのp95予算を通過。単なるrequestAnimationFrameの刻みだけをUI操作時間とみなしていない。

実IPCはWebView内でinvoke開始から応答オブジェクトの受信までを測り、時刻とbyte数だけをPlaywrightへ返す。各経路5回、p95は最大値。

| 経路           | p95 ms | 応答 byte |
| -------------- | -----: | --------: |
| 日（記録単位） |   8.80 |   102,270 |
| 週（記録単位） |  33.90 |   715,710 |
| 月（42日要約） |  31.50 |     7,125 |

最初の試験では全記録をpage.evaluateからNodeへ返しており、Playwright/CDPの複製を含めた週取得が321msだった。製品のIPC境界を越えるデバッグ転送を除いて上記へ修正した。元の測定は探索証跡に残し、予算を緩和していない。

同期中のRSSは、起動完了後のTauriと既存WebView2子プロセス7個を対象に、固定した同じプロセス集合の現在WorkingSetを合算して監視した。観測最大増分は127,025,152 byte（約121.14MiB）で、128MiB予算内。約8295msに44サンプル、要求間隔20msだが実平均は約189ms。サンプル間の瞬間値や、新たに生成/終了した短命プロセスは独立追跡していない。コア単独のOS peak RSSも約80.4MiB以下で別に測定した。

生JSONのtree_peak_sum_rss/tree_rss_incrementは、日週月の連続取得後に採取した各プロセスの生涯peakの和と起動後RSSとの差である。各peakが同時とは限らず、同期終了後の試験も含むため、同期中128MiB予算の判定には使わない。判定に使った実際の観測値はsync_memoryに分離した。メモリ全量を128MiB以下と主張するものではなく、同期による増分の予算である。

人間機能ON、同期、期間表示、詳細表示の間のWebView HTTP(S)リクエストを記録し、Tauri/IPCのローカルorigin以外への送信は0件だった。Rustの人間経路もSQLite・ローカルファイルだけを使用する。OS全体の別アプリやWebView2自身の更新通信をpacket captureした試験ではない。詳細に禁止マーカーがないこと、原本内容・同期IDをDTOへ含めないことも検証した。

[ネイティブ実測JSON](performance/human-phase5-native.json)、[ハードウェアとRSS監視結果](performance/human-phase5-resources.json)。再現（検証用exeを別名で準備してから）:

```powershell
$env:CODECAL_E2E_EXE = (Resolve-Path .local/e2e-bin/codecal-human-e2e.exe).Path
$env:CODECAL_NATIVE_PERF_ROWS = '1000000'
$env:CODECAL_ENFORCE_PERF = '1'
npm run test:e2e -- --grep 'human performance:'
```

通常CIは1万件で同じ機能経路を検証する。CODECAL_ENFORCE_PERFを明示したローカル基準端末だけで、ハードウェア依存のUI/IPC/RSS予算を厳格に判定する。Windows以外のネイティブUI/メモリ性能、OS cache flush後のcold性能、実利用者のアクティビティによる性能は未測定。合成試験の結果をそれらの保証に拡張しない。

### 測定範囲と残る制約

性能ゲートの初回は、人間キャッシュが空の状態を対象とした。多数の別ソース履歴を既に保持した状態で新しいソースを初回取り込みする時間は未測定。その場合は旧正常キャッシュを維持する通常の差分・区間解決経路を使う。原本の変更sequenceがないため、1件変更やAIイベントだけの変更でも全確認が必要であり、更新頻度が高い原本では継続的なIO/CPU負荷になる。通知集約・最小間隔はこの負荷を制限するが、Reiの変更を前提にしない。

同じ物理ファイルIDを保持したままのin-place上書きは別ソースとして識別できない。原本の自然な大量更新、全プラットフォームのネイティブ性能、短命プロセスを含む全OSメモリ、外部バックアップを含む完全消去の保証は今回の合成試験から導かない。必要時は設定OFFと明示キャッシュ削除後の原本再取り込みを使う。

### 自動同期を1時間に変更した後のworker計測

人間の製品側の自動確認は前回試行完了から1時間間隔になった。上記Phase 5の通知待機時間は当時の600ms/最小1秒ポリシーの測定であり、現在の待機時間を示さない。`human_worker_benchmark` のみ `--features human-benchmark` を指定してビルドし、合成fixtureに限って1秒間隔で実処理を測定する。通常のデスクトップはこのfeatureの有無にかかわらず `HumanRuntime::new` を使い、1時間間隔のままとする。UI・環境変数・保存済み設定に間隔変更口は追加しない。
