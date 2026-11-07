# 08 限定的な名前探索と本文検索

Status: ready-for-agent
Resolution: implemented

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

通常Chatは検索・一覧のたびにexec_commandのargvを作成する。新たな検索エンジンや言語解析は今回の範囲外。

## 作業範囲

- list_filesとsearch_filesを追加し、literal既定、明示regex/glob、path、無視設定、隠しfileの挙動を定義する。
- backend候補rgの必要機能を検査し、信頼された起動環境の固定argvで使う。追加依存をREADMEへ記し、無い場合はfeature_unavailableにする。
- 件数・本文配送・走査時間・子プロセス寿命を有限化し、打切りと一部fileの読取不能を区別する。

## 受入条件

- [x] 名前/本文、空結果、literalとregex、glob、パスの空白/Unicode、binary、ignore/hidden、symlinkを検証する。
- [x] host-user読取範囲と整合させ、model指定の実行file・shell・自動downloadを追加しない。
- [x] 大量出力・長大な一致行・不正pattern・依存欠如・時間上限・終了時の子回収を検証する。
- [x] 検索中も既存のcontrolを塞がない。結果からread_fileで目的箇所を取得できる。

## 依存と検証

03〜06の後。backend能力・追加依存を確定してから公開する。

新しい隔離file fixture、tests/contracts.rs、tests/transport.rs。実rgの能力試験と通常Chatでの探索試験を区別して記録する。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
- 2026-11-07: 実装完了。list_files / search_filesを実rgの固定argvで実装。literal/regex/glob・ignore/hidden・空白/日本語path・非UTF-8本文・symlink非追跡、件数/配送/frame上限、依存欠如・非対応実行ファイル・時間上限・終了時回収を検証した。書換可能な範囲内の検索実行ファイルは拒否。追加依存とhost-user読取範囲をREADMEへ明記。詳細は[実装記録](../implementation.md)。
