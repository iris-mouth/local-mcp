# 06 状態照会ごとの出力量指定

Status: ready-for-agent
Resolution: implemented

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

src/jobs.rsのstatusは起動時output_page_bytesをstreamごとに半分ずつ使い、呼出しごとに小さくできない。

## 作業範囲

- command_statusへ任意max_output_bytesを追加し、stdout/stderr合計の既定16KiBを初期候補にする。
- 信頼されたoutput_page_bytes以下でのみ指定でき、保持量・期限・実行期限は変えない。
- exec/apply_diffの初回配送にも同じ内部既定を使い、変更系引数の受付同一性を変えない。

## 受入条件

- [x] 小さい/既定/上限量でページを取得し、既存カーソルで重複観測・継続ができる。
- [x] stdout/stderr・PTY・UTF-8境界・gap・expiry・既知終了結果を保持する。
- [x] 小さすぎる量やJSON膨張も明示的に処理し、単なる配送料不足を実行失敗へ変換しない。
- [x] 「終了まで待つ」modeを追加せず、出力増加で観測を返す現行条件を維持する。

## 依存と検証

01/03/04の後。05と共通のbyte予算・上限の説明にする。

tests/contracts.rs、tests/transport.rs。複数の返却量でbyte位置・本文とメッセージ量を確認する。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
- 2026-11-07: 実装完了。max_output_bytesは既定16KiB、明示時8byte以上で信頼された上限以内とした。8byteの非消費継続、UTF-8/両stream、極小の既存設定でも実行済み状態を失わないこと、PTY/欠損/期限切れを検証。初回exec/diffの受付同一性と観測待ちの条件を維持。詳細は[実装記録](../implementation.md)。
