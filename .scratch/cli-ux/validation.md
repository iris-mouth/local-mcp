# CLI UX実装の検証

2026-10-27。既存worktreeのmain、実装前HEAD `5cb5a7d5cdcb793773893616ffe3c4cb3e6120db`を基準とする。既存の未追跡`.DS_Store`は変更していない。

## 結果

| 検査 | 結果 |
| --- | --- |
| `cargo fmt --check` | 成功 |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 成功 |
| `cargo test --locked --offline --all-targets` | 単体9・契約14・実IPC3、計26成功 |
| `cargo build --release --locked --offline` | macOS arm64で成功。新規crateなし |
| release実行ファイルによる`tests/cli_ux.py` | 初回setup、設定保存、workspace選択、ジョブ一覧・停止、終了取消・同意、状態不明、再接続、更新失敗時の旧設定維持、破損拒否、引数なし初回起動が成功 |
| CLI試験の並行性・回収 | 遅いHTTP検査中と遅いTunnel停止中もCLI/IPCが応答。再接続中の終了でclient・状態ファイルを回収。元runtime/job維持を確認 |
| CLI試験の公式依存検査 | 実際の公式Codexによるschema検査と公式Tunnel clientのdoctorが成功。専用一時HOME・placeholderキーを使い、実キーや実Tunnelは使用していない |
| `tests/connection_runtime.py` | 排他、有限再試行、認証失敗時の自動再試行抑制、明示復旧、元ジョブ維持、interrupt/EOF/TERM/HUPの回収が成功 |
| `tests/shutdown.py` | fixtureで成功。公式Codexでも起動中断、グループ/ownerへのSIGINT、EOF、実ジョブ実行中の終了同意と回収が成功 |
| `tests/real_app_server.py` | 最終releaseで66項目成功。最終ソース・実行ファイルのハッシュ一致を試験内で確認 |
| 配布物のsmoke検査 | arm64の本体・README・LICENSEをtar.gz化し、SHA256検証、展開後の`--help`/`--version`が成功 |
| workflow構文 | Ruby YAMLによる両workflowの読取と、全runステップの`bash -n`が成功 |
| `git diff --check` | 成功 |

公式App Serverの結果・バージョン・ソースと実行ファイルのハッシュは[real-app-server-results.json](real-app-server-results.json)に記録した。

## 再実行方法

通常の開発環境で、READMEのVerifyを実行する。公式依存を含むCLIセットアップの検査は次の形で実行できる。

```sh
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/cli_ux.py /absolute/path/to/codex /absolute/path/to/tunnel-client
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/shutdown.py /absolute/path/to/codex
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/real_app_server.py /absolute/path/to/codex .scratch/cli-ux/real-app-server-results.json
```

この環境ではRust 1.96.0とキャッシュを`/private/tmp/local-mcp-cargo`・`/private/tmp/local-mcp-rustup`から使用した。初回の制限環境ではUnix socket bindが拒否されたため、ソケットを使う試験は一時領域・fixtureに限定したsandbox外実行で検証した。自動承認レビューによる拒否はない。

初回の公式受入実行は試験中のソース変更を末尾のハッシュ検査で検出し、無効になった。最終ソースを固定した実行で66項目とハッシュ検査を通過し、上記JSONを生成した。既存接続試験は、新しい設定読込が実際の利用者HOMEへ触れないよう、重複起動を含む全connectを一時HOMEへ隔離した。

## 後続のCI・Release受入

実装後にmainへのpush、両macOSアーキテクチャのCI、v0.1.0の配布物生成・ダウンロード・実Chat受入・公開まで実施した。最初のCIで見つかったlintコンポーネント不足とWindowsからのUnix fixture直接起動を修正し、全4ジョブが成功した。

詳細は[Release受入・本番切替記録](release-validation.md)を参照。下記のうち、GitHub Actions、Intelのnative受入、実Chat/Tunnel/Keychain、本番runtime切替、Release公開はそこで完了している。

## 初回実装検証時点の未実施範囲

- GitHub Actions上での実行、Intelネイティブでの受入、draft Releaseの作成・公開は未実施。workflowを追加し、ローカルで構文とarm64の配布物生成を検証した。
- 実Chat、実Tunnel、実Keychainでの新runtimeの疎通・画像受入は未実施。本番runtime・登録先・Keychain・既存の公式実行ファイルは変更していない。
- 署名・公証は初版の対象外。

macOS CIには[GitHub公式のrunner対応表](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job)に従い、arm64の`macos-15`とIntelの`macos-15-intel`を指定した。
