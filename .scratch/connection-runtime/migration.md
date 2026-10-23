# 接続ランタイムの切替・復旧手順

## レビューする生成物

`local-mcp prepare`は、新しい配置先に次を生成する。

- `bin/local-mcp`: Rust本体。prepare/connect/start/mcpを含む。
- `client/tunnel-client`: 指定した既存公式clientのコピー。
- `runtime.json`: schema version、既存Tunnel ID、公式Codex CLIの絶対パス、Keychainのservice/account、コピー後の2バイナリのSHA256。
- `profiles/local-mcp.yaml`: ループバックの動的health port、通常のinfoログ、キーを除去して起動するstdioコマンド。
- `start-local.command`: Rust CLIをexecする起動入口。
- `state/`, `logs/`: 接続時のhealth/PIDファイルとclientログ。

キー取得、既存配置上書き、client起動、OS自動起動登録はprepareに含まれない。Keychain項目の作成・更新機能は移植せず、今回の移管では既存`chatgpt-mcp-tunnel`を参照する。GBrainとの共有項目は改名・複製・削除しない。

生成物には絶対パスを埋めるため、レビュー用runtimeを別の場所へ移動して本番にしない。本番用の最終パスでprepareを再実行する。途中失敗時は部分生成物が残る場合があり、同じ場所へ自動再適用せず内容を確認する。

## 本番切替前

1. レビュー用runtime.jsonのSHA256と実ファイル、profileのstdioコマンド、launcherの実行先を確認する。
2. 新バイナリの隔離検証結果と公式App Serverの書込範囲・symlink越境拒否・Session固定・終了の検査結果を確認する。実Chatの接続成功とは区別する。
3. 旧起動入口`~/.local/share/chatgpt-mcp/start-local.command`が起動した作業セッションとlocal-workspace用clientの稼働状況を確認する。実行中ジョブがあれば本人と終了時点を決める。新ランチャーの排他ロックは旧Python/Shellランチャーを止める機能ではない。
4. 旧共有runtimeのlocal-mcp binary、launcher、local-workspace profileと共有プラグイン表示名・ツール・権限設定を記録する。旧runtimeは復旧用に保持する。

## 切替

本人が本番切替を確認した後に行う。

1. 最終配置先`~/.local/share/local-mcp`が未使用であることを確認し、レビュー済みrelease、既存公式client、同じTunnel ID、同じ公式Codex CLIを指定してprepareを行う。既存ディレクトリがあれば上書きせず別の新規配置先を検討する。
2. 旧local-workspace用接続だけを、その起動元の端末から終了する。GBrainの接続、LaunchAgent、プロセス、共有Keychain項目、公式Codex CLIには変更を加えない。
3. 新launcherを使い捨ての作業先で起動する。必要なKeychainダイアログは本人がmacOSで確認する。
4. Session readyに加え、新clientのhealth/readyを確認する。既存ChatGPT接続のTunnel IDと権限設定を維持して、表示名をlocal-mcpへ変更する。ツール定義は同じ9ツールで、差異がある場合だけ内容を確認して再取得する。
5. 本人が指定した通常ChatでSession IDと作業先の一致、読取・画像・編集・コマンド実行と既知終了、範囲外拒否を確認する。実Chatへプロンプトを送る場合は送信先と試験範囲の許可を確認する。
6. Tunnelだけの切断後に同じruntimeと元jobを観測できること、Ctrl-CでSession/App Server/clientが終了することを確認する。元コマンドの再実行で確認を代用しない。
7. 初回は旧runtimeを残したまま結果を記録する。旧repoの削除・共有資源の撤去は別の作業。

## 復旧

1. 新launcherの端末でCtrl-Cを送り、元Sessionとclientの実終了を確認する。停止受付だけで旧接続を重ねて起動しない。
2. 旧`~/.local/share/chatgpt-mcp/start-local.command`を作業先を指定して起動する。新しいSession IDとruntimeが生成される。元の実行ジョブを復旧・再配送する手順ではない。
3. 既存プラグインの表示名を必要に応じて元のlocal-workspaceへ戻し、Sessionと接続を確認する。ツールは同じ9ツールを維持する。
4. 新配置は停止状態で調査用に保持する。Keychainの書き戻しやGBrainの停止・再起動は不要。

## 運用上の境界

- 自動起動は作業開始に連動する。Macログイン時の常駐接続は登録しない。
- Tunnel client終了時の自動再起動は1/2/4秒後の計3回まで。同じ起動Sessionの生存期間中の合計回数で、明示`/tunnel-restart`でリセットする。
- Keychain取得・helper起動エラーは自動再試行しない。ログとmacOS側の状態を確認した後、`/tunnel-restart`を使う。
- `Session ready`はローカル実行基盤の準備完了。ネットワーク接続準備の証拠はclientのready結果で確認する。
- 新CLIは保存PIDからプロセスを制御せず、起動時から所有するclientと同じプロセスグループを制御する。異常終了後の診断・グループを離脱した任意の子孫・owner強制終了後の残留は、実際の状態を確認する。
- 所有するTunnelのleader終了は`waitid(WNOWAIT)`で観測し、同じグループの接続用プロセスを終了させてからleaderをreapする。leaderだけの異常終了やTERMを無視する接続用の子も回収対象とし、別グループへ離脱した任意の子孫はこの保証に含めない。
