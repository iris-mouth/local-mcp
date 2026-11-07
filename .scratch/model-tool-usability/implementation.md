# 採用9項目の実装・検証記録

2026-11-07。ブランチは `codex/model-tool-usability`。実装前HEADは `e9413786f9c5aff5b975699c9fcda22fbeaf0fb3`。独立したレビュー後の修正版で最終検証を再実施した。
[仕様](spec.md)の9項目を実装した。準備時点のソース・環境・検査は[baseline.md](baseline.md)に履歴として残す。

## 実装内容

| 項目 | 実装 | 主な確認先 |
| --- | --- | --- |
| 01 UTF-8ページ分割 | 原文byte位置のカーソルを保ち、正常な日本語・4byte文字をページ境界で壊さない。未完末尾はpending_utf8、予算不足はbudget_limitedで示す | src/jobs.rsの境界試験、tests/usability.rs、実App Server |
| 02 引数説明 | 12ツールの各引数へ意味・単位・既定値・短い例を追加。argv、観測待ち、request_id、stdinと停止ACKの意味を説明 | src/mcp.rs、tests/transport.rs、tests/schemas.py |
| 03 詳細エラー | field / expected / recoveryを追加。引数の値を転載せず、実行前の引数拒否だけdispatch=not_attemptedとする | src/protocol.rs、tests/usability.rs、実App Server |
| 04 出力スキーマ | src/schemas.rsでツール別の必須field、型、enum、nullable、エラーを宣言。native imageの既存例外を維持 | tests/schemas.py、image wire回帰 |
| 05 読取量 | read_fileは省略時200行・64KiB。max_bytes、next_line / has_moreで継続し、完全な行とCRLFを返す | tests/usability.rs、tests/contracts.rs、実App Server |
| 06 出力量 | command_statusへmax_output_bytesを追加。stdout/stderr合計で既定16KiB。初回exec/diff応答も同じ内部既定を使う | tests/usability.rs、実App Serverの8byteページ |
| 07 ジョブ一覧 | list_jobsを追加。job_id / state / exit_codeをID順に返す。既定50・最大200件、runtimeとlimitに結び付く継続 | tests/usability.rs、tests/transport.rs、実App Server |
| 08 名前・本文探索 | list_files / search_filesを追加。本文は既定literal、明示regex。固定argvで信頼されたripgrepを実行 | tests/usability.rsの実rg、実App Server |
| 09 編集結果 | Gitが解釈したpath・create/update/delete・予定追加/削除行数を保持。ファイル別にplanned / applied / not_applied / unknownを返す | tests/usability.rsの実Git、実App Server |

## 公開契約と上限

- 既存9ツールの名前・必須引数と基本envelopeを維持し、追加3ツールで計12とした。view_imageは引き続きoutputSchemaなしでnative imageとJSON receiptを配送する。
- read_file.max_bytesは1以上、信頼されたmax_message_bytes / 16以内。1行が予算を超える場合は明示的に拒否し、CLIのbyte範囲読取を案内する。ソース全体へのサイズ上限は設けず、要求範囲の後にある長大行・不正UTF-8を検証対象にしない。
- command_status.max_output_bytesの明示指定は8以上、信頼されたoutput_page_bytes以内。各streamへ半分ずつ割り当てる。保持量・保持期限・実行期限・受付記録の同一性はこの引数で変えない。従来設定の上限が8未満でも、引数省略時は実行済みの状態とbudget_limitedを保持する。
- 正常UTF-8のbyteを実際に返した分だけカーソルを進める。実行中の未完末尾と、終了時の不正bytes・保持先頭の欠損を分ける。gap / lossy / expired / source_truncatedを維持する。
- list_jobsは取得時点の一覧でsnapshotではない。コマンド本文や出力を載せず、control枠を使う。長いwork待ちが満杯でも既存の入力・停止・即時観測とともに応答できる。
- 検索は既定50・最大500件、結果配送64KiBまたはmax_message_bytes / 32の小さい方まで。1frameはmax_message_bytes / 16、走査時間はrpc_timeout_msまで。上限・読取失敗・非UTF-8の結果はtruncated / truncation_reason / partial / skipped_non_utf8で示す。
- 検索は既定でignore設定を尊重し、隠しファイルを除外し、symlinkを再帰追跡しない。明示pathの読取範囲は既存のhost-user権限に従う。子プロセスは上限到達・作業セッション終了時に停止して回収する。
- 編集対象は1件最大256ファイル。metadataは1編集につきmax_message_bytes / 32以内、作業セッション全体でedit_metadata_bytes_total（既定1MiB）以内。受付・変更前に容量を検査する。
- Git checkでの拒否とapply前の停止はnot_applied、apply成功はapplied、apply配送後の失敗・元App Server喪失はunknownとする。予定行数を実測の変更量とせず、同一request_idの再配送では元の結果を返す。出力期限切れやその後のファイル変更を再適用の理由にしない。
- 最終的にJSON・重複text・native image等を含むシリアライズ済み応答を配送上限と照合する。配送料不足を既知の実行失敗・未実行へ変換しない。

## 追加依存

- Rust実行時依存: `serde_path_to_error 0.1.20`。引数の問題箇所を取得する。Cargo.lockの追加crateはこれだけ。
- 検索機能: ripgrep。起動時の絶対PATH要素または信頼された設定の絶対search_executableから解決し、作業セッションで固定する。許可書込範囲・単発拡張の追加root内にある実行ファイルは使わず、読取操作を通じた実行認可の迂回を防ぐ。欠如・起動不能・能力検査失敗ではfeature_unavailableを返す。自動downloadやモデル指定の実行ファイルは追加しない。
- テスト専用: `tests/requirements-schemas.txt`の`jsonschema==4.26.0`。Draft 2020-12で公開schemaと実際のMCP応答を照合する。配備バイナリにPython依存は加わらない。
- Nixのruntime PATHとdevShell、CI/Releaseの検索試験環境へripgrepを追加した。CI/Releaseは実MCP応答のschema検証も実行する。

## 実施した検証

Rust 1.96.0 / Cargo 1.96.0を、無視対象の`target/model-tool-usability-build/`へ隔離して使用した。通常のシェル設定は変更していない。

| 検査 | 結果 | 確認範囲 |
| --- | --- | --- |
| cargo fmt --all -- --check | PASS | 全Rustソースの整形 |
| cargo clippy --locked --offline --all-targets -- -D warnings | PASS | 警告なし |
| cargo test --locked --offline --all-targets | PASS、41件 | unit 12、contracts 14、transport 3、usability 12 |
| cargo build --locked --offline --release | PASS | 最終releaseを生成 |
| tests/schemas.py（最終release） | PASS | 全12ツールの定義、型付き11ツールの実MCP応答36件、native image例外、誤ったstate/textの拒否 |
| tests/cli_ux.py（最終release） | PASS | guided setup、CLIのjobs/stop、再接続、設定保存・破損時拒否、更新・rollback fixture。公式Codex schemaとTunnel doctorも隔離HOMEで検証 |
| tests/connection_runtime.py（最終release） | PASS | 使い捨て接続設定・Tunnel fixture、有限retry、認証失敗、interrupt/EOF/終了時の子回収 |
| tests/shutdown.py（最終release） | PASS | fixtureと公式Codexで初期化中断、EOF、owner/process group終了、診断領域の削除条件 |
| tests/real_app_server.py（最終release） | PASS、77項目 | 実App ServerとmacOS sandboxの実際の挙動 |
| CI/Release YAMLとbash -n | PASS | CI 4本、Release 10本のrunスクリプトを構文確認 |

Rust/IPC試験では、エラー本文への入力値の混入防止、8byteページの非消費継続、極小の既存設定でも既知状態を保持すること、control枠の維持、空ファイル・終端・長大行の読取、検索のliteral/regex・glob・ignore/hidden・空白/日本語path・非UTF-8本文・symlink非追跡、配送/frame上限・依存欠如・時間上限・終了時の検索プロセス回収、書換可能な検索実行ファイルの拒否を確認した。

編集試験のfixtureは固定されたGit check/applyスクリプトを実Gitで実行し、新規・更新・削除・複数ファイル・引用された日本語/tabパス、check拒否、apply配送後の非ゼロ終了・通信断、metadata容量拒否、出力期限切れ後の同一受付再確認を検証した。checkを保留するfixtureでは、apply前の停止がnot_appliedとなり、ファイルが変わらないことも検証した。fixtureはsandboxの証明には使わない。

実App Serverは`codex-cli 0.166.0-alpha.6`、macOS 27.0.1で検証した。このバージョンだけに固定する契約は追加していない。実試験では書込範囲外・symlink経由の拒否、単発拡張、network拒否/明示拡張、Git index非変更、入力/停止ACK、PTY、再接続、native image 7形式、大きなテキスト/画像、64/256MiB出力、既定30秒の観測待ちと自動実行期限なし、終了時清掃も回帰確認した。

77項目の名前、検証したバイナリと全`src/*.rs`・CargoファイルのSHA-256は[real-app-server-results.json](real-app-server-results.json)へ保存した。実試験の開始・終了時にソースと実行ファイルが同一であることも検査している。メモリの記録値はこの試験時の観測であり、一般的な性能保証ではない。

## レビュー後の修正

独立した2エージェントの[規約・仕様レビュー](review.md)で、任意の検索実行ファイル欠如による起動失敗、不正なGit終了応答時の編集状態、UTF-16自動変換の3件を修正した。修正前の失敗を再現し、実Gitによるファイル変更後の不正応答と同一受付再確認、UTF-16のraw検索、検索なしでも他ツールを使えることを回帰試験へ追加した。再レビューの残る指摘は両軸とも0件。

## 実装検証時点で実施していない受入

通常Chatで同じモデル・ホストを使う成功率・往復回数・転送量の比較は未実施。引数説明や初期予算が最適であるとは主張しない。
後続の[作業ブランチ・mainのCI](deployment.md)で、macOS arm64/Intel・Ubuntu・Windowsの全4ジョブが成功した。Linux/Windowsの配備・sandboxを認定する試験ではない。Nix buildは未実施。
本番反映とpushは後続の明示指示で追加された作業であり、進行と結果は[切替記録](deployment.md)で追う。Releaseの新規公開は依頼されていない。

チケットのStatusは既存のtriage語彙を維持し、実装の完了は`Resolution: implemented`とコメントで記録する。
