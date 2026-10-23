# Issue tracker: Local Markdown

Issues and specs live in `.scratch/`.

- One feature per directory: `.scratch/<feature-slug>/`.
- Spec: `.scratch/<feature-slug>/spec.md`.
- Tickets: `.scratch/<feature-slug>/issues/<NN>-<slug>.md`,
  numbered from `01`, with one file per ticket.
- Record triage state in a `Status:` line near the top.
  Use the vocabulary in `triage-labels.md`.
- Append comments under `## Comments`.

When a skill says "publish to the issue tracker", create a local file.
When it says "fetch the ticket", read the referenced local file.
