# local-mcpの接続・起動・配置機能移管

2026-10-24。本人が登録先・ランタイム名のlocal-mcpへの切替、Tunnel管理の移管、Rust統合と最小の起動シェル、作業開始に連動する自動起動を採用。

## 範囲

- `prepare`で未使用の配置先に実行バイナリ・公式Tunnelクライアント・profile・起動入口・ハッシュ記録を生成する。キー取得、サービス起動、本番上書きを伴わない。
- `connect`で本人が選んだ作業先の既存Session所有プロセスを起動し、App Serverの初期化成功後にTunnelを開始する。
- Tunnelの通信は既存公式client、認証情報の保管は既存macOS Keychainを利用する。キーを読む処理は専用の子プロセスからclientへexecし、Session所有プロセス、MCP facade、App Server、実行ジョブへキーを渡さない。
- 新しい常駐daemonを作らず、作業開始からCtrl-C/EOFまでが接続の生存期間。既存の`start`/`mcp`も維持する。
- Tunnelだけの異常終了は回数制限つきで再起動し、元Session/App Server/実行ジョブを維持する。認証取得失敗は自動再試行しない。上限後は接続停止を表示し、端末から明示再起動できる。
- 既存Tunnel IDとKeychain項目`chatgpt-mcp-tunnel`を利用。新ランタイムは独立した公式clientのコピーを持ち、共有GBrain配置へ依存しない。
- 同じユーザーとTunnel IDの新ランタイムの重複起動は排他ロックで拒否。ロック非対応の旧ランチャーは切替時に稼働を確認して本人と停止する。
- GBrain固有ファイル・LaunchAgent・Chat指示・Skill/pluginの移管は別プロジェクトの担当。

## 言語・依存

本体・準備・接続管理は既存Rust crateに実装し、新しいcrateを追加しない。Finder用`.command`はRust CLIをexecするだけ。配備した実行経路のPython依存を外す。許可書込範囲・単発拡張・実行認可・Session所有・診断の既存契約を保持する。

## 受入条件

1. 隔離先への生成、既存配置への上書き拒否、権限、引用、実ファイルのSHA256を確認する。
2. 公式clientへ渡す環境とMCP起動時のキー除去をダミー値で検査し、実際のキーを試験に使わない。
3. 同一Tunnelの排他、起動失敗・有限再起動・明示再起動・Ctrl-C/EOFでの終了と回収をfixtureで確認する。
4. 既存の許可ルート、範囲外・symlink越境拒否、Session固定、App Serverの実行と終了の回帰検査を行う。
5. 本番切替前に、生成物と手動切替・復旧手順を提示する。旧共有ランタイムとGBrainを実装段階では変更しない。
6. 実Chat/Tunnel/Keychainでの受入は隔離fixtureや直接App Server検査と区別して記録する。

## 設計記録

ADR-0005がADR-0001/0003のchatgpt-mcpへの起動管理委譲部分を更新する。旧設計・受入結果は当時の記録として保持する。
