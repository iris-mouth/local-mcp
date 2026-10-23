# App Serverを実行基盤にしたlocal-mcp

Design status: agreed
Implementation status: implemented; local acceptance passed; ordinary Chat acceptance pending
Updated: 2026-10-23

2026-10-23、本人のQ17回答「確定します。」により、実行契約・依存方針・受入条件を含む設計全体を合意した。
実装時に検証・調整する項目を、確定した契約と区別して記録する。2026-10-23の本人指示「実装してください。」により実装を開始した。進捗・実測・未実施の受入は[実装検証記録](implementation.md)に分けて記載する。

## 確定した方針

- local-mcp本体をRustの薄いMCPアダプターとして設計する。
- 判断は通常Chatのモデルが行い、App Serverの公開`command/exec`系へ実行を委譲する。
  Codexモデルを呼ぶ`thread`/`turn`への再委譲は行わない。
- MCP接続・Tunnel・Keychain・起動運用は既存のchatgpt-mcpが管理する。
  local-workspaceとGBrainを維持し、chatgpt-mcpのPython/ShellのRust化は対象に含めない。
- 日常の操作確認はChat内で行う。サーバーの実行認可と区別し、モデルの`approved`値やyoloを認可の代用にしない。
- 既存の認証、起動対象セッションへの限定、子プロセスへの接続キー非継承、書込範囲修正の効果を維持する。
  モデル指定のcwdや書込先から許可書込範囲を増やさない。
- モデルへ少数の用途別ツールを提示する。App Server初期化、RPC ID、通知処理、base64処理をモデルへ渡さない。
- 短い実行は結果、長い実行はjob IDを返し、状態と新出力をまとめて取得できるようにする。
- 画像は通常Chatが実際に視覚利用できることを受入条件にする。
- 編集にはGit等の既存CLIを利用し、独自の編集エンジンを増やさない。
- 実行状態不明と出力取得不能を区別する。出力を取り直すために変更操作を自動再実行しない。
- 新しい接続、Tunnel、Keychain項目、GUI/Electronを作らない。

### 第1ラウンドの本人決定

本人の回答「すべて推奨どおり」により、次を確定した。

- Q1: 接続・認証・起動運用を維持した上で、MCPツール名と入出力は刷新してよい。
  旧ツール互換層を要件に含めない。
- Q2: ジョブ状態と出力の再取得は、生存中の実行管理プロセスに限定する。
  出力には容量・期限の上限を設け、再起動をまたぐ永続ジョブ台帳や自動復旧は作らない。
- Q3: 必須受入対象はmacOS。Linux・Windowsの対応保証は別途判断する。

### 第2ラウンドの本人決定

本人の回答「すべて推奨どおり」により、次を確定した。

- Q4: 固定認可上限の中で、既存の認証済MCP接続から届く要求を実行する。
  操作確認はChatホストの設定に従い、サーバーは作業セッション・許可root・機能上限を検査する。
  毎回の本人クリックをサーバーが証明する仕組みは作らない。
- Q5: 作業先外を含めてホストユーザー権限で読める、現行の読取範囲を維持する。
  読取範囲と許可書込範囲は別の概念として扱う。
- Q6: 基本ツールは次の8用途とする。名前は仮であり、詳細schemaは未確定。
  ディレクトリ一覧・検索は既存CLIを利用する。

| 用途 | 仮のツール名 |
| --- | --- |
| 作業先・許可範囲の照会 | `session_info` |
| テキスト読取 | `read_file` |
| 画像の視覚利用 | `view_image` |
| 差分編集・ファイル作成／削除 | `apply_diff` |
| 短時間／長時間の実行 | `exec_command` |
| 状態と新出力の取得 | `command_status` |
| 実行中プロセスへの入力 | `write_stdin` |
| 停止要求 | `stop_command` |

### 第3ラウンドの本人決定

本人の回答「すべて推奨どおり」により、次を確定した。

- Q7: 通信・追加書込先・sandbox全面解除を単発拡張として提供する。
  通常実行は通信なし・基本root内とし、別ツールで1ジョブごとに必要な拡張を明示する。
  Mac側で許可していない拡張は拒否し、追加root・通信・全面解除は既定で無効とする。
  全面解除はfilesystemとnetworkのsandbox制限を外す別モードであり、限定rootを維持する指定とは扱わない。
- Q8: MCP接続だけが切れた場合もジョブを継続し、再接続後に状態と保持中の出力を取得できるようにする。
  App Server接続とジョブ管理は既存`local-mcp start`が所有し、MCP facadeは既存セッションIPCを利用する。
  同じ起動セッションと元App Serverが生存中の契約とし、新daemonや再起動後の復旧を追加しない。

### 第4ラウンドの本人決定

本人の回答「すべて推奨どおり。並行可能上限は変更可能にしましょう。」により、次を確定した。

- Q9: 同じセッションを使うChat同士でジョブの照会・入力・停止を共有する。
  並行実行上限は既定4とし、Mac側の信頼された設定で変更できるようにする。
  4は設計上の初期値であり、性能試験による適正値・CPU数・子プロセス数の保証ではない。
  専用の差分編集は同じ作業セッション内で直列化する。
  任意コマンド・別セッション・外部writerによる同じファイルへの変更は自動調停しない。
- Q10: 実行・編集・入力へ依頼IDを付け、同ID・同内容は再配送せず元の受付状態を返す。
  同ID・別内容は拒否する。受付記録は起動中に保持し、出力の期限切れでは削除しない。
  受付記録の容量に達したら、新規受付を拒否して重複防止を維持する。
- Q11: 標準unified diffをGitで検査し、成功した場合だけ同じpatchを一度適用する。
  hunkの行数表記はGitで再計算できるが、内容の推測修正・自動再適用は行わない。
  Git indexは変更しない。適用前検査をロックやI/O障害・強制終了時の巻戻し保証とは扱わない。
- Q12: 入力は既定closedとし、必要時にpipe/PTYを選ぶ。後続入力は元ジョブの権限を継承する。
  非ゼロ終了も終了結果として返し、ツール自体の失敗と区別する。
  停止要求の受付と実際の終了確認を分ける。
  終了不明は当該ジョブに記録し、管理基盤が生存し実行を受け付けられる場合は、別ジョブを容量内で受付可能とする。
  入力配送不明となったジョブでは追加入力を止め、照会・可能な停止を残す。

### 実行管理の境界

同じ`start`の新しい実行開始を有限の`max_jobs`で制限する。満杯なら新規開始を即時拒否し、queueや自動的なjob種別分類を追加しない。
専用編集のcheck→applyは1つの論理ジョブとして枠を使う。job枠とは別にRPC受付・入力・出力等へ有限の上限を設ける。
job満杯だけを理由に、状態照会・入力・停止・作業先照会・直接ファイル読取を拒否しない。
終了を確認していないジョブは枠を解放しない。

| 観測状態 | 採用した契約の適用 |
| --- | --- |
| MCP接続だけ切断、元管理基盤は生存 | 元jobの観測を継続する |
| 終了結果が既知で出力だけ取得不能 | 既知の終了を維持し、新規受付可能 |
| 停止要求後も元接続で観測中 | `stop_requested`として観測を続け、待ち時間超過だけで不明にしない |
| 入力配送だけ不明 | 当該jobの追加入力を止め、終了結果とは別に保持する |
| 単一jobが不明、元管理基盤は生存・受付可能 | 不明jobの枠を保持し、別jobは残り容量内で受付可能 |
| App Serverの元stdioを喪失 | 終了未観測の全jobを不明とし、新規実行は基盤利用不能として拒否する。既知の完了結果・保持出力は維持する |

元接続を失った後の停止は配送不能と返し、別App Serverや推測PIDへ送らない。
基盤喪失時の自動再起動、別プロセスへの再接続、変更操作の再実行を追加しない。
再起動後の永続job復旧や、別start・外部writer・独立daemonの全域調停は保証しない。
今回は起動・負荷・切断・停止試験を実施していない。

### 第5ラウンドの本人決定

本人はQ13・Q16を推奨どおり採用し、Q14・Q15について
「設定値は現在のlocal-mcpの設定から変更する意義がある場合のみ、変更しましょう。」と指定した。
以前提示した数値を一括採用したものとは扱わない。

- Q13: 信頼された開始処理で作業先、Git metadata候補、専用tmp、実行環境を解決・表示する。
  Git metadataは本人が許可したものだけを通常write rootsへ含める。
  commandの環境は利用者HOME、信頼されたPATH、locale、専用TMPDIR等の有限allowlistを基礎とし、
  既存SSH agentの再利用はMac側で明示した場合だけにする。
  App Server自身の環境・保存先と利用者commandの環境を分ける。
  モデル指定cwd・env・GIT_DIRからrootを増やさず、新Keychain項目や認証情報を作らない。
- Q14/Q15: 現行値と意味を維持できるものは維持する。
  新しい保持機能やMCP配送・memoryの有限化に必要な変更は、変更理由と受入条件を添えて選定する。
  実測根拠のない数値をApp Serverの必須値・既存設定値・性能合格値として扱わない。
- Q16: local-mcp自身は本文・入出力・受付記録を永続化しない一方、公式App Server診断への本文保存を例外として認める。
  専用保存先を表示し、不要な外部送信を無効にし、通常終了と元App Serverの終了を確認した後に専用管理領域を削除する。
  異常終了で残留した場合は場所を表示し、旧processの終了を推測した自動削除や他用途の保存先の削除は行わない。
  `history.persistence=none`等をSQLiteを含む本文非保存の保証とは説明しない。
  旧製品での例外合意を、本案件の合意へ流用しない。

### 現行値との比較と変更方針

mainと配布指定patched commitで下表の値・挙動は同じ。
「明示上限なし」は本体に制限がない意味で、OS・memory・接続経路まで無制限という保証ではない。

| 項目 | 現行 | 新設計での方針・変更意義 |
| --- | --- | --- |
| foreground待ち | 固定30秒 | 既定30秒を維持。明示的な待ちなし開始も残す |
| 実行期限 | 明示上限なし | 自動的な10分終了等を導入しない。App Serverにも期限無効を明示し、server defaultを黙って継承しない |
| UTF-8・改行 | UTF-8、CRLFを含む元の改行 | 維持。不正UTF-8の黙った置換や改行正規化をしない |
| 画像形式 | PNG/JPEG/GIF/WebP/BMP/TIFF/AVIF | 7形式を維持して通常Chatで検証する。根拠なく3形式へ狭めない |
| 実行出力 | stdout/stderr全量収集、途中取得なし | 新しい途中取得・再取得用bufferに有限のjob別/全体容量を設ける。source全寿命のcapture capと混同しない |
| 結果・出力保持 | TTLなし、完了pollで削除 | 再取得のため非消費cursorへ変更。完了後の出力には有限の保持期限を新設する。既知の終了結果と受付記録は出力期限と別に保持する |
| 受付記録 | 機構なし | 重複配送を防ぐため件数・依頼ID長等を有限化。dispatch前に満杯を拒否し、受け付けた依頼を忘れて再実行しない |
| text読取 | UTF-8全file、行/byte/source上限なし | 全量読取の意味を無根拠に200行へ変更しない。必要時の行範囲と、明示的な返却量制御を追加する。元file全体へ一律10MiB上限は設けない |
| 画像・MCP byte上限 | 明示上限なし | base64膨張・JSON overhead・採用MCP経路の実測限界に基づき選定する。5MiB等を現行値として扱わない |

新しい出力buffer・ページ・保持期限・受付件数の具体的初期値は、既存設定の変更ではなく新機能の資源設定。
有限性、Mac側での設定可能性、超過時の動作を設計契約とし、初期値は実装時の代表負荷と配送限界の検証に基づいて決める。
非消費cursorの読取でjobを削除せず、保持bufferの欠落と元captureでの切詰めと期限切れを区別する。
`outputBytesCap`はApp Serverではstreamごとの全実行寿命のcapであり、1応答のページ量や合算buffer量の代わりに使わない。
streamingと`disableOutputCap`により既存のsource出力を維持し、アダプター側で有限化する候補を対象版で検証する。
これらのfieldが採用バイナリで使えるか、backend側memoryが増え続けないかは今回未検証。

## 確定した実行契約

以下はQ1〜Q16を具体化し、Q17で本人が確定した操作・認可・結果の契約。
新しい資源設定の具体的初期値は、明記した変更意義と受入条件に基づき実装時に選定する。
本人の実装開始指示があるまではコード・依存・実行設定を変更しない。

### 依存と実行基盤

- MCPは公式Rust SDKのrmcpを採用し、protocol negotiation・schema・content・annotation・stdioを利用する。
  非同期処理とJSONにはTokioとSerde/serde_jsonを用いる。実際のcrate版は実装時に固定し、今回はCargoを変更しない。
- Codex内部Rust crate群へのgit依存を解消し、App Serverの公開JSON-RPCの必要部分だけを扱う。
  送信methodは`initialize`、`initialized`、`command/exec`、同`write`/`resize`/`terminate`に限定する。
  受信する出力通知と元execの完了応答を分け、`process/*`の終了通知を混用しない。
- App Serverの通常ユーザー設定を暗黙に継承して実行権限を決めず、固定認可上限から生成したpolicyを実行ごとに明示する。
  system/managed要件に反する場合は拒否を返し、迂回設定やsandbox解除への自動再試行を行わない。
- 専用保存先・有限環境・不要な送信の無効化・認証cacheを複製しない起動を、対象版で検証する。
  モデルAPI呼出し、login、新しいKeychain項目の作成をcommand実行の前提にしない。
- 配置に使うApp Server版と必要method/field、境界の試験結果を対応付ける。
  未確認版への自動更新、内部crateへのfallback、未対応fieldの黙示的無視を前提にしない。
  exact version比較だけで互換性の合否を決めず、必要な挙動の検証を条件とする。

### 共通入力と識別

- schemaは未知fieldを拒否する。モデル入力の`approved`、yolo、Chat ID、raw sandboxPolicyを認可に使わない。
- 全toolに既存wrapperが検査する`session_id`を含める。session_info以外には、session_infoが返す不透明な`runtime_id`も含める。
  同じsession名で起動し直してもruntime_idを更新し、旧job・旧依頼を新起動へ結び直さない。
  runtime_idやjob_idは認証資格情報の代わりではない。
- 実行・編集・入力にはclient側の`request_id`を要求する。これは操作の照合用で、App ServerのRPC IDではない。
  同一runtime内のIDと正規化した内容digestをdispatch前に予約し、同ID同内容は元の結果・受付状態、別内容は不一致を返す。
  同じcommandでも新しいrequest_idは別操作であり、内容だけで同一意図と推測しない。
- 受付記録と終了metadataは出力期限では削除しない。有限の件数・サイズに達したら新規変更要求をdispatch前に拒否する。
  新規受付拒否中も既存依頼の照会・可能な停止を残す。再起動をまたぐ重複防止を保証しない。

### ツール表面

共通fieldを除く主要入力と出力を記す。path・入力bytes・diff・RPC件数等の有限上限は信頼された起動設定で検査し、session_infoに有効値を返す。

| tool | 主要入力 | 主な結果・副作用 |
| --- | --- | --- |
| `session_info` | session_id | runtime_id、作業先、基本root、固定認可上限、read範囲、設定上限、基盤状態、公式診断保存先 |
| `read_file` | path、任意start_line/max_lines | UTF-8本文、実際の行範囲、続きの有無。通常fileを直接読む。明示した返却量で収まらない場合は続きまたは限界を返し、黙って欠落させない |
| `view_image` | path | native MCP image contentとpath/MIME/bytes等のmetadata。既存7形式を対象とし、base64本文をtextへ渡さない |
| `apply_diff` | request_id、標準unified diff | 作業先に対するGitのcheckと一度のapplyを1論理jobとして扱う。job状態、適用結果、Git出力を返す |
| `exec_command` | request_id、argv配列、任意cwd/wait_ms/input_mode/PTY size | 基本root・通信なしで実行。wait_msの既定は30000、0なら即job ID。既定の自動実行期限は付けない |
| `exec_command_expanded` | 通常実行の入力、reason、access | 固定上限内の追加root/通信、またはfull accessを元jobへ固定する。開始結果へ実効範囲を返す |
| `command_status` | job_idまたはrequest_idの一方、任意cursor/wait_ms | 状態、終了結果、保持中の新出力、次cursor、欠落・期限等を一度に返す。新しい実行を開始しない |
| `write_stdin` | job_id、request_id、data/EOF/resizeのいずれか | UTF-8入力または端末サイズを元processへ配送。元権限を継承し、受付と入力配送不明を返す |
| `stop_command` | job_id | 元processへの停止要求と観測状態。繰返し呼出しは同じ停止試行を参照し、別processへ配送しない |

- shell解釈は暗黙に挟まず、既存と同じargv実行を基本にする。shellが必要な依頼はargvでshellを明示する。
- 通常実行と単発拡張のcwdはcanonical化し、full access以外ではそのjobの許可root配下だけを受け付ける。
  モデルのcwdをApp Serverへ直送してworkspace rootを拡張しない。
- 単発拡張のaccessは「選択した追加rootとnetwork」の限定モードか「full access」の排他的な指定とする。
  full accessと限定root/network=falseを併記し、制限を維持したように説明しない。
- 専用編集は作業先内のtext fileを対象とし、同じdiff bytesと固定Git optionsでcheck→applyを行う。
  `--recount`による行数再計算は許すが、index更新、reject file生成、3way、内容の推測補正、自動再適用を行わない。
  追加rootでの任意編集が必要な場合は、明示した単発拡張のコマンドで既存CLIを使える。
- 読取は書込rootに制限しない。通常file確認、読取中の増大にも効く配送量検査、UTF-8境界・改行保持を行う。
  画像の未対応・不正データ・配送限界を成功として扱わず、通常Chatの視覚利用で検証する。
- 任意のshell環境指定が必要な場合はargvの`env`等で明示できる。MCPからApp Server自身の環境や固定上限を変更するfieldは出さない。

### 結果・エラー・出力

- MCPのstructuredContentをモデル向けの正規結果とし、text contentにも識別子・状態・重要な結果を読める形で返す。
  image contentは別contentとして付ける。App Serverのraw RPC、内部診断、接続キーを結果に混ぜない。
- 結果にはschema_version、ok、適用できるsession/runtime/job/request識別子、状態・終了code・出力情報を含める。
  コマンドの非ゼロexitはokなツール実行の終了結果であり、副作用がなかったことを意味しない。
- 引数不正、認可拒否、ID不一致、容量超過、未対応能力、読取失敗、基盤利用不能は安定したtool errorとして返す。
  dispatch後に結果を失った場合はnot_startedと断定せず、実行状態不明を添え、自動再送を促す汎用retryable値を付けない。
- jobの受付・実行中・停止要求中・終了・不明を区別する。実行中は元execの応答を観測している状態で、プロセスの副作用開始を証明する意味ではない。
  停止ACK、無出力、状態照会の待ち時間超過だけで終了済み・不明へ変更しない。
- 入力ACKはApp Serverの受付結果で、対象アプリによる消費完了ではない。
  入力配送不明はjobの終了結果と別に保持し、当該jobの新しいdata/EOF/resizeを止める。
- 出力はstdout/stderrを区別する。PTYでstdoutへ多重化されることを明示する。
  cursorはjobとruntimeとstream位置へ結び付ける。読取で消費せず、古い位置の欠落は最初に残っている位置とともに返す。
  非UTF-8出力を置換表示する場合はlossyを明示し、元bytesのoffsetと混同しない。
- 出力が期限切れ・破棄・切詰めでも、既知の終了結果を不明や失敗へ変更しない。
- annotationは用途に合わせる。照会・読取はreadOnlyHint、任意実行・編集・入力・停止は変更可能として扱う。
  通信可能な拡張実行にはopenWorldHintを付ける。任意実行を一律idempotentとは宣言しない。

### 起動・終了と受入条件

- startは固定設定、起動世代、同じApp Server stdio、受付記録、job/出力を所有する。
  MCP facadeとセッションIPCは長いexec待ちでstatus/input/stopを塞がないように扱う。
- 同じ作業セッション名の同時起動でsocketや状態を上書きせず拒否する。再起動は新しいruntimeとして扱う。
- start終了は管理jobへ停止を要求し、元App Serverの終了を有界に確認する。
  期限内に確認できなければ未確認と表示し、管理processの終了を全子孫・独立daemon・全副作用の停止保証へ広げない。
  公式診断の削除は所有する専用領域と確認済みの通常終了に限定する。
- 実装後の受入には、現行30秒/即backgroundの意味、無期限実行、起動session限定、旧runtime/別session拒否、接続キー非継承を含める。
- 実App Serverで、root内書込成功、範囲外・symlink・外部cwd経由の拒否、本人許可したroot追加後の成功を確認し、旧patched回帰条件を維持する。
- job満杯でもstatus/input/stopが進むこと、複数jobの出力が混ざらないこと、同依頼の再配送でdispatchが増えないこと、MCP再接続で元jobを観測できることを確認する。
- 元stdio喪失、stop ACKだけ、入力ACK喪失、出力期限切れ、Git不一致・外部変更・I/O失敗を別条件として検証する。
- 通常Chatから読取・差分編集・実行・入力・停止・単発拡張の確認と拒否を行う。
  画像はMIMEやbytesの応答だけで合格にせず、未見の図柄の内容を通常Chatが正しく説明できることを確認する。
- 新しい資源設定の初期値は、代表作業と大出力、長寿命job、画像のbase64/JSON膨張、RPC並行時の応答・memoryから選ぶ。
  採用値・現行との差・変更意義・検証結果を記録する。実装中の都合だけで既存形式や実行期限を狭めない。

設計確定時点では上記の受入はすべて計画であり、実行済み試験ではなかった。実装後の実施結果と未実施項目は[実装検証記録](implementation.md)を参照する。
2026-10-23に、既存コードとの整合と公開API契約との整合を別々に読取確認し、重大な事実不整合は見つからなかった。
これは実App Server・Git・通常Chatの結合試験の代わりではない。

### 採用した実行経路

```mermaid
flowchart LR
  Chat[通常Chat] --> Tunnel[既存Tunnel]
  Tunnel --> Wrapper[既存stdio wrapper]
  Wrapper --> MCP[local-mcp mcp]
  MCP -->|既存セッションIPC| Start[local-mcp start]
  Start -->|生存中のstdio接続| AppServer[Codex App Server]
```

これは設計上の経路であり、現在のコードへ実装済みという意味ではない。

## 設計時に確認した基準

- このworktreeのコード基準は`21025d048f54cc9f948c26ac42fa36183dc453c2`。
  設計確定時点では、設計文書以外のコード・依存・実行設定は変更していなかった。
- 書込範囲修正`0c2dd3bf4940ea427dac185872032df5a34ccbb3`は、
  `codex/session-write-boundaries`の`1439d36`に保持されている。
  設計確定時点のworktreeには未収録だった。今回の実装では、同修正の書込境界と回帰条件を公開App Server上で維持する。
- chatgpt-mcpの`local-mcp-version.json`も上記patched commitを指定する。
  stdio wrapperは起動時セッション以外の`tools/call`を拒否し、接続キーを子local-mcpへ渡さない。
- 現行local-mcpの読取はホストユーザー権限で行い、書込rootによる読取制限はない。
  現行の完了pollはジョブを削除し、途中出力・cursor・入力の契約はない。
- App Serverの`command/exec`はthreadなしで実行できる。
  短時間結果と長時間jobの切替、出力保持はアダプター側の責務になる。
- `writable_roots`はcwdへの追加rootとして説明されている。
  固定rootと任意cwdの直送では境界を保証できず、アダプター側のcwd検査も必要になる。
- 現行公式資料と保存済schemaにはread制限field等の差がある。
  公式資料のfieldが採用バイナリで利用できるとは、今回の読取だけでは確認できない。
- 現行ジョブの所有者は`local-mcp mcp`であり、`start`ではない。
  MCPプロセスが終了・再生成されると、`start`が生きていても元ジョブの状態は参照できない。
- 同じ`start`がApp Serverの元stdio接続とジョブを保持し、既存セッションIPCでMCP要求を中継すれば、
  新daemonや外部接続を追加せずMCP再接続後の観測を設計できる。
  これはソースと公開契約からの設計推論であり、今回の実機確認ではない。
- `workspaceWrite`は固定追加write rootとnetwork有効化を併用できる。
  `dangerFullAccess`ではfilesystemとnetworkのsandbox制限を解除するため、限定rootを維持する指定とは区別する。
- 同じ作業セッションのChatを、信頼できるChat IDで独立分離する仕組みは現経路にない。
  Thin Aの複数workspace間の排他を、独立した各`start`間の排他へそのまま移すこともできない。
- `git apply --check`は適用可否の検査で、対象ファイルのロックではない。
  通常applyはhunk不一致でpatch全体を拒否するが、I/O障害や強制終了まで含むロールバック保証ではない。
- 公式rmcpの型と例にはstdio server、structuredContent、outputSchema、image/MIME、annotationがある。
  採用する依存版と実際の結合は未確認。
- 現行read_file/get_imageはRust直接読取であり、新設計でもhost-user readを維持するためにhelperを追加する必要はない。
  行範囲、通常file検査、読取中の増大にも効くbyte上限、base64後のMCP応答量は別途制限が必要。
- 公式資料の専用CODEX_HOME/sqlite_homeは保存先の分離であり、本文非保存の保証ではない。
  旧版の検証報告ではhistory none/RUST_LOG off等でも専用診断SQLiteにdummy commandが残った。
  これは2026-10-21の当時版の観測であり、現行版の再実測ではない。

## 設計ツリー

- 実行境界: App Server公開command系、通常Chatの判断、Rust、固定認可上限を採用済み。
  - 読取はホストユーザー権限の範囲を維持する。
  - 通信・追加書込範囲・sandbox全面解除は、既定offの単発拡張として提供する。
  - Q13: 信頼された起動時のGit metadata root・tmp・環境・認証資源再利用の方式を採用済み。
- MCP操作: 旧schemaの互換維持は不要。基本8用途を採用済み。
  - 専用編集はGitによる事前検査と一度の適用を採用済み。
  - 実行・入力・停止の結果契約を採用済み。
  - Q14/Q15: 現行値・形式を維持し、変更意義のある資源制限だけを検証して選定する。
  - 具体的な入力・出力・annotationは、確定した実行契約に記載済み。
- 実行管理: プロセス再起動後の永続復旧は対象外。
  - 同じ`start`と元App Serverが生存中はMCP再接続後の観測を提供する。
  - 同一セッションのChat間共有と起動中の受付照合を採用済み。
  - 同一セッションの共有、可変の並行上限、編集の直列化、終了不明時の扱いを採用済み。
  - 非消費cursorと有限の出力・受付保持を設計し、具体的初期値は実装時の検証で選定する。
- 依存と運用:
  - rmcpと公開JSON-RPCを使う依存方針を採用済み。具体的crate版は実装時に固定する。
  - Q16: 公式診断保存の例外と分離・通常終了後の削除を採用済み。
  - 採用App Server版の必要method/fieldと境界挙動の検証条件を確定した実行契約へ記載済み。
- 受入:
  - 通常Chatでの読取・画像・編集・短時間/長時間実行・入力・停止と拒否条件を確定した実行契約へ記載済み。
  - 既存テストや旧製品のfixtureの存在を、通常Chatの合格に読み替えない。

Q17により設計全体の共有理解を確認済み。設計確定後、2026-10-23に本人から実装開始の指示を受けた。
新しい資源設定の具体的初期値とcrate版は、明記した選定基準に従う実装時の調整項目であり、既存挙動を無根拠に変更する許可ではない。

## 設計確定時点の範囲と確認状態

設計確定コミット`29a32a1`では、要件、用語、設計判断、必要最小限の設計文書を扱った。
その段階では実装、Cargo依存、実行設定、ビルド、App Serverやアプリの起動、配置、切替、Issue/PR作成、push、公開は行わなかった。
設計確定までには、実行・結合・通常Chatの受入試験は行っていなかった。

## 根拠

- [実行基盤のADR](../../docs/adr/0001-app-server-execution-boundary.md)
- [操作確認と固定認可上限のADR](../../docs/adr/0002-chat-confirmation-and-execution-authority.md)
- [作業セッションによる実行管理のADR](../../docs/adr/0003-session-owned-execution.md)
- [公式診断保存のADR](../../docs/adr/0004-official-diagnostics.md)
- [公式App Server仕様](https://learn.chatgpt.com/docs/app-server#command-execution)
- [公式設定例](https://learn.chatgpt.com/docs/config-file/config-sample)
- [公式ツール設計](https://developers.openai.com/plugins/plan/tools)
- [Git公式apply仕様](https://git-scm.com/docs/git-apply)
- [公式Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
- `codex/session-write-boundaries:src/mcp.rs`と`src/sandbox.rs`の修正・回帰条件。
- `/Users/mitsuhiro/Documents/Codex/chatgpt-mcp/local-mcp-version.json`と`runtime-templates/local-mcp-stdio.py.in`。
- `/Users/mitsuhiro/Documents/Codex/chatgpt-chat-local-workspace/src/thin-a/`は契約の参照のみ。
  独立製品は放棄済みで、helper・Electron・独自接続管理・永続復旧等を丸ごと移植しない。
  同repoの`docs/cleanup-2026-10-22.md:40`では通常Chatの画像利用は未達、編集・実行・入力・停止は未実施。
