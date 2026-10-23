# App Serverアダプターの実装・検証

Updated: 2026-10-23
基準: 設計確定コミット `29a32a1`。本人の「実装してください。」により、このworktreeで実装を開始した。
設計のAstraレビューと、今回の実装・実機検証は別の実施内容として扱う。

## 実装

- Codex内部Rust crate・専用sandbox helper・関連pinを除去し、rmcp 3.7.0と公開JSON-RPCへ移行した。Cargo.lockに依存を固定した。
- `start`が元App Server接続、runtime、受付記録、ジョブ、有限出力を所有する。既存セッションIPC上のMCP facadeは再生成可能とした。
- 基本8用途と別の単発拡張実行を、strictな9ツールとして公開した。既存wrapperの`session_id`制約と接続キー非継承を維持する。
- 固定認可上限は信頼された開始時JSONだけから決める。canonical cwdとrootの変更検査、追加root/network/fullの排他的指定、起動世代と依頼IDの照合を実装した。
- Gitのnumstatで対象を解釈し、専用差分はcheck→applyを一度ずつ実行する。patchは準備通知後に元接続のstdinへ一度配送し、argv長上限や一時patchファイルへの保存を避けた。
- 入力ACKと消費、停止ACKと終了、単一job不明と接続喪失、出力欠落と既知の終了結果を分離した。
- 画像はmacOS ImageIOに同じメモリ内bytesを渡し、寸法予算とデコードを検査する。独自codecや画像一時ファイルを追加していない。
- 専用CODEX_HOME/sqlite領域、有限のcommand環境、不要exportの無効化、通常終了確認後だけの所有領域削除を実装した。
- Nixの依存固定を新Cargo.lockへ合わせた。Nix自体のビルドは未検証。

## 変更理由を伴う初期資源設定

既存の30秒待ち、自動実行期限なし、UTF-8/CRLF、7画像形式は維持した。
新しい保持・再取得・配送のために有限予算を追加した。下記は今回の初期運用値であり、上流・Tunnel・通常Chatの必須値や、性能試験で求めた最適値ではない。

| 設定 | 初期値 | 選定理由・確認対象 |
| --- | --- | --- |
| 並行実行 | 4、開始時変更可能 | 本人の合意値。試験では2へ変更して満杯時の拒否と照会・停止を確認する |
| job別/全体の保持出力 | 4/16 MiB | 長寿命・大出力を全量保持せず、stderrも保持できるようstream別に半分ずつ配分する |
| 出力ページ | 256 KiB | 再取得を小さく区切り、UTF-8置換・JSON/text重複の膨張後も配送予算へ収める |
| 完了後の出力期限 | 3600秒 | 直後の再取得を可能にしつつ無期限保持を避ける。試験では1秒へ変更し、終了結果・受付記録が残ることを確認する |
| 受付記録 | 8192件、ID 128 bytes以下 | 起動中の照合を有限化する。満杯で忘却せず新規変更を拒否する。試験では1件へ変更する |
| MCP/IPCメッセージ | 16 MiB、設定可能 | 12 MiB source textの行範囲読取と約11 MiB画像のbase64/JSON配送をstdio上で検証する。通常Chat経路の配送上限を証明する値ではない |
| stdin/diff | 256 KiB / 1 MiB | 1依頼のメモリ・RPC占有を有限化する。source file全体の一律上限には転用しない |
| 画像pixel数 | 32 Mi pixels | encoded bytesだけでは抑えられないraster展開を有限化する。metadataを先に確認し、最大128 MiBのRGBA rasterへ制限する |
| RPC数、ACK待ち、終了待ち | 64、10秒、5秒 | 変更が滞留しても受け付け続けない。観測待ちやACK超過を実行期限・終了証明へ読み替えない |

## 検証の区別

自動契約テストは、故障を注入する疑似backendで行う。疑似backendはsandboxを提供せず、権限境界の証拠には使わない。
実機試験は公式App ServerとmacOS sandbox、実際のMCP stdio facadeを使用し、毎回新しい使い捨てリポジトリへ書き込む。
接続キー試験は合成sentinelを使い、実際の資格情報を読み取らない。

ビルド・静的検査・試験の最終結果、選択したCLIとアダプターのhash、ソースhashは[機械可読記録](acceptance-results.json)へ保存する。

## 最終結果

- `cargo fmt --check`: 成功。
- `cargo clippy --locked --offline --all-targets -- -D warnings`: 成功。
- `cargo test --locked --offline`: 契約テスト11件成功。
- `cargo build --release --locked --offline`: 成功。
- 同じreleaseバイナリによる実App Server試験: 59項目成功。試験中にバイナリと対象ソースが変わっていないこともhashで確認した。
- 環境: macOS 27.0.1、codex-cli 0.163.0-alpha.10.2。公式CLIとadapterのhashは機械可読記録へ保存した。

59項目では、root内成功・範囲外/symlink/cwd拒否、限定拡張と全面解除、Git metadataとlinked worktree、重複配送、接続張り直し、closed/pipe/PTY入力、停止、7画像形式、配送量、30秒待ちと期限無効化、通常終了時の専用領域削除を実機確認した。

疑似backendでは元接続喪失、入力ACK喪失、停止ACK後も未終了、出力の欠落/期限切れ、受付容量、root差し替え、既知終了と出力異常の分離を確認した。実機での同じ障害注入は未実施。

release版の大出力試験で記録したピークRSS（KiB）:

| source出力 | adapter | App Server |
| --- | --- | --- |
| 64 MiB | 72896 | 68032 |
| 256 MiB | 77136 | 70496 |

どちらもsourceの総byte位置が実出力と一致し、source側切詰めなし、保持bufferの欠落ありと確認した。backendで全寿命の出力全量に比例するmemory増加は、この2負荷の観測では見られなかった。これは長期運用・全並行条件・他版でのmemory上限の証明ではない。

修正過程では、sipsのファイル出力検証を破棄してメモリ内ImageIOへ切り替えた。full modeの余分なfieldがSerdeのunit variantで無視される点も実機試験で検出し、strictなstruct variantへ修正した。拒否条件の回帰テストを11件に含めている。

## 未実施の受入・範囲

- 既存Tunnelと通常Chatの経路を使う、9ツールの実利用・確認設定・拒否条件の受入。
- 通常Chatが未見の図柄を説明できる画像視覚受入。今回のnative MCP image配送・デコード成功を、この受入へ読み替えない。
- 通常Chat/Tunnel経路での実際の最大配送量と、長期間・多数RPC並行での資源設定調整。
- 公式診断の異常終了後の残留を、運用として確認・回収する受入。実装は異常時に推測削除せず場所を表示する。
- Linux/WindowsおよびNixの配布保証。

本番バイナリの置換、既存chatgpt-mcp/Tunnel/Keychain設定の変更、配置・切替・push・公開は行っていない。
