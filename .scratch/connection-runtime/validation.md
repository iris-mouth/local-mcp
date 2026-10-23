# 接続・起動・配置移管の実装と検査 — 2026-10-24

## 実装

`prepare`/`connect`と専用credential helperを既存Rust crateへ追加した。作業セッションと元App Serverを既存ownerが所有し、準備成功後のTunnel起動・最大3回の有限再起動・端末からの明示再起動・終了時回収を行う。外部の新daemonやモデルへの実行再委譲は追加していない。

公式clientへのキー注入は子helperからのexecで行い、profileは`/usr/bin/env -u CONTROL_PLANE_API_KEY -u OPENAI_API_KEY`でMCPを起動する。キーをログや生成物へ保存する処理はない。検査では実Keychainを取得せずダミー値を使った。

## 検査結果

| 検査 | 結果と範囲 |
| --- | --- |
| fmt / clippy（全target、警告エラー） / diff whitespace | 成功 |
| `cargo test --locked --offline` | 新規4・既存契約14・実IPC3、計21成功 |
| `tests/connection_runtime.py` | debugとレビュー用releaseで成功。配置ハッシュ・権限・上書き拒否・排他・有限retry・認証失敗時停止・明示restart・元runtime/job保持・INT/EOF/TERM/HUP回収 |
| `tests/shutdown.py` | debug/fixtureとレビュー用release/公式CLIで各6経路成功 |
| `tests/real_app_server.py` | debugで公式CLI `0.163.0-alpha.10.2`の66項目成功。許可root、範囲外・symlink拒否、Session所有、単発拡張、入出力、診断cleanupなど |
| releaseビルド | Rust 1.96、既存lockfile、offlineで成功。crate追加なし |
| 既存公式Tunnel clientの`doctor` | ダミーキーを渡して成功。profile、stdio実行先、health listenerを確認。実Tunnelへは接続していない |

初回sandbox実行ではUnix socket bind、doctorではループバックhealth portのbindが拒否された。用途を隔離検査に限定したsandbox外実行で同じ検査が成功した。自動承認レビューによる拒否はない。

プロトコルfixtureはfilesystem sandboxを実装しないため、実書込境界の証拠は公式App Serverの66項目から取る。fixtureの接続成功は実Chat/Tunnel/Keychain成功の証拠にしない。公式clientのdoctorはキーの有効性やremote readinessを証明しない。

## レビュー用生成物

- 配置: `/private/tmp/local-mcp-runtime-review-20261024-75831e0e`
- 実バイナリのSHA256、保護対象の比較結果、doctor結果: [staging-results.json](staging-results.json)
- 公式App Server結果とソース・バイナリハッシュ: [real-app-server-results.json](real-app-server-results.json)
- 移行と復旧: [migration.md](migration.md)

生成物は現在の本番と同じTunnel ID、同じ公式CLI、同じKeychain service/accountを参照し、既存公式clientのコピーとLICENSE/NOTICEを含む。作成とdoctorにキーの取得は含まれない。

## 本番切替の状態

隔離検査完了時点では、本番配置・共有接続切替・Keychain確認は未実施だった。その後、本人の本番切替指示を受け、`~/.local/share/local-mcp`への配置、既存アプリの表示名変更、本人のKeychain確認と実Tunnelの受入を完了した。[本番切替結果](production-switch.md)。検証Sessionは正常終了し、次の作業開始待ち。通常Chatの別会話での視覚受入は未実施。

chatgpt-mcpの作業ツリーは`codex/integration-retirement`でclean、注釈付きタグのcommitは引継ぎどおり`a5b3e8fd935810272ee7e7ca7dff8aef343be209`だった。旧共有runtimeのbinary/client/launcher/helpers/local-workspaceとGBrainのprofile・plistのSHA256は検査前後で一致した。実装段階でGBrainの設定・起動・Keychain・公式CLIを変更していない。

初回検査の記録作成時点ではlocal-mcpの変更はmainに未コミットだった。既存の未追跡`.DS_Store`を保持している。

## Astraレビューによる追補

初回の本番切替後、本人の指示でAstraの独立した実装基準・仕様レビューを実施した。所有グループの残留P2を修正し、macOSのzombieグループEPERMも状態確認つきで処理した。両軸の最終レビューは追加指摘なし。[レビュー記録](review.md)。

修正後の21テスト・接続fixture・終了fixture・fmt/clippyと、最終releaseの接続fixtureおよび公式App Server66項目が成功した。本番には同じ設定で修正版を配置し、停止状態を保持した。[修正版結果](post-review-results.json)。初回検査のSHA256は当時のものとして残し、最終releaseとソースのハッシュは[post-review-real-app-server-results.json](post-review-real-app-server-results.json)で区別する。
