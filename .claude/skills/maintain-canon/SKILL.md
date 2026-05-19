---
name: maintain-canon
description: Maintain prose/canon/ — the canonical, high-level design documentation. Use when adding, consolidating, or auditing canon docs.
---

## Purpose

`prose/canon/` captures the codebase's systems, specifications, and intent at a
high level — enough that a human or AI can get the gist without reading the
whole codebase. Canon describes *what is* and points into the code; it does not
re-document implementation detail that the code already carries.

## Meta-structure

- `prose/canon/` — settled truth. One concept per page. Indexed by `INDEX.md`.
- `prose/proposals/` — fleshed-out proposed changes, not yet implemented.
- `prose/BOOKMARKS.md` — quick notes and ideas not yet worth a proposal.

Canon never references proposals or plans.

## Doc spine

Every canon doc opens with a `# Title`, then a one-line `Implementation`
blockquote anchor, then a `## TL;DR` of two or three sentences. Title, the
anchor, and the TL;DR are mandatory; other sections (When to use, How, Gotchas,
Links) are optional — add them when they help.

- The `Implementation` anchor points at a folder or module, never a file and
  never a line number. It is the navigational hook from concept to code.
- No `Status` line — membership in canon means settled and implemented. Mark
  status only for genuine exceptions (e.g. a draft specification).

## Principles

One topic per page; one canonical per topic. Prefer deletion with consolidation
over duplicates. Keep pages skimmable and high-level; include minimal code.
Reference folders, not files or line numbers — they rot.

## Workflow

- **Inventory** — list docs with a one-line summary; note overlaps.
- **Consolidate** — pull unique, current bits into the canonical; rewrite for skim.
- **Prune** — replace overlaps with a one-line stub linking the canonical;
  delete obsolete docs and references.
- **Organize** — keep flat or under a few theme folders; nest sparingly.
- **Index** — keep `INDEX.md` curated, with one-liners; remove drift.

## Done when

No obvious duplicates. Everything discoverable from `INDEX.md`. Docs are short,
skimmable, folder-anchored, and easy to maintain.
