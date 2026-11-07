# 採用9項目のレビュー

2026-11-07。本人の「レビュー、試用、マージ、本番反映まで進めてください。」に基づく。
基準はmainの`2ee8bde6b64f30ef95e294faecc1352892893e3b`、初回レビュー対象は`4f49b2725dc98b310bc78bd044f6b97f094b0aba`。
比較は`git diff 2ee8bde6b64f30ef95e294faecc1352892893e3b...HEAD`。準備コミット`e941378`も含めた。
code-reviewスキルに沿い、独立した2エージェントで規約と仕様を別々に確認した。

## Standards

文書化された契約へのP2指摘は2件。

- 設定済みの任意search_executableが存在しないと、canonicalizeの失敗が作業セッション全体の起動失敗になる。READMEの「検索はfeature_unavailableとし、他ツールは利用できる」という契約に反する。
- Git applyの終了応答でexitCodeが欠落・不正な場合、実行状態だけunknownとなり、ファイル別結果がplannedのまま残る。READMEの「apply配送後の不明な効果はunknown」という契約に反する。

関連する設計上の判断として、未知状態への遷移で`apply_dispatched`を判定する処理の重複も指摘された。これは独立した規約違反とは数えない。

修正後の再レビューでは、任意検索依存の解決失敗時の縮退、Job::mark_unknownによる共通遷移、不正な終了応答時の未送信diffデータの破棄、実Gitによる回帰試験を確認。両P2と関連する重複は解消し、残る指摘・マージ阻害は0件。

## Spec

[採用仕様](spec.md)と9チケットに対するP2指摘は2件。

- 前述の不正なexitCode経路。仕様の「planned対象と確認済みのapplied / not_applied / unknownを分ける」に反する。実Gitでファイルが変更された後にexitCodeだけを省く再現で、state=unknownとedits.status=plannedの食い違いを確認した。
- ripgrepの自動BOM判定が有効なため、UTF-16のファイルをUTF-8へ変換して検索結果に出す。仕様の非UTF-8の扱いと、チケット08の検索結果からread_fileで本文を取得する契約に反する。検索でneedleを返した同じファイルをread_fileが非UTF-8として拒否することを再現した。

修正後の再レビューでは、apply前の不明結果はnot_applied、配送後はunknownとなることと、同一受付の再確認を検証する回帰試験を確認。検索へ`--encoding none`を追加し、自動変換された一致を返さないことも確認した。UTF-16をraw bytesで検索して一致が出なければskipped_non_utf8=0でよい。この数値は走査した全ての非UTF-8ファイル数を意味しない。
仕様からの未承認の機能追加は検出されず、再レビューの残る指摘・マージ阻害は0件。

## 修正と検証

- 独立した指摘は合計3件。規約軸2件、仕様軸2件のうち、不正終了応答の指摘は重複している。
- 修正前の回帰試験で、検索実行ファイル欠如による起動失敗と、check/apply両段階のplanned残留を再現した。
- 修正後は、実Gitのcheck/apply終了からexitCodeを取り除くfixtureで、実ファイルがbefore/afterであること、not_applied/unknownの区別、元のjob_idと編集結果を再配送で維持することを検証する。
- UTF-16 BOMの本文を通常のUTF-8一致として捏造しないこと、検索依存がなくてもread_fileを使えることを回帰試験へ追加した。
- NixのnativeCheckInputsには、今回の実Git試験に必要なgitも明記した。Nix build自体はローカル未実施。

最終の試験結果は[実装記録](implementation.md)、マージと本番反映は[切替記録](deployment.md)へ記録する。レビューエージェントはソースを変更せず、修正と試験の実行は親エージェントが行った。
