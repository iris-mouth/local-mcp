# 04 ツール別の出力schemaと実返却の整合性

Status: ready-for-agent
Resolution: implemented

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

src/mcp.rsのoutputSchemaはschema_versionとokだけで、実際のstate/text/cursor/errorなどを宣言していない。

## 作業範囲

- 既存ツールごとに返却field、enum、nullableな結果、継続・エラーの形を定義する。
- 代表的な成功/失敗の実返却を公開schemaで検証する。単なるfield存在確認で代用しない。
- Rust型との二重定義を増やす場合はずれを検出する。validator依存はテスト用途と配備用途を区別して選ぶ。

## 受入条件

- [x] 受付済み/実行中/停止要求済み/終了/不明と、出力異常・引数拒否を検証できる。
- [x] view_imageのoutputSchema無し、native imageとreceiptの実IPC挙動を保持する。
- [x] 追加ツールと配送量fieldを含め、公開schema・実返却・READMEが一致する。

## 依存と検証

02/03を受ける。05〜09も返却変更のたびにこの契約へ追随する。

tests/contracts.rs、tests/transport.rs。既存image_wireテストを含め、実際のMCP応答を検証する。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
- 2026-11-07: 実装完了。src/schemas.rsへ型・enum・nullable・継続・エラーを定義した。Draft 2020-12 validatorで全12ツールの公開定義と実MCP応答36件、誤ったstate/textの拒否を確認。view_imageのnative image例外と既存wire回帰も通過。詳細は[実装記録](../implementation.md)。
