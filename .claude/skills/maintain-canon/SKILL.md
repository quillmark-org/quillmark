---
name: maintain-canon
description: Maintain prose/canon/ — the canonical, high-level design documentation. Use when adding, consolidating, or auditing canon docs.
---

## Purpose

`prose/canon/` captures the codebase's systems, specifications, and intent at a
high level — enough that a human or AI can get the gist without reading the
whole codebase. Canon describes *what is* and points into the code; it does not
re-document implementation detail that the code already carries.

## Structure

`prose/README.md` is the single source of truth for prose structure — the four
tiers, the canon doc spine, and the link invariants. Read it before editing
canon; `scripts/check-canon.mjs` enforces it in CI.

## Principles

One topic per page; one canonical per topic. Prefer deletion with consolidation
over duplicates. Keep pages skimmable and high-level; include minimal code.

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
