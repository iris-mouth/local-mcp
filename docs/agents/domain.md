# Domain docs

This repository uses one context: root `CONTEXT.md` and `docs/adr/`.

Before exploring:
- Read `CONTEXT.md` for project vocabulary.
- Read ADRs relevant to the topic.
- If either is absent, proceed silently.

Use the glossary's canonical terms. Surface conflicts with an ADR explicitly.

Create domain documents lazily through domain-modeling:
- `CONTEXT.md` contains only project-specific terms and definitions.
- Record an ADR when a decision is costly to reverse, surprising without
  context, and the result of a real trade-off.
- Number ADRs sequentially under `docs/adr/`.
