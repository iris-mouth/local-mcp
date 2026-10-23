# 通常Chat受け入れテスト

Date: 2026-10-23 (Asia/Tokyo)
Status: completed including fixed view_image blind visual acceptance; legacy 10-tool catalog restored; all test connections stopped

Latest follow-up: 修正版view_imageを新9ツールとして通常Chatで検証し、content_items=[text,image]から画像出力が成功。未見の5図形すべての色・形・相対位置が、回答後に読んだ正解manifestと一致した。修正版の視覚受入は合格。詳細と最新の復元状態は[画像診断記録](image-diagnosis.md)末尾を参照。以下の未達記録は修正前の履歴である。

Follow-up: 旧get_imageの画像はcontent_items[0]に保持され、画像出力helperへ渡すと未見の5図形を認識でき、本人許可後の正解照合で全件一致した。旧画像経路の視覚受入は合格。一方、新view_imageへcatalogを一時更新した再試験では、通常ChatへstructuredContentと同じメタデータだけが公開され、content/content_itemsが無く視覚受入は未達。詳細と復元状況は[画像診断記録](image-diagnosis.md)を参照。以下は元の受入時点の記録を保持する。

## 現時点の結果

通常Chatで、新9ツールの公開、session/runtime照合、CRLF行範囲読取、差分と重複抑止、非ゼロ終了、pipe入力と重複抑止/EOF、停止ACKと実終了、限定拡張と権限非継続、固定上限/別session/旧runtime/依頼ID競合/symlink diff/Git context不一致の拒否を確認した。

画像はnative MCP画像応答まで成功したが、通常Chatモデルが図柄を認識できず視覚受入は未達。facade切断でTunnel clientも終了し、既存運用による手動Tunnel再起動後に同じruntimeと元jobの既知終了情報を取得できた。自動再接続を合格とはしていない。

新実装のテストowner/Tunnelは終了し、専用診断領域も正常cleanup済み。本人のKeychain確認後、共有プラグインは旧10ツールへ復元し、通常Chatで旧10の列挙と旧session_infoの実呼出しを確認した。旧テスト接続も終了し、既存GBrain Tunnelだけが残りreadyz=200。本番binary/profile/helper/start scriptとレビュー済みソースのhashは不変。旧get_imageでも同じfixtureの図柄は認識されず、新実装固有の原因とは断定しない。

## 対象

- Source commit: `c712cc4e5a35e51904009a9d06bb1f36af8bc764`
- Tested binary SHA-256: `068b83de3cf0c6faef324db436731402027451fe1a167d763015010654a101b6`
- Session: `chat-uat-be0d9b8ebc5a4fba`
- Runtime: `5b79c71a-dcf7-42ba-82f9-04aac94142ca`
- Disposable project: `/private/tmp/local-mcp-chat-acceptance-sxjd1vm3/project`
- Disposable expanded-root ceiling: `/private/tmp/local-mcp-chat-acceptance-sxjd1vm3/outside`
- Test launcher: `/private/tmp/local-mcp-chat-acceptance-sxjd1vm3/run.py`
- Test runtime: `/private/tmp/local-mcp-chat-acceptance-sxjd1vm3/runtime`
- Ordinary Chat: https://chatgpt.com/c/6adb1cea-a160-83ec-ae60-82ea97f56eb0

## 分離

既存chatgpt-mcpのrun-tunnel.pyとlocal-mcp-stdio.pyを変更せず一時runtimeへコピーした。既存local-workspace profileはcommandのstdio wrapperパスだけを一時runtimeへ変更し、同じTunnel、認証helper、公式clientを使用する。新しいTunnel/Keychain項目や常駐サービスは追加していない。本番バイナリ・起動ファイル・GBrain設定は変更していない。開始前のプロセス確認ではlocal-workspaceは停止、GBrain用Tunnelのみ稼働していた。

テスト設定はmax_jobs=2、限定拡張rootは使い捨てoutsideだけ、network/full/SSH/Git metadata拡張は無効。読取は従来どおりhost-user権限。画像はランダムな幾何図形を生成し、Chatへの回答前に表示しない。正解manifestはfixtureとは別の位置に保存し、Chatには渡さない。

## 実施記録

以下は時系列の記録。途中の「未完了」「待機中」は当時の状態であり、最終状態は冒頭と末尾を参照。

## 観測

- ownerとApp Serverの起動成功。固定runtimeを確認。
- テストTunnelの`/healthz`と`/readyz`はHTTP 200。
- 通常Chatでlocal-workspaceを添付した新規Chatを作成し、session_infoのみを依頼した。初回は接続準備前に`MCP SSE probe returned 429 from openai.org`となった。変更操作は依頼していない。
- 当該Chatは旧10ツールを列挙した。新実装9ツールとのcatalog同期は未確認。
- health/ready確認後、同じChatからsession_infoを1回再依頼し成功。返却されたruntime、cwd、基本2 roots、追加root上限、network/full無効、backend_available=true、active_jobs=0が実際の起動と一致した。

## 未完了

新9ツールの通常Chatへの公開、読取・diff・exec・status・入力・停止・限定拡張・拒否・再接続・画像視覚利用・操作確認。未完了をローカル実機66項目の成功へ読み替えない。

## ツールcatalogの差分と停止理由

既存プラグイン管理画面:
https://chatgpt.com/settings/plugins-settings/plugin_asdk_app_6abe1312e0b0819197da242e74d7d440

旧10ツール: `session_info`, `read_file`, `get_image`, `list_directory`, `write_file`, `execute`, `start_command`, `poll_job`, `stop_job`, `without_sandbox`。

新9ツール: `session_info`, `read_file`, `view_image`, `apply_diff`, `exec_command`, `exec_command_expanded`, `command_status`, `write_stdin`, `stop_command`。session_info以外はruntime_idを必須とする。

変更の影響は、この既存local-workspaceプラグインを使う他のChatにも及ぶ。プラグイン権限設定・認証・Tunnel ID・本番バイナリ・GBrainは変更対象にしない。

管理画面の「ツールを更新」は自動承認レビューに拒否され、実行されていない。理由は、共有プラグイン設定の変更がテスト用セッションの許可を超え得ること、影響範囲と復元方法が未提示だったこと。別経路で更新を試していない。

## 更新前snapshotと復元案

テストbaseに以下を保存した。認証キーは含めていない。

- `installed-tools.json`: 既存の配置済みバイナリからtools/listで取得した旧10ツールのschema。
- `candidate-wrapper-tools.json`: 新実装を既存stdio wrapperへ通したtools/listの新9ツールのschema。これはローカルMCP検査であり、通常Chatへの新ツール公開成功ではない。
- `installed-file-hashes.json`: 本番binary/profile/helper/start scriptのhash。

許可を得た場合の実行案は、テスト中だけ既存プラグインの「ツールを更新」で新9ツールを取得し、同じ通常Chatまたは新しいテストChatで受入を行うこと。試験終了後はテストsupervisorへ`stop`を送り、そのowner/Tunnelの終了を確認する。既存の`~/.local/share/chatgpt-mcp/start-local.command /private/tmp/local-mcp-chat-acceptance-sxjd1vm3/project`を起動して旧バイナリの専用セッションを一時的に提供し、同じ管理画面で旧10ツールを再取得してsnapshotと照合する。その旧テストセッションもCtrl-Cで終了する。既存ファイルを上書きする復元ではない。

現在のテストsupervisorはCodexのexec session `8501`で稼働中。操作は`status`/`stop`。将来の別プロセスへPIDだけで結び直さない。

## 一時更新の本人許可

共有local-workspaceの旧10→新9ツールへの一時更新と、試験後の旧10への復元について、本人が「一時更新と試験後の復元を許可する」と回答した。対象・他Chatへの影響・本番バイナリ/権限/認証/GBrainは不変であること・snapshotと復元手順を提示した上での許可。許可後に同じ管理画面の「ツールを更新」を実行し、ボタンの処理中→有効への復帰を観測。通常Chatの再読込後、新9ツールの公開確認とread_file/view_imageの実利用を依頼中。

## 通常Chatで確認済みの操作

- 新9ツールを実際に列挙。read_fileがruntime付きで成功し、readme.txtの1〜2行、CRLF、next_line=3/has_more=trueを確認。
- view_imageはok=true、image/png、4329 bytes。モデルは画像本体を認識できず、色・図形数・位置を未確認と報告したため視覚受入は未達。別途ローカルの同じstdio wrapper応答にはtextとnative image blockがあり、decodeした4329 bytesはfixtureと一致することを検査した。これは通常Chatの視覚成功ではなく、切分け証拠に限定する。
- apply_diffのuat-diff-1はbefore→after、exit=0。同一ID/diffの再配送は同じjob `8a81b8e3-dd61-44fc-a953-9485cda11e3b`。read_fileとMac側実ファイルでもafterを確認し、Git index hashは初期値と一致した。
- exec_commandとcommand_statusのuat-nonzeroはok=true、exit=7、stdout=out、stderr=errを維持。job `4459289b-4241-4bd2-9052-04b2a0269727`。
- pipe catのjob `89b833c2-cf97-4a7e-a0f4-68ecfe159109`に同じuat-inputを2回要求したが、stdoutはCHAT-PIPE-ONCEの1回分だけ。EOF後にexit=0、input_closed=true、input_delivery_unknown=false。
- uat-reconnectのsleep job `9300331e-0c9a-43e6-ae67-2096d0b76acd`を起動。テストTunnelの配下関係と実行パスを照合してfacade wrapperだけをSIGTERMし、その子facade終了を確認。元owner/App Server/Tunnelは保持したまま、通常Chatへ再接続後の状態観測と停止、旧runtime拒否を依頼中。
- 本番runtime5ファイルのhashは引き続き初期snapshotと一致。

## facade切断後の接続運用

切断したfacade wrapperの終了に伴い、テストTunnel clientもOnStop hookを実行して終了した。通常Chatのsession_infoは約2分後に`Tunnel-client did not poll after this request was enqueued`で失敗し、Chatは指示どおり新規実行や停止を行わなかった。元owner/App Serverを再作成した証拠はない。テストclientだけを既存run-tunnel.pyで手動再起動し、health/ready=200を確認して読取専用の再確認を1回依頼した。これは自動復旧の成功ではなく、既存運用による手動Tunnel再起動の試験である。

再起動したテストTunnelはexec session `32379`。試験終了時はこのclientを停止した後、元supervisor `8501`へstopを送り、owner/App Serverの終了確認・専用診断cleanupを確認する。

## 再接続・停止・固定上限の通常Chat実測

- 手動Tunnel再起動後、通常Chatのsession_infoで元runtime `5b79c71a-dcf7-42ba-82f9-04aac94142ca`を再取得し、元uat-reconnect jobのfinished/exit=0を確認した。起動し直したApp Serverへジョブを再結合した試験ではない。
- fresh sleep job `421d74e0-9cc5-4ceb-a169-3679497c2e74`のstop_commandはaccepted、その後command_statusでfinished/exit=137。ACKと終了を別々に確認した。
- 旧runtimeのread_fileはscope_mismatch/runtime_mismatchで拒否。
- 基本execによるoutside/basic-denied.txtとproject/escape/symlink-denied.txtはexit=1/Operation not permitted。通常Chatではsandbox内の存在検査が不存在/アクセス不可の区別を付けないことを明示。Mac側で直接、実ファイル未作成を確認。
- 明示した限定拡張uat-expanded job `30b34cdd-50c5-4c18-b5e0-e82fe0cceb66`はexit=0、outside/expanded.txt=expandedをread_fileとMac側で確認。直後の基本execによるoutside/after-expanded-denied.txtは再び拒否され、権限は継続しない。
- fullとnetwork:trueはauthorization_deniedでdispatch前拒否。
- プラグイン管理画面の既存権限モードは「低リスクの操作を許可」。変更していない。別モードでの毎回確認/本人拒否は今回の成功に含めない。

## 終了前の追加確認

通常Chatへ、同じrequest_idで異なるargvの拒否、新規symlink diffの拒否、Git context不一致でapplyしないこと、別session_idの拒否を各1回依頼中。共有catalogは、本人許可に沿って試験後に旧10ツールへ戻す。復元後には同じ画像を旧get_imageで読み取る比較だけを行い、新実装固有の問題とhost側の視覚経路を区別する材料とする。画像の図柄・正解manifestは引き続きChatへ提示しない。

## 最終拒否テストの結果

- request_id=uat-nonzeroと異なるargvはrequest_id_mismatch。新規jobなし。
- new file mode 120000のsymlink diffはdiff_rejected / patch target must be a regular text file。通常Chatはこの操作を重要と見なし確認画面を表示した。テスト操作者（本agent）が「一度だけ許可」を選択した後でもサーバーが拒否し、操作確認と実行認可を区別できた。「常に許可」や権限設定変更は行っておらず、本人の手動クリックを証明する試験でもない。Mac側でもnew-symlinkのlexists=false。
- Git context不一致のjob `b14b772d-6d28-43f7-aa24-319bb5b8ff3d`はok=true/finished/exit=1。read_fileとMac側でedit.txt=after、Git index hash不変。
- 別session IDはwrapperのUse the session_id shown in the active Mac terminalで拒否。
- 新実装の全9ツールの実呼出しは実施できたが、view_imageの図柄認識は未達。MCP image blockが存在することだけで視覚受入成功とはしない。

## 新実装セッション終了・復元状況

- 再起動したテストTunnel（exec32379）を終了し、元supervisor（exec8501）へstopを送った。owner exit=0。元ownerが管理していたlocal-mcp-runtime-CUAdkmディレクトリの消失を確認し、通常終了後の専用診断cleanup成功。
- 旧10ツール復元用に既存start-local.commandを使い捨てprojectで起動。旧Session IDは18fa181d-b06c-4228-af36-8b56e18f1dc3、exec sessionは73598。
- 旧ownerは起動済みだが、Tunnel起動helper配下の/usr/bin/securityがKeychain確認待ち。旧Tunnelのhealth/readyはまだ未接続。SecurityAgentの操作はComputer Useに安全上禁止されており、別経路でcredentialを取得せず本人へ一度だけ許可を依頼した。
- この時点では共有catalogはまだ新9ツール。旧10への更新ボタンは、旧Tunnelのreadyを確認してから実行する。
- ここまで本番binary/profile/helper/start script5ファイルとレビュー済みRustソースのhashは不変。

## 画像応答の比較材料（原因は未確定）

同じfixture画像をローカルstdioで比較すると、旧get_imageはcontent=[image]でstructuredContentなし、新view_imageはcontent=[text,image]とstructuredContentを返す。双方のnative imageは4329 bytesで同一SHA-256。画像bytesの差ではなく返却構造やhostへの受渡しの差を今後切り分ける材料だが、これだけで原因を断定しない。旧get_imageの通常Chat視覚比較はKeychain確認待ちのため未実施。

## Keychain確認後の復元再開

本人が「一度だけ許可しました」と回答。旧Tunnelのhealthz/readyzがともにHTTP 200となったことを確認した。明示許可済みの旧10ツールへの復元として管理画面で「ツールを更新」を実行し、処理中から有効への復帰を観測。通常Chatを再読込し、旧10ツールの列挙、旧Session IDでのsession_info、旧get_imageによる同一fixtureの盲検比較を依頼中。

## 旧10ツール復元の通常Chat確認

Keychain確認後に旧Tunnelがreadyとなり、共有catalogを旧実装から更新した。通常Chatの再読込後、旧10ツールを実際に列挙し、新view_image/apply_diff/exec_command等が公開されていないことを確認。旧Session ID 18fa181d-b06c-4228-af36-8b56e18f1dc3でsession_infoを1回呼び、同じ使い捨てprojectとの一致を取得した。旧get_imageで同一fixtureの画像視覚比較を実施中。

## 最終確認（2026-10-23）

- 旧10ツールの復元後、通常Chatはsession_info/read_file/get_image/list_directory/write_file/execute/start_command/poll_job/stop_job/without_sandboxを列挙し、新view_image等が公開されていないことを確認した。旧Session IDのsession_infoも成功した。
- 同じfixtureを旧get_imageで1回取得したが、通常Chatは図柄を認識できず未確認と報告した。画像受入は新旧とも未達。新実装だけのregressionまたはhostの仕様と断定せず、native MCP imageからモデルへの受渡しを次の切分け対象とする。画像の正解manifestはChatへ渡していない。
- 旧復元用のexec session73598をCtrl-Cで終了（起動scriptの想定された終了code130）。新旧のlocal-mcpと試験用Tunnel clientが残っていないことを実プロセス一覧で確認した。
- 開始前から稼働するGBrainのTunnel PID32053だけが残り、readyz=200。GBrain設定・認証は変更していない。
- 本番runtimeのbin/local-mcp、profiles/local-workspace.yaml、run-tunnel.py、local-mcp-stdio.py、start-local.commandのSHA-256は更新前snapshotと一致。レビュー済みRustソースのhashも実機66項目の検証記録と一致する。
- 本番バイナリの置換・配置・切替・pushはしていない。共有plugin catalogは一時更新後に旧10へ復元済み、権限モード「低リスクの操作を許可」も変更していない。
- 使い捨てprojectとschema/hash/メタデータの切分け証拠はテストbaseに保持。モデルの画像視覚受入を除く主な機能は今回の設定で確認した。別の権限モード、hostでの本人拒否、最大配送量、長期負荷は今回の成功に含めない。
