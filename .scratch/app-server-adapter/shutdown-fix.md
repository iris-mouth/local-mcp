# Ctrl-C終了処理の修正 — 2026-10-23

## 要求と原因

本番切替試験で発生した終了警告とlocal-mcpの残存を修正する。既存の権限設定・認証・GBrainは変更しない。

原因は2つあった。

1. App Serverが所有local-mcpと同じ端末プロセスグループに属し、Ctrl-CのSIGINTを直接受けた。所有セッションが停止要求とstdio EOFを送る前に元App Serverが終了し、正常終了を確認できない警告が出ていた。
2. `tokio::io::stdin`のキャンセル不能なblocking readが残り、作業セッションの終了処理が戻った後もTokio runtimeの終了を妨げていた。

公式App Server・開いたPTYを使った最小再現では、プロセスグループへのSIGINTで警告と13秒超の残存を確認した。所有local-mcpだけへのSIGINTでも警告なしで13秒超残存した。標準入力処理だけを修正すると単独SIGINTは約0.03秒で終了し、グループSIGINTでは残存が解消して警告だけが残った。この比較から2要因を独立に確認した。

## 修正

- `start`の入力待ちを専用の標準スレッドと有限のchannelへ移し、未完了の端末入力がruntime終了を妨げないようにした。EOFと`/status`の既存動作は維持する。
- SIGINT受信をschema検査後・App Server起動前に登録し、初期化待ちとready直後のCtrl-Cにも対応する。初期化中断・失敗時も元stdioを閉じて子の終了を有界に待つ。
- 元App Serverを別プロセスグループへ置き、端末Ctrl-Cを所有セッションが受ける。既存のジョブ停止、元stdioのclose、子のwaitという終了順序を維持し、警告条件や診断領域の削除条件を緩めない。
- `tests/shutdown.py`に開いたPTYでのグループSIGINT、単独SIGINT、EOF、実行中ジョブありのグループSIGINTを保存。ready直後に信号を送り、終了コード・警告・所有PID消滅・IPC/lock削除・診断領域削除を検査する。

## 検証

- 修正前の最小再現: グループSIGINTは警告・残存、単独SIGINTは残存で失敗。
- 入力待ち修正後、信号伝播修正前の保存回帰試験: 警告を検出して失敗。
- `cargo fmt --check`、`cargo clippy --locked --offline --all-targets -- -D warnings`、`git diff --check`: 成功。
- `cargo test --locked --offline`: 14契約 + 3実MCP/IPCテスト、17件成功。最初のsandbox内実行はIPC bind拒否でtransport 3件が起動できず、Mac実IPCで同じテストを再実行して成功した。
- `LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/shutdown.py`: fixtureの4経路成功。
- 同試験に公式CLIを指定: 4経路成功、各終了約0.01秒、警告なし。実行中sleepのPID・App Server・所有プロセスの終了、診断領域・socket・lock削除を確認。
- `LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/real_app_server.py <公式CLI>`: macOS実機66項目成功（公式CLI `0.163.0-alpha.10.2`）。通常Chatの視覚試験は今回再実施していない。
- 本番の`profiles/local-workspace.yaml`、`run-tunnel.py`、`local-mcp-stdio.py`、`profiles/gbrain-memory.yaml`のSHA256は切替前記録と一致。認証・Keychain・GBrainは変更していない。

本番配置や共有ツール定義はこの修正作業では更新していない。以前の異常終了で残った診断領域を推測で削除していない。

## Astra初回レビューと追補

`95354d7`の仕様軸は指摘なし。実装基準軸で、SIGINTを起動の最初に登録しながら受信をready後までpollしないため、初期化待ち中のCtrl-Cが遅れるP2指摘を受けた。

schema検査中は従来の端末信号動作を維持し、その後に受信を登録する。初期化とSIGINTを同時に待ち、中断時には分離したApp Serverのstdioを閉じ、子を回収してからエラーを返すよう修正した。初期化失敗時も同じ回収経路を使う。

遅延初期化fixtureを追加。`95354d7`を一時領域から再ビルドしたバイナリは2秒の中断期限で失敗し、追補版では即時中断・子PID消滅を確認した。元の4終了経路と合わせてfixture/公式CLIの各5経路が成功した。

追補後にfmt、clippy、17テスト、公式App Server受入66項目を再実行して成功。[最終実機結果](shutdown-real-results.json)に最終ソース・バイナリのSHA256を保存した。
