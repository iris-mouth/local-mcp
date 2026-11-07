# 通常Chat向けMCPツールの使いやすさ

Status: ready-for-agent
Resolution: implemented

2026-11-07。本人が採用した9項目を実装。基準HEADは `2ee8bde6b64f30ef95e294faecc1352892893e3b`、準備コミットは `e9413786f9c5aff5b975699c9fcda22fbeaf0fb3`、作業ブランチは `codex/model-tool-usability`。
この記録は実装範囲と受入条件を定める。実装・検証結果は[implementation.md](implementation.md)、準備時点の記録は[baseline.md](baseline.md)を参照する。通常Chatでのモデル成功率評価は未実施。後続のマージと本番反映は[切替記録](deployment.md)で追う。

## 採用範囲

| 採用項目 | チケット | 主な変更箇所 |
| --- | --- | --- |
| UTF-8の文字境界を保つページ分割 | [01](issues/01-preserve-utf8-output-pages.md) | src/jobs.rs、契約・実IPC試験 |
| 引数の説明・既定値・短い例 | [02](issues/02-tool-argument-contracts.md) | src/mcp.rs、README.md |
| 原因と復帰方法が分かるエラー | [03](issues/03-recoverable-errors.md) | src/protocol.rs、src/session.rs、src/jobs.rs |
| ツール別の具体的な出力スキーマ | [04](issues/04-tool-output-schemas.md) | src/schemas.rs、src/mcp.rs、実MCP応答のschema検証 |
| 読み取り量の既定制限 | [05](issues/05-bounded-file-reads.md) | src/files.rs、src/protocol.rs、src/mcp.rs |
| 呼び出しごとの出力量指定 | [06](issues/06-command-output-budgets.md) | src/jobs.rs、src/protocol.rs、src/mcp.rs |
| MCPからのジョブ一覧取得 | [07](issues/07-mcp-job-list.md) | src/jobs.rs、src/session.rs、src/mcp.rs、src/rpc_capacity.rs |
| 検索・ファイル一覧ツール | [08](issues/08-file-search-and-list.md) | src/search.rs、src/config.rs、src/session.rs、src/mcp.rs |
| ファイル別の編集結果 | [09](issues/09-file-edit-results.md) | src/files.rs、src/jobs.rs、src/protocol.rs、src/mcp.rs |

## 今回含めないもの

- apply_patch形式への対応、独立したreplace_text、シェル実行専用ツール。
- 次のツール呼び出し候補の返却、作業環境・Git・主要コマンドの要約。
- 読取版IDと編集時照合、複数箇所の一括読み取り。
- テスト・コンパイラ診断パーサ、quick/full検証コマンドの登録。
- 「終了まで待つ」状態確認モード。現行の観測待ちと実行期限の区別を維持する。
- 本番配置、Tunnel・Keychain・ChatGPT登録の変更、Release公開。

## 既存契約との関係

- 根のCONTEXT.mdとADR-0001〜0005を使う。実行は元App Serverの公開command/exec系へ委譲し、通常ChatからCodexモデルへ判断を再委譲しない。
- 作業セッション、固定認可上限、許可書込範囲、基本実行と単発拡張の境界を維持する。
- session_id / runtime_idの照合を維持し、古いruntimeへ新しいジョブを結び直さない。
- 変更操作は既存Jobsと受付記録を使う。同じrequest_idと内容は元の状態を返し、変更内容の不一致は拒否する。
- 操作確認と実行認可は別。詳細な説明やannotationsを実行認可の代わりにしない。
- 実行終了、停止要求の受付、入力配送不明、実行状態不明、出力取得不能を区別する。出力が取れないことを実行失敗や未実行へ変換しない。
- 読取は現行のhost-user範囲。検索も読取操作であり、許可書込範囲を読取範囲として説明しない。
- 本文・出力・受付記録を永続化しない。新しい保持情報も有限かつ同じ作業セッションの生存期間内に限定する。
- native imageとJSON receiptを返すview_imageの既存ホスト互換を保持する。
- 既存の9ツール名・必須引数・成功/エラーの基本envelopeを維持する。list_jobs / list_files / search_filesを追加し、計12ツールとする。

## 実装した契約

以下の値をREADME・ツール定義・実装・契約試験に反映した。資源予算の初期設定であり、最適値やモデル成功率向上が実証された値ではない。

### 01 UTF-8とカーソル

- 有効なUTF-8の文字をページ境界だけで置換文字へ変換しない。元のbyte位置を保持し、実際に返した原文byte分だけカーソルを進める。
- 実行中に末尾の文字が未完なら、次の出力を待てるようにする。終了時まで未完、元から不正UTF-8、保持先頭の欠損は区別して示す。
- gap / lossy / expired / source_truncatedと既知の終了結果を保持する。小さすぎる配送量では明示的な制限を返し、進まないカーソルを無限に返さない。
- 日本語・4byte文字・CRLF・backendの通知境界・保持bufferのtrim・stdout/stderrの両方を確認する。

### 02 引数説明と03 エラー

- pathとcwdの基準、argvではシェル展開しないこと、wait_msは観測時間であること、stdinの既定closed、入力・停止ACKの意味をツール定義から理解できるようにする。
- request_idは同一依頼の再配送でだけ再利用し、結果不明時は元の依頼を確認することを短く説明する。文法例は頻出の誤用に絞る。
- 既存error.code / error.messageに、必要に応じfield・期待する型/範囲・復帰説明を追加する。入力本文・環境・秘密をそのままエラーへ転載しない。
- 実行前に拒否したと証明できる段階だけ未実行と示す。dispatch_unknown / result_unknownなどは元request_idの観測を案内する。
- 非ゼロ終了は成功したツール配送の中のコマンド結果として保持する。引数不正とは分ける。

### 04 出力スキーマ

- ツールごとに実際の返却フィールド、状態enum、nullableな終了コード、出力ページ、継続情報、エラーを宣言する。全てを単なる任意objectにしない。
- 公開スキーマと実返却の整合性を検証する。Rust型との二重管理を増やす場合はずれを検出する方法を備える。
- 成功・受付段階・実行中・終了・不明・拒否を検証し、view_imageは既存のoutputSchema無し/native image返却を回帰確認する。
- 今後の追加ツール・返却フィールドも各チケット内でこの契約へ反映する。

### 05 ファイル読取と06 コマンド出力量

| 対象 | 実装した既定値 | 上限と継続 |
| --- | --- | --- |
| read_file | max_lines省略時200行。追加のmax_bytesで本文の返却量を指定、既定64KiB | 信頼されたメッセージ由来の既存配送上限以内。next_line / has_moreを活用 |
| command_status | 追加のmax_output_bytesでstdout/stderr合計の本文返却量を指定、既定16KiB | 信頼されたoutput_page_bytes以内。既存の非消費カーソルを活用 |
| exec_command / apply_diffの初回応答 | 内部のstatus処理で同じ既定配送量を使う | 変更系の引数や受付記録の同一性へ配送量を追加しない |

- byte指定は元のUTF-8本文に対する量とし、JSON/重複text等の配送時の膨張も別に検査する。token数の保証とはしない。
- 省略時の既定配送量は信頼された上限で切り下げる。明示指定が上限を超えた場合は、黙って別の値へ変更せず引数エラーを返す。
- 保持buffer・出力期限・max_message_bytesは固定認可上限側の設定のまま。MCP引数で保持量を増やさない。
- 行単位読取はCRLFと元本文を保つ。1行が配送量を超える場合は本文を黙って欠かさず、明示的な制限と取得方法を返す。
- 範囲外の長大行・不正UTF-8を要求範囲の読取失敗にしない。ソースファイル全体への新しいサイズ上限は設けない。
- command_statusの既定の待機条件は変えず、返却量だけ変更する。既知の終了結果は出力不足・期限切れ後も残す。
- command_statusの明示max_output_bytesは8byte以上。従来の信頼されたoutput_page_bytesが8未満の場合、引数省略時は既知の状態とbudget_limitedを返し、配送後のジョブを未実行扱いにしない。

### 07 ジョブ一覧

- list_jobsはsession_id / runtime_id、任意のlimit・cursorを受ける読取専用ツールとする。
- 既存Jobs::summariesを基にjob_id / state / exit_codeを返す。コマンド本文や出力の一覧化・永続台帳は追加しない。
- limitは既定50、最大200とし、配送量上限も検査する。ID順のカーソルは同じruntimeと同じlimitに結び付ける。
- 一覧は取得時点の状態でありsnapshotではない。追加や状態変更があり得ることを示し、古いruntime・別条件のカーソルは拒否する。
- 有限のcontrol枠を使い、長いwork待ちが埋まっても同じ作業セッションのジョブを確認できるようにする。

### 08 検索とファイル一覧

- list_filesは名前・globによるファイル探索、search_filesは本文検索と用途を分ける。本文は既定literal、明示指定時のみregexとする。
- 既定pathは作業先。明示pathの検索も現行host-userの読取範囲と整合させる。symlinkは既定で再帰追跡しない。
- 無視設定と隠しファイルの既定挙動を説明し、必要な場合だけ明示的に変更できるようにする。binaryや非UTF-8の扱いも返却契約に含める。
- 結果にはパス、本文検索なら1始まりの行番号と本文を返す。limitは既定50、最大500、結果配送は64KiBまたはmax_message_bytes由来の上限の小さい方とする。
- 件数・配送量・時間に有限の上限を置き、打切りはtruncatedと理由で示す。初期実装で全体索引や検索snapshotを作らない。
- backendはrg。信頼された起動環境から解決した実行ファイルを固定argvで利用し、モデルへ実行ファイル指定やシェル文字列を公開しない。
- 検索用実行ファイルは許可書込範囲と追加rootの外に置く。モデルが実行ファイルを書き換え、読取ツール経由でサーバー権限の実行へ持ち込むことを防ぐ。
- rgは検索機能の追加依存としてREADMEへ明記した。起動環境のPATHまたは信頼された設定のsearch_executableから解決し、アプリ同梱の絶対パスへ依存しない。欠如時はfeature_unavailableを返し、自動downloadしない。
- 検索プロセスの出力・時間・終了処理を作業セッションが管理する。検索中もcontrolを塞がず、意味検索・言語解析・常駐indexは追加しない。

### 09 ファイル別編集結果

- apply_diffと既存Jobsの経路を使い、Gitが解釈した対象パス・操作・予定行数を受付記録に関連付ける。
- planned対象と確認済みのapplied / not_applied / unknownを分ける。予定行数を実測の変更量として表示しない。
- check段階の拒否・apply前の停止は未適用と示せる。apply配送後の非ゼロ終了や通信断は、個別の証拠なく全ファイル未適用と断言しない。
- Gitの成功結果は適用時点の確認であり、その後も現在ファイルが同一である保証にはしない。
- 同じrequest_idの再配送・MCP再接続・出力期限切れ後も、保持している結果を返し、変更を再適用しない。追加metadataの件数とbyte数にも有限の上限を設ける。
- 1編集の対象は最大256ファイル、metadataはmax_message_bytes / 32以内。作業セッション全体のedit_metadata_bytes_totalは既定1MiBとし、超過は受付・変更前に拒否する。

## 実装順と受入

1. 準備済みのRust 1.96.0環境と、fmt / clippy / Rust試験26件・fixture3本の基準を[baseline.md](baseline.md)で確認する。
2. 01を単独で修正し、日本語のページ分割・不正UTF-8・既存欠損表示を確認する。
3. 02 / 03 / 04で既存9ツールの入出力契約を整え、05 / 06の配送量制御を実装する。
4. 07、08、09を順に追加し、各段階で引数説明・返却スキーマ・README・実IPC試験を更新する。
5. 通常Chatで同じモデル・ホストを使って、引数修正の往復、転送量、誤編集、重複実行、検索から読取への到達を比較する。fixture成功をモデル成功率の証明にしない。

全体の回帰対象は、受付の重複防止、範囲外・symlink書込拒否、古いruntime拒否、元App Server喪失、入力/停止ACKと実終了の区別、出力欠損/期限切れ、control枠、native image、既存CLI・接続管理・終了処理。
Rust/fixture成功、実App Serverでの境界試験、通常Chatでの受入は別々に結果を記録する。新機能はfixtureだけで本番受入済みとしない。

## 準備時点の判定（履歴）

- 採用9項目と対象外、基準HEAD・ブランチ、個別チケット・依存順・受入条件が追える。
- 実装コードはまだ変更せず、基準ソースと既存実行ファイルの対応、実施/未実施の検査をbaselineへ記録する。
- 初期値・検索backend・schema検証方法は実装上の案として区別し、実装結果に合わせて更新する。

## Comments

- 2026-11-07: 本人の「それで行きましょう。作業ブランチを作り、実装前の準備を進めてください。」に基づく。採用9項目以外の機能はこの準備に混ぜない。
- 2026-11-07: 本人の「実装してください。」に基づき9項目を実装。Rust 40件、公開定義12ツールと実応答36件のschema検証、既存Python fixture 3本、実App Server 77項目を最終releaseで確認。通常Chat評価と本番配置は別の未実施事項として[実装記録](implementation.md)に明記した。
- 2026-11-07: 後続の「レビュー、試用、マージ、本番反映まで進めてください。」により、上記の実装時の対象外だったマージ・配置・既存接続への反映を追加で実施する。機能範囲は9項目のまま。独立したレビューで3件を修正し、Rust 41件・実App Server 77項目を修正版で再確認した。[レビュー](review.md)と[切替記録](deployment.md)を参照。
- 2026-11-07: 作業ブランチとmainの4環境CIが成功し、mainへの統合・push・新runtimeの既定化を完了。本人のKeychain確認後、実Tunnelと同じ本番SessionのMCPで試用し、ChatGPT公開一覧の12ツールへの更新、未終了ジョブ0件での正常終了と回収を確認した。新runtimeは次回起動待ち。[切替結果](deployment.md)を参照。
