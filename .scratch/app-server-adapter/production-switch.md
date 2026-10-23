# 本番切替記録 — 2026-10-23

利用者の指示により、本番バイナリと既存local-workspace共有定義を新9ツールへ切り替えた。共有定義は新9のまま維持する。

## 配置

- ソース: `bc70f0271459f0f9fb84e58c459909f536a2ab90`（Astraレビュー済み）。
- 配置先: `~/.local/share/chatgpt-mcp/bin/local-mcp`。
- SHA256: `ba3e3fe9363604418574f0d5b5e953fa19c032ef1b70eee3dc23ba5672d2852a`。
- 旧バイナリ・ランチャー・旧10ツール定義・変更前ハッシュのバックアップ: `~/.local/share/chatgpt-mcp/backups/local-mcp-9-tools-20261023-215801`。
- 既存ランチャーに公式Codex CLIの絶対パスを`--codex`で指定。Finder相当の最小PATHでも起動できることを確認した。
- 既存プラグインの「ツールを更新」で共有定義を更新。操作確認設定「低リスクの操作を許可」は維持。

## 通常Chatでの確認

使い捨てプロジェクト `/private/tmp/local-mcp-chat-acceptance-_9_7m4__/project` で実施。
会話: https://chatgpt.com/c/6adb2dc8-ce7c-83ee-aa70-25fa5726a53d

| ツール | 結果 |
| --- | --- |
| session_info | 実セッション・作業先一致、最終active_jobs=0 |
| read_file | CRLFファイル読取と変更後内容を確認 |
| view_image | content_itemsのtext/imageから画像出力。未見5図形の色・形・位置すべて正解 |
| apply_diff | before→after、終了コード0、Git indexハッシュ不変 |
| exec_command | stdout/stderr分離、終了コード7の正常な取得 |
| exec_command_expanded | limited、roots=[]、network=falseのままtrue実行、終了コード0 |
| command_status | 各ジョブの実終了・出力を取得 |
| write_stdin | pipeへの1回入力と別要求EOF、出力1回、終了コード0 |
| stop_command | 停止ACKと実終了を分離して確認、終了コード137 |

通常実行による許可書込範囲外への書込は拒否された。ホスト側でも対象ファイルの未作成を確認。
画像の正解manifestは回答確定後に親タスクのみで読んで採点し、正解は試験Chatへ送信していない。

## 保持・終了確認

`profiles/local-workspace.yaml`、`run-tunnel.py`、`local-mcp-stdio.py`、`profiles/gbrain-memory.yaml`は変更前後のSHA256が一致。認証情報・Keychain項目・GBrain設定は変更していない。GBrainのreadyzは最終確認で200。

試験接続のCtrl-C終了時、App Serverの正常終了を確認できない警告が出た。App Server子プロセスの残存はなかったが、所有local-mcpが残ったため、既知の試験PIDだけにTERMを送りランチャーの終了コード143を確認した。診断ディレクトリ `.../T/local-mcp-runtime-DaKUFD` は証拠として残存する。これは通常Chatの9ツール確認とは別の終了処理上の未解決事項。

本番配置と共有9ツール定義は維持し、使い捨て作業セッションは終了済み。次回は既存ランチャーから作業先を指定して起動する。
