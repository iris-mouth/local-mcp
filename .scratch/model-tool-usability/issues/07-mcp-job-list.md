# 07 同じ作業セッションのジョブ一覧をMCPへ公開

Status: ready-for-agent
Resolution: implemented

Spec: [採用9項目の仕様](../spec.md)

## 現状と目的

ジョブ一覧はsrc/cli.rsの/jobsだけから参照でき、session_infoは件数だけを返す。

## 作業範囲

- list_jobsを追加し、job_id/state/exit_code、有限limit、runtimeに結び付く継続情報を返す。
- 既存のID順を基本にし、初期limit50・最大200を候補とする。snapshot保証は作らない。
- 有限control枠で扱い、コマンド本文・出力の台帳や新しい永続状態を追加しない。

## 受入条件

- [x] 空一覧、複数ページ、実行中/停止要求/終了/不明、MCP再接続後の同じruntimeを確認する。
- [x] 古いruntime・別条件のcursorを拒否し、作業セッション再起動をまたいで復元しない。
- [x] work枠が満杯でもジョブ確認・既存の入力/停止が応答する。

## 依存と検証

03/04/06の後。既存Jobs::summariesを再利用する。

tests/contracts.rs、tests/transport.rsの容量・再接続ケース、tests/cli_ux.pyの既存/jobs回帰。

## Comments

- 2026-11-07: 本人が採用した項目の実装準備として作成。実装・受入は未実施。基準環境と検査結果は../baseline.mdを参照する。
- 2026-11-07: 実装完了。list_jobsを追加。既定50/最大200件、ID順、runtimeとlimitに結び付くcursor、liveな一覧を実装した。空・複数ページ・異なるlimit/古いruntimeの拒否、再接続、1work枠と64wait飽和時のcontrol応答を確認。詳細は[実装記録](../implementation.md)。
