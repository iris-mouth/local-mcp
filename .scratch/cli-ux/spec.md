# 通常Chat向けCLI UXとRelease配布

2026-10-27。本人が実装計画を採用し、実装を指示。

## 採用方針

- 通常Chatから選択したローカルworkspaceのファイル・コマンドを操作する用途に限定する。
- 1接続につき1workspace。起動中の切替は行わず、終了して選び直す。
- macOSの行単位CLIとして既存Rust本体を拡張する。全画面TUI、新規crate、daemon、自動更新、永続ジョブ台帳は追加しない。
- 許可書込範囲と固定認可上限を維持する。読取は現行のhost-user範囲。MCPツールや引数を変更しない。

## 操作

- 引数なし: 未設定ならsetupを実行し、設定後はworkspaceを選択してconnectする。
- setup [--from-runtime PATH] [--config FILE]: 新規配置をprepareし、Codex能力・Tunnel設定を検査して既定runtimeとSettingsを保存する。接続は開始しない。
- connect [PROJECT] [--runtime PATH] [--config FILE]: 明示引数を優先し、保存設定、従来の実行ファイル相対配置の順でruntimeを解決する。PROJECT省略の非対話実行は拒否する。
- /help、/status、/jobs、/stop JOB_ID、/tunnel-restart、/quit。入力をシェルへ渡さない。
- ジョブの受付・実行・停止要求・終了・不明を区別する。停止ACKだけでは実行終了と表示しない。
- 対話中の/quit・Ctrl-Cは未終了または不明のジョブがあれば既定Noで確認する。確認中もIPCを処理する。再度Ctrl-C、非対話SIGINT、EOF、TERM、HUPは終了処理へ進む。
- 接続準備は公式clientのloopback /readyzで検査する。クライアント起動、readiness、Chatからの実疎通を同一視しない。遅い検査・再接続で元ジョブの観測・停止を塞がない。

## 保存と更新

~/Library/Application Support/local-mcp/preferences.jsonにversion、既定runtime、Settings、最近のworkspace最大10件を保存する。秘密・コマンド本文・ジョブ出力は保存しない。0700のディレクトリ、0600のファイル、ロックと原子的置換を使い、不正・破損した設定は拒否する。

setup --configはSettingsを保存する。connect --configはその起動だけの指定で、既存startは従来の設定解決を保つ。最近のworkspaceはSession起動後に更新し、更新失敗は警告して作業を継続する。

更新時は旧作業を終了し、新Releaseのsetup --from-runtimeで別の新規配置を生成する。同一Tunnelのleaseを検査から既定runtime更新まで保持し、検証失敗時は旧設定を維持する。旧配置は自動削除しない。

Keychainは既存の専用子プロセスだけが実キーを読む。setupはOS上での項目準備とChatGPT接続登録を案内し、キー入力・作成・変更を行わない。

## 配布と受入

GitHub ActionsでmacOS arm64/x86_64をそれぞれネイティブ検証し、実行ファイル・README・LICENSEのtar.gzとSHA256SUMSを生成する。バージョンタグからdraft Releaseを作り、公開は別操作。公式CodexとTunnel clientは別途配置する。署名・公証は初版に含めない。

fixtureのセットアップ・更新・破損拒否、PTYでの選択・操作・終了確認、loopback readinessと復旧、元runtime/job維持、終了・診断回収を検証する。fmt/clippy/cargo testと既存実App Server試験を実行する。実Chat/Tunnel/Keychainの受入は別に記録し、fixture成功を実疎通成功とは扱わない。本番配置・キー・登録先は実装作業で変更しない。
