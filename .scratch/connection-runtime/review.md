# Astra review — 2026-10-24

本人の「astraにレビューを依頼して、問題なければコミットしてください」に基づく。

- モデル: `gpt-6-astra`。実装基準と仕様の2軸を独立したreview sub-agentで確認した。
- 基準: `76a940c81b057e08d6e862109b176103652cb6f5`。対象は未コミットの変更と新規ファイル。既存`.DS_Store`は除外。
- 仕様: `.scratch/connection-runtime/spec.md`、既存App Server仕様、ADR-0001から0005、CONTEXTとAGENTSの手順。

## Standards

初回と修正後の再レビューで、文書化された実装基準への違反とコミットを妨げる指摘はなかった。

## Spec

初回にP2を1件検出。Tunnel leaderだけが異常終了した場合に、元の`tick()`はleaderをreapして唯一のhandleを捨て、同じグループの子を回収しなかった。隔離fixtureの4回の起動後に、owner exit 0にもかかわらず4子プロセスが残ることをreviewerが再現し、そのfixtureは回収済み。

`OwnedTunnel`へ所有処理を集約した。標準`Child`と`waitid(WNOWAIT)`でleaderをreapせずに終了・親子関係を確認し、PGIDが再利用されない状態でグループへsignalを送り、最後にleaderをreapする。Tokioの別waiterによるreapはない。

macOSではzombieだけのグループへもEPERMが返ることを別の隔離fixtureで確認した。EPERMを無条件に無視せず、所有する終了済みleaderと、稼働中のグループメンバーが存在しないことをlibprocで検査してから終了を確定する。取得エラーや稼働メンバーの残留は成功扱いにしない。列挙したPIDへsignalを送る処理はない。

再レビューで両軸とも追加指摘なし。AstraはローカルSDKのABI・定数・終了処理を読み取り確認した。実行検査は親エージェントが担当し、reviewerによる再実行と混同しない。

## 回帰検査

接続fixtureを、グループ全体の停止からleaderだけのSIGKILLへ変更した。各非認証失敗の起動でTERMを無視する子を作り、全retry後の消失、元runtime/jobの保持、認証失敗時の自動再試行なし、通常終了時の回収を検査する。

修正後に21テスト、接続・終了fixture、fmt、全targetのclippyを確認した。最終releaseの接続fixtureと公式App Server結果はpost-reviewの記録を参照する。

保証対象は所有する同じプロセスグループの接続用プロセス。意図的に別グループへ離脱した任意の子孫やowner自身のSIGKILL後の復旧は対象外。
