# 実装前の基準確認

2026-11-07。ブランチ `codex/model-tool-usability`、基準HEAD `2ee8bde6b64f30ef95e294faecc1352892893e3b`。
採用9項目のコード実装はまだ行っていない。今回の準備文書はこの基準に対して記録する。

## ソースと既存実行ファイル

- `.scratch/cli-ux/real-app-server-results.json`に記録されたsrc/*.rs・Cargo.toml・Cargo.lockの16件すべてが現在の内容と一致した。
- 既存の`target/release/local-mcp`はlocal-mcp 0.1.0。SHA256は`97a4bf4765986b79222e2e89330f44d513fd88fea5ef6ddbaaae85efeddab9cb`で、同じ既存受入記録と一致した。
- この対応を確認した上で既存release実行ファイルのfixture試験を再実行した。過去の実App Server/通常Chat受入を今回再実行したとは扱わない。

## 検査結果

| 検査 | 今回の結果 |
| --- | --- |
| 作業ツリーと新規文書の保存先 | PASS。作成先は選択したworktree内。開始時の未追跡変更は.DS_Storeだけ |
| 現在ソース16件と既存受入記録の照合 | PASS |
| 既存releaseバイナリと受入記録のSHA256照合 | PASS |
| releaseバイナリによるtests/cli_ux.py | PASS、exit 0 |
| releaseバイナリによるtests/connection_runtime.py | PASS、exit 0 |
| releaseバイナリによるtests/shutdown.py | PASS、exit 0 |
| cargo fmt --check | PASS、隔離したRust 1.96.0で実行 |
| cargo clippy --locked --offline --all-targets -- -D warnings | PASS、exit 0 |
| cargo test --locked --offline --all-targets | PASS、単体9・契約14・実IPC3、計26成功 |
| 新規文書のリンク・Status・空白検査 | PASS、spec/baseline/9チケットの計11ファイル |

fixtureとRust試験は使い捨てHOME・一時リポジトリ・ローカルIPC/HTTP・疑似backendを使う。実Keychain・実Tunnel・通常Chatへの接続は実施していない。
ローカルIPCを使う試験はsandbox外実行で行った。自動承認レビューによる拒否はない。

## Rust検証環境

開始時はPATHにcargo/rustcがなく、過去に使った一時配置にも実行ファイルが残っていなかった。最初のcargo呼出しはexit 127、隔離toolchain導入直後のoffline試験は依存cache不足でexit 101だった。これらをソースの試験失敗とは扱わない。

`target/model-tool-usability-build/`へ公式rustup-initを取得し、公式SHA256と一致することを確認して、Rust 1.96.0 / rustfmt / clippyを導入した。生成物は既存.gitignoreのtarget配下に置き、シェル起動設定・通常のRust保存先・本番runtimeは変更していない。

- rustup-init SHA256: `63752a1919a3a4793173ec70f8f212621a7ce38ba6a246095984482f4de20f9f`。
- rustc: `1.96.0 (ac68faa20 2026-05-25)`。
- cargo: `1.96.0 (30a34c682 2026-05-25)`。
- cargo fetch --lockedで依存を取得し、その後のclippy/testはofflineで成功した。Cargo.lockと実装ソースは基準HEADから変更していない。

保存先の選択と--no-modify-pathは[公式rustupの導入手順](https://rust-lang.github.io/rustup/installation/index.html)、[公式の手動導入方法](https://rust-lang.github.io/rustup/installation/other.html)に従った。

repoのルートから再利用する場合:

```sh
export CARGO_HOME="$PWD/target/model-tool-usability-build/cargo"
export RUSTUP_HOME="$PWD/target/model-tool-usability-build/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline --all-targets
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/cli_ux.py
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/connection_runtime.py
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/shutdown.py
```

targetは生成物なので永続的な開発環境の保証ではない。削除された場合はrust-toolchain.tomlの1.96.0とcomponentsに合わせて検証環境を再作成する。

## UTF-8不具合の切り出し再現

現行Output::pageと同じ「byteで切り、各ページをlossy decodeする」処理を、小さな独立した例で再現した。これはサーバーE2E試験ではない。

- 入力は正常なUTF-8の「あ」50,000文字。
- 現行既定の出力ページ256KiBをstreamごとに半分にした131,072byteで分割。
- 前後のページにそれぞれ置換文字が1つ発生し、ページの連結は元本文と不一致。

01の実装では、この再現に加えて実際のOutput・通知境界・保持欠損を検証する。

## 今回まだ実施していないこと

- 採用9項目のコード変更、新API・新しい出力schemaの実動作検証。
- 公式App Serverを使った今回の境界試験、通常Chatのモデル成功率・転送量の比較。
- CI、Release、配布、本番配置・登録・Keychainの変更。
