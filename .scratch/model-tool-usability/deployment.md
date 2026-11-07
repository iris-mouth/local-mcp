# レビュー後の試用・マージ・本番反映

2026-11-07。本人の「レビュー、試用、マージ、本番反映まで進めてください。」に基づく。
この記録は実装時点の対象外だった統合・配置作業を追う。新しいRelease公開は含めない。

## 反映前の確認

- [独立した2軸レビュー](review.md)の指摘3件を修正し、再レビューの残る指摘は両軸とも0件。
- Rust 41件、実MCP schemaの12ツール/36応答、実App Server 77項目、CLI・接続管理・終了の各試験が修正版で成功した。[実装記録](implementation.md)。
- origin/mainは実装前の`2ee8bde6b64f30ef95e294faecc1352892893e3b`と一致した。mainへの保護ルールはない。
- 稼働中の旧本番接続は別の作業先で、実行中ジョブは0件だった。切替直前に再確認し、元ownerの正常終了と所有プロセス・socketの消失を確認してから切り替える。
- 本番の検索依存としてHomebrewのripgrep 15.2.0を導入した。信頼された設定で絶対パスを選び、MCP引数から実行ファイルを選ばせない。

## 反映手順

1. 修正版ブランチをpushし、GitHub Actionsの4環境CIを確認する。
2. 確認したブランチをmainへfast-forwardでマージし、origin/mainへ反映する。
3. 同一の検証済みreleaseから、未使用の最終パスに新しいruntimeを生成する。旧runtimeは復旧用に保持する。
4. 元のTunnel ID、client、公式Codex、Keychain参照を引き継ぐ。信頼された実行認可の設定と最近の作業先を保持する。
5. 新runtimeで実Tunnel疎通、12ツールの公開、検索→読取、編集結果、ジョブ一覧、出力量とUTF-8継続を試用する。
6. 既存プラグインのツール定義を再取得して反映を確認する。試用で始めたジョブを実終了させ、所有する試用接続を正常終了する。

配置先・ハッシュ・設定バックアップ・識別子・試用応答の原本は無視対象のローカル領域へ記録する。公開リポジトリには利用者の作業先、会話URL、セッション識別子、秘密の値を記載しない。

## 結果

### 試用とマージ

- 本番候補の生成済みruntimeで、実MCPの9フローと19応答のschema検証が成功。ファイル一覧→literal検索→行読取、200行の継続と日本語CRLF、修正箇所を示すエラー、実Git編集と同一受付再確認、8byteのUnicodeページ、実行中ジョブ一覧、停止後の実終了を確認し、所有する候補Sessionを正常終了した。
- [作業ブランチのCI](https://github.com/iris-mouth/local-mcp/actions/runs/42335249991)はmacOS arm64・Intel、Ubuntu、Windowsの全4ジョブ成功。
- レビュー修正版`63c68694b11f8c4df68ff4887e8d7e04e9441af9`をmainへfast-forwardで統合し、origin/mainへpushした。
- main上でも同一SHAの[CI](https://github.com/iris-mouth/local-mcp/actions/runs/42335746677)が全4ジョブ成功した。

### 本番ランタイム

- 新しい既定: `~/.local/share/local-mcp-usability-63c68694-live`。
- 検証・配置済みバイナリSHA256: `73a524df6b45f9b789cc3d805ee9eeb5e4fb065ca318f3f5373298708c850f13`。最終の実App Server受入と一致した。選択した公式CodexのSHA256も受入時と一致した。
- 旧`~/.local/share/local-mcp-v0.1.0`は変更せず復旧用に保持した。切替直前に旧Sessionのジョブ0件を再確認し、同じowner/socketを照合してSIGINTで終了。所有プロセス・socket・診断領域・Tunnel状態ファイルが回収された。別親のプロセスなので旧ownerの終了コード自体は取得していない。
- サポート済みのsetupで、元のTunnel・client・公式Codex・Keychain参照を引き継ぎ、新規runtimeの生成と既定化を実施した。設定ファイルは0600で、元の固定認可上限と最近の作業先を維持。追加設定は検索用の絶対パスで、編集metadataは新しい既定1MiBとした。
- 新launcherでこのリポジトリのSessionとApp Serverを起動した。初回の既存Keychainへのアクセス確認はMac側の本人操作へ引き継ぎ、本人の「Mac側で許可しました」後に実接続の検証を完了した。

### 実接続と公開定義の確認

| 対象 | 結果 |
| --- | --- |
| 新runtimeのhealth / ready | どちらもHTTP 200 |
| 実Tunnel経由のsession_info | 作業先とruntimeが起動時の値に一致、backend利用可能 |
| 実Tunnel経由のread_file | 試用ファイルの元本文と一致 |
| 実Tunnel経由のapply_diff | 実Gitの適用をappliedと返し、実ファイルの変更を確認。同一request_idは元jobと編集結果を返し、再適用なし |
| 実Tunnel経由のexec_command / command_status | printfの既知終了と、合計8byte予算による3ページの日本語・4byte文字の復元を確認 |
| 本番と同じSessionのMCP facade | 新しい一覧・検索・ジョブ継続を含む6確認、7応答のschema検証が成功 |
| ChatGPT側の公開定義 | 既存プラグインで「ツールを更新」を実行。公開一覧で書込5・読取7の計12とlist_files / list_jobs / search_filesを確認 |
| 正常終了 | 未終了ジョブ0件で/quit、所有ownerはexit 0。owner/client/facade、socket、診断領域、health/PIDファイルの回収を確認 |

この実行中のChatは以前の9ツールの定義を保持していたため、新3ツールの実呼出しは同じ本番SessionのMCP facadeで確認し、ChatGPT側の登録は公開一覧で別に確認した。新しい会話などで定義を再取得するまで、既存の会話では旧定義が見える場合がある。別の通常Chatモデルによる成功率比較は行っていない。

試用接続は正常終了し、新runtimeは既定のまま次回起動待ち。次回は新runtimeの`start-local.command`を作業先付きで起動する。実接続の応答原本と12ツールの確認画像はローカルの非公開記録に保存した。

### 復旧

新Sessionの実終了を確認してから、保持した旧runtimeの`start-local.command`を作業先指定で明示起動する。既存プラグインでツールを再取得すると旧9ツールへ戻せる。既定設定の切替前バックアップもローカルの非公開記録に保持した。元の終了済みジョブや受付を再実行・復旧する手順ではない。
