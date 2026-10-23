# 本番切替結果 — 2026-10-24

本人の「本番切り替えを進めてください」に基づき、新ランタイムの配置と既存接続の切替を完了した。

## 配置と登録

- 本番配置: `~/.local/share/local-mcp`。
- 起動入口: `~/.local/share/local-mcp/start-local.command /absolute/project/path`。引数なしの場合は作業先を入力する。
- 初回配置時のバイナリSHA256: `01cdb2db923a03b3a20ab414968aed4e6a24451fc7a806da64f46f03525011b2`。初回レビュー用releaseと一致。Astraレビュー後の修正版は末尾に記録する。
- 既存公式Tunnel clientと同じ公式Codex CLIを利用し、同じTunnel IDとKeychain項目を参照する。
- 既存ChatGPTアプリの表示名をlocal-workspaceからlocal-mcpへ変更。アプリID `asdk_app_6abe1312e0b0819197da242e74d7d440`、version ID、権限「低リスクの操作を許可」、認証方式は維持した。ツールは既存9ツールのままで、不要な再登録・定義更新は行っていない。
- 名前変更は既存の設定画面から保存し、画面上のlocal-mcp表示を確認。[設定画面](production-plugin.jpg)。

## 実接続での確認

起動前の旧local-workspace用owner/clientは不在で、GBrain用clientだけが稼働していた。新runtimeをこのlocal-mcpリポジトリを作業先として起動し、以下を既存の接続済みMCPツールから実Tunnel経由で確認した。

| 対象 | 結果 |
| --- | --- |
| health / ready | どちらもHTTP 200 |
| session_info | Session・runtime・cwd一致、既定の実行認可、backend available |
| read_file | READMEの6行を取得 |
| exec_command | `/usr/bin/true`の既知終了・exit 0 |
| 許可範囲外書込 | 通常実行で拒否、exit 1、対象ファイルの未作成をMac側でも確認 |
| apply_diff | `.scratch/connection-runtime/production-smoke.txt`をbeforeからafterへ変更、exit 0、Mac側で実内容を確認 |
| view_image | JPEGのnative image blockとスコープ付きreceiptを取得・表示 |
| Tunnel再起動 | 同じSession/runtimeを維持し、元sleep jobの既知終了exit 0を再取得。コマンド再実行なし |
| 正常終了 | Ctrl-C、owner exit 0、所有socket・診断領域・health/PIDファイル・client/facadeの消失を確認 |

Tunnel再起動時にmacOS Keychainの確認が発生し、最初の読取専用status要求はタイムアウトした。本人が「今回のアクセスを許可した」と回答した後、ready 200と元runtime/jobの終了結果を取得した。本人のKeychain確認を含む手動のTunnel再起動確認であり、無人の再認証成功とは扱わない。自動再起動上限の検査は隔離fixtureで行っている。

通常Chatの別会話へプロンプトを送る試験は行っていない。このCodexチャットの接続済みツールを通じた実Tunnelの受入であり、通常Chatモデルの画像視覚受入とは区別する。

## 最終状態と復旧

作業時だけ接続する運用に合わせ、検証Sessionは正常終了した。本番runtimeは次回の起動待ち。起動後は端末に表示されるSession IDを通常Chatへ渡し、最初にsession_infoで作業先を確認する。

旧`~/.local/share/chatgpt-mcp`のbinary、client、launcher、helpers、local-workspace/GBrain profile、GBrain plistはSHA256一致で保持した。GBrainのreadyは最終確認でもHTTP 200。共有Keychain項目の変更、GBrainの停止・再起動、公式Codex CLIの変更は行っていない。

復旧は新接続の実終了後、旧`~/.local/share/chatgpt-mcp/start-local.command`を作業先指定で起動し、必要なら表示名を戻す。[手順](migration.md)。元ジョブの自動復旧・再配送は含まない。

詳細な識別子と検査結果は[production-results.json](production-results.json)。初回切替記録の作成時点ではソース変更はmainの未コミット状態だった。既存の未追跡`.DS_Store`を保持し、移管元repoの変更・削除や旧共有runtimeの撤去は行っていない。

## Astraレビュー後の修正版

本人のレビュー・条件付きコミット指示を受け、Astraが検出したP2（leader単独クラッシュ時の所有グループ残留）を修正した。実装基準・仕様の再レビューは追加指摘なし。[レビュー記録](review.md)。

最終releaseの接続回帰、21テスト、終了fixture、fmt/clippy、公式App Serverの66項目が成功した。停止中であることを確認し、既存の新runtimeを`~/.local/share/local-mcp-before-astra-20261024-80102f20`へ退避して、同じ最終パスで新規生成した。現在の本番バイナリSHA256は`7e3737e3b27b98ee54cdb8130b3956b191a42d0214bc184311c416efc23c234d`。

Tunnel ID、client、公式CLI、Keychain参照は一致。今回の回収処理修正では実TunnelやKeychainを再起動せず、配置は停止状態で保持した。前述の実接続受入は初回配置の記録であり、修正版の回帰は隔離fixtureと公式App Serverで確認した。[修正版配置結果](post-review-results.json)、[最終releaseの公式App Server結果](post-review-real-app-server-results.json)。

修正前の新runtimeへ戻す場合は、稼働中の新接続がないことを確認し、現在の`local-mcp`ディレクトリを別の未使用名へ退避してから、backupを`local-mcp`という元の名前へ戻す。埋め込み絶対パスがあるため、backup名のままlauncherを起動しない。従来のchatgpt-mcpへの復旧手順も維持する。
