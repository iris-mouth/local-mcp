# 09 差分編集の対象と確認済み適用結果

Status: ready-for-agent

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

apply_diffはGit checkからapplyへ進む既存jobを返すが、ファイル別の予定対象・適用状態を公開していない。

## 作業範囲

- Gitが解釈した通常text対象のpath/操作/予定行数を受付に結び付け、保持量を有限にする。
- plannedと確認済みapplied/not_applied/unknownを分け、既存のcheck/apply段階を利用する。
- 同じrequest_idの再配送や再接続後も元の結果を返す。出力期限切れを理由に再適用しない。

## 受入条件

- [ ] 新規/更新/削除/複数file、check拒否、apply前停止、apply配送後の失敗・backend断を確認する。
- [ ] 予定行数や停止ACKを適用成功の証拠にせず、apply後の失敗を一律未適用にしない。
- [ ] 範囲外、.git、symlink、非通常mode、index非変更など既存の編集境界を維持する。
- [ ] 保持metadataの件数/byte予算を超えた場合は、変更前に明示的に拒否する。

## 依存と検証

03/04/06の後。apply_patch形式や別編集経路は追加しない。

tests/contracts.rs、tests/transport.rs、tests/real_app_server.py。擬似backendだけでGit適用やsandbox成功を証明しない。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
