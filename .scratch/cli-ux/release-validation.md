# v0.1.0 Release検証概要

2026-10-27。配布物生成、ダウンロードによる検証、実Chat受入、本番runtime切替、公開が完了した。

## 公開物とCI

- Release: https://github.com/iris-mouth/local-mcp/releases/tag/v0.1.0
- Release source: `f6697675c8b21955d305f5c410836da0dca362b3`
- main CI: https://github.com/iris-mouth/local-mcp/actions/runs/40479012262 — macOS arm64/Intel・Ubuntu・Windowsの全4ジョブ成功。
- tag CI: https://github.com/iris-mouth/local-mcp/actions/runs/40517579182 — 成功。
- Release workflow: https://github.com/iris-mouth/local-mcp/actions/runs/40517579128 — 両native macOSでのビルド、lint、Rust試験、CLI/接続/終了試験、配布物smoke検査が成功。

両アーカイブとSHA256SUMSを実際にダウンロードし、内容、Mach-O CPU type、SHA256を照合した。[release-assets.json](release-assets.json)は公開配布物から計算できるハッシュ・CPU typeだけを記録する。

| アーキテクチャ | アーカイブSHA256 |
| --- | --- |
| aarch64-apple-darwin | `3e0792ef11fa7b89ae89ec07abe34a63bb482a58f76d365131fcc94057eb83cc` |
| x86_64-apple-darwin | `91987fdd0c25d77e3ba929870364246b5b1f31cd6025e583249d7d71db7a6162` |

## 受入と切替

- ダウンロードしたarm64配布実行ファイルで、公式App Serverの66項目が成功。
- 通常Chatから、テキスト読取、画像視認、コマンド実行、差分編集・再読取、長時間ジョブ開始が成功。
- Tunnel再接続後も同一の作業セッションと元ジョブを観測できた。
- 通常実行の範囲外書込は拒否され、権限拡張を使わずに確認した。
- 元ジョブの停止受付だけでなく、実終了まで確認した。
- 新runtimeを既定に設定し、旧runtimeは復旧用に保持。受入セッションは正常終了し、所有する診断・socket・Tunnel状態ファイルを回収した。
- 署名・公証は合意どおり初版の対象外。

実Chatの原本・スクリーンショット・端末固有の診断記録はローカルの非公開領域に保持する。公開リポジトリには会話URL、セッション識別子、利用者のworkspaceパスを記載しない。
