# 01 UTF-8の文字境界を保つ出力ページ

Status: ready-for-agent

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

src/jobs.rsのOutput::pageは任意のbyteで切り、from_utf8_lossyへ渡す。正常な日本語でもページ境界で置換文字が生じる。

## 作業範囲

- Outputの切出しとカーソル進行を、原文byte位置を保った文字境界へ合わせる。
- 実行中の未完末尾、終了時の不正UTF-8、保持先頭の欠損を区別する。
- 設定上限を超える保持や本文の永続化を追加しない。

## 受入条件

- [ ] 日本語と4byte文字を複数ページへ分け、連結が元の正常UTF-8と一致する。
- [ ] backend通知が文字途中で分かれる場合、CRLF、片streamのみの出力を確認する。
- [ ] gap/lossy/expired/source_truncated、元から不正なbytesの表示と既知の終了結果を保持する。
- [ ] 小さすぎる予算や未完末尾で、無限待機や進まない継続を作らない。

## 依存と検証

なし。準備済みのRust 1.96.0環境を使い、最初に着手する。

src/jobs.rs内の意味のある境界試験、tests/contracts.rs、tests/transport.rs。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
