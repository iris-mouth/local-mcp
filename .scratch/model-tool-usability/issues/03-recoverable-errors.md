# 03 詳細と復帰方法を返すエラー

Status: ready-for-agent

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

src/session.rsのparsedマクロはserdeの具体的原因を捨て、invalid or unknown tool fieldsへまとめる。

## 作業範囲

- error.code/messageを維持し、field・期待する型/範囲・復帰説明を必要時に追加する。
- 実行前の拒否と配送/結果不明を、処理段階の証拠に基づいて区別する。
- 入力本文・秘密・環境の値をそのままエラーへ含めない。次回呼出し候補機能は追加しない。

## 受入条件

- [ ] 必須不足、型違い、不明field、排他違反、範囲違反、runtime不一致で修正箇所が分かる。
- [ ] dispatch_unknown/result_unknownは未実行や単純なretryableへ読み替えず、元request_idの観測を案内する。
- [ ] 非ゼロ終了を引数エラーにしない。受付記録により重複実行を防ぐ既存挙動を保つ。

## 依存と検証

02と語彙を揃え、04のエラーschemaへ反映する。

tests/contracts.rsとtests/transport.rsで実際の拒否・配送障害・再確認を検証する。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
