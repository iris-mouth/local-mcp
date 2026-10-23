# Codex App Serverを実行基盤とする

2026-10-22採用。独自の実行基盤とCodex内部Rust crateへの依存の保守を減らすため、local-mcpをRustの薄いMCPアダプターとし、ローカルコマンドをCodex App Serverの公開`command/exec`系へ委譲する。
通常Chatが判断を担い、`thread`/`turn`でCodexモデルへ再委譲せず、MCP接続・Tunnel・Keychain・起動運用は既存のchatgpt-mcpが管理する。
公開APIがexperimentalである変更リスクを受け入れる一方、作業セッションと許可範囲の検査、ジョブ状態と出力の管理、MCPへの結果変換はアダプターに残す。

根拠: 本人の採用決定（GBrain fact `1541`）と本設計セッションへの引継ぎ、[公式App Server仕様](https://learn.chatgpt.com/docs/app-server#command-execution)。設計の採用は実装開始・配置・公開の許可を意味しない。
