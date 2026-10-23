# App Serverアダプターの実装・検証

Updated: 2026-10-23
基準: 設計確定コミット `29a32a1`。本人の「実装してください。」により、このworktreeで実装を開始した。
設計のAstraレビューと、今回の実装・実機検証は別の実施内容として扱う。

## 実装

- Codex内部Rust crate・専用sandbox helper・関連pinを除去し、rmcp 3.7.0と公開JSON-RPCへ移行した。Cargo.lockに依存を固定した。
- `start`が元App Server接続、runtime、受付記録、ジョブ、有限出力を所有する。既存セッションIPC上のMCP facadeは再生成可能とした。
- 基本8用途と別の単発拡張実行を、strictな9ツールとして公開した。既存wrapperの`session_id`制約と接続キー非継承を維持する。
- 固定認可上限は信頼された開始時JSONだけから決める。canonical cwdとrootの変更検査、追加root/network/fullの排他的指定、起動世代と依頼IDの照合を実装した。
- Gitのsummary/numstatでmodeと対象を解釈し、専用差分はcheck→applyを一度ずつ実行する。patchは準備通知後に元接続のstdinへ一度配送し、argv長上限や一時patchファイルへの保存を避けた。
- 入力ACKと消費、停止ACKと終了、単一job不明と接続喪失、出力欠落と既知の終了結果を分離した。
- 画像はmacOS ImageIOに同じメモリ内bytesを渡し、寸法予算とデコードを検査する。独自codecや画像一時ファイルを追加していない。
- 専用CODEX_HOME/sqlite領域、有限のcommand環境、不要exportの無効化、通常終了確認後だけの所有領域削除を実装した。
- Nixの依存固定を新Cargo.lockへ合わせた。Nix自体のビルドは未検証。

## 実装レビュー4件の修正

実装コミット`bad5dda`に対するAstraのStandards 1件、Spec 3件を対象とする。

- 出力異常の原因を`output_error`として実行・接続エラーと別に保持し、後から元接続が失われても`output_unavailable`を維持する。不正base64、field欠落、不正streamそれぞれの後に接続を失う回帰テストを追加した。
- 長い実行・観測待ちと、session_info・待機なし照会・入力・停止に別の有限容量を確保した。IPC受信枠はdispatch待ちに保持せず、受信枠が埋まっている間はacceptを待つ。応答配送までは受付枠または処理枠を保持し、待機taskや応答bufferを無制限に作らない。
- 専用diffはGitの`--summary`による作成・削除・変更modeも検査し、通常fileの`100644`/`100755`だけを受け付ける。Gitの解釈前のpatch本文からmodeを推測しない。新規symlink/submoduleと通常fileからsymlinkへの変更を拒否するテスト、通常fileのmode・本文を受理するテストを追加した。
- read_fileの行数・返却byte数の終端判定を次行の読込み前へ移した。次のbytesの有無だけで続きのmetadataを返し、要求範囲外の長大行・不正UTF-8を読取失敗の原因にしない。CRLFと終端の続きなしも検査する。

追加した実プロセスのMCP/IPC回帰テストでは、ownerのRPC設定1での実行待機中と、同じfacadeの64件の観測待機が満杯の状態を検査する。この2件は接続競合の修正後に各10回連続成功した。backendは故障注入用fixtureであり、sandboxの証拠にはしない。実App Serverの受入にもRPC設定1での並行照会・入力・停止、行範囲、新規symlink拒否を追加した。

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
| RPC数、ACK待ち、終了待ち | 各容量64、10秒、5秒 | ownerのworkとcontrol、IPC受信、backendのACKに個別の有限容量を設ける。MCP facadeもwork/control各64を確保する。長い待機が照会・入力・停止の予約枠を使わない。観測待ちやACK超過を実行期限・終了証明へ読み替えない |

## 検証の区別

自動契約テストは、故障を注入する疑似backendで行う。疑似backendはsandboxを提供せず、権限境界の証拠には使わない。
実機試験は公式App ServerとmacOS sandbox、実際のMCP stdio facadeを使用し、毎回新しい使い捨てリポジトリへ書き込む。
接続キー試験は合成sentinelを使い、実際の資格情報を読み取らない。

ビルド・静的検査・試験の最終結果、選択したCLIとアダプターのhash、ソースhashは[機械可読記録](acceptance-results.json)へ保存する。

## 最終結果

- `cargo fmt --check`: 成功。
- `cargo clippy --locked --offline --all-targets -- -D warnings`: 成功。
- `cargo test --locked --offline`: 契約テスト14件、実プロセスのMCP/IPCテスト2件、計16件成功。
- `cargo build --release --locked --offline`: 成功。
- 同じreleaseバイナリによる実App Server試験: 66項目成功。試験中にバイナリと対象ソースが変わっていないこともhashで確認した。
- 環境: macOS 27.0.1、codex-cli 0.163.0-alpha.10.2。公式CLIとadapterのhashは機械可読記録へ保存した。

66項目では、root内成功・範囲外/symlink/cwd拒否、限定拡張と全面解除、Git metadataとlinked worktree、重複配送、接続張り直し、closed/pipe/PTY入力、停止、7画像形式、配送量、30秒待ちと期限無効化、通常終了時の専用領域削除を実機確認した。レビュー修正に追加した7項目は、要求外の長大行、新規symlink拒否、RPC設定1での待機中の照会・入力・停止・元実行の終了観測を検証する。

疑似backendでは元接続喪失、入力ACK喪失、停止ACK後も未終了、出力の欠落/期限切れ、受付容量、root差し替え、既知終了と出力異常の分離を確認した。実機での同じ障害注入は未実施。

release版の大出力試験で記録したピークRSS（KiB）:

| source出力 | adapter | App Server |
| --- | --- | --- |
| 64 MiB | 73024 | 67248 |
| 256 MiB | 77152 | 69904 |

どちらもsourceの総byte位置が実出力と一致し、source側切詰めなし、保持bufferの欠落ありと確認した。backendで全寿命の出力全量に比例するmemory増加は、この2負荷の観測では見られなかった。これは長期運用・全並行条件・他版でのmemory上限の証明ではない。

修正過程では、sipsのファイル出力検証を破棄してメモリ内ImageIOへ切り替えた。full modeの余分なfieldがSerdeのunit variantで無視される点も実機試験で検出し、strictなstruct variantへ修正した。拒否条件の回帰テストを契約テスト14件に含めている。

## 通常Chatでの受入（2026-10-23）

既存Tunnel経由の通常Chatで新9ツールを実際に呼び、読取・差分・実行・入力・停止・限定拡張と拒否条件、操作確認後も固定認可上限が維持されることを確認した。facade切断に伴いTunnel clientも終了したため、既存helperで手動再起動し、元runtimeとジョブの既知終了情報を再取得した。自動再接続の成功とは扱わない。

画像はnative MCP image配送まで成功したが、通常Chatは図柄を認識できず、視覚受入は未達。比較した旧get_imageでも同じfixtureを認識できなかったため、新実装固有の原因とは断定しない。詳細と切分け証拠は[通常Chat受入記録](chat-acceptance.md)に記す。

後続の[画像診断](image-diagnosis.md)では旧get_imageのcontent_itemsを画像出力helperへ渡して盲検認識に成功した。しかし新view_imageの再試験では、通常ChatへstructuredContentと同じメタデータのみが公開され、content/content_itemsが無いため視覚受入は引き続き未達。新旧の結果を区別する。

その後、成功したview_imageだけstructuredContent/outputSchemaを使わず、識別子付きJSON textとnative imageを返す互換修正を行った。全17テスト・clippy・[実機66項目](image-fix-acceptance-results.json)成功後、通常Chatで修正版の新9ツールを実利用。content_items=[text,image]から画像出力し、未見5図形の色・形・相対位置を正しく回答できた。本人許可による正解manifest事後照合も全件一致し、修正版の画像視覚受入は合格。詳しい証拠と復元状況は画像診断末尾を参照。

本人許可による共有plugin catalogの一時更新は旧10ツールへ復元し、新旧のテスト接続をすべて終了した。本番binary/profile/helperは不変で、既存GBrainの稼働を確認した。

## 未完了の受入・範囲

- 今回と異なるChat権限モード、hostでの本人の操作拒否を含む確認設定の受入。今回のテスト操作者による一度だけ許可を、本人の手動クリック証明とは扱わない。
- 通常Chat/Tunnel経路での実際の最大配送量と、長期間・多数RPC並行での資源設定調整。
- 公式診断の異常終了後の残留を、運用として確認・回収する受入。実装は異常時に推測削除せず場所を表示する。
- Linux/WindowsおよびNixの配布保証。

本番バイナリの置換、既存chatgpt-mcp/Tunnel/Keychain設定の変更、配置・切替・push・公開は行っていない。
