# 接続・起動・配置管理をlocal-mcpへ移す

2026-10-24採用。chatgpt-mcpの保守を終了できるよう、local-workspace向けの接続・起動・配置管理を既存Rust本体へ統合し、通常利用の登録先とランタイム名をlocal-mcpへ切り替える。シェルは必要な起動入口に絞り、公式TunnelクライアントとmacOS Keychainを使い、選択した作業先の開始に連動してTunnelを起動・管理する。

ADR-0001/0003のchatgpt-mcpへ起動管理を委譲する部分を本決定で更新する。Tunnel再起動と元App Server/Sessionの再起動は別の操作とし、キーを扱う子プロセスをSession所有・MCP・実行ジョブから分離する。新しい配置先の生成物と移行・復旧手順を先に確認し、GBrain固有の運用はcodex-gbrain-runtimeへ分ける。

根拠: 本移管チャットにおける本人の登録先・責務・Rust統合の採用と「作業開始に連動して自動起動」の回答。
