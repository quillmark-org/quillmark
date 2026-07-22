# prose/

Long-form project documentation, in four tiers by maturity:

- **`canon/`** — canonical documentation: high-level, stable captures of the
  codebase's systems, specifications, and intent. The settled truth. Start at
  [`canon/INDEX.md`](canon/INDEX.md). Canon describes *what is* and points into
  the code; it does not re-document implementation detail.
- **`references/`** — authoritative, standalone specifications. Each
  reference is self-contained: it makes no outbound links to other prose
  docs, so it can be cited freely from canon, user docs, and code comments
  without forming a cycle.
- **`proposals/`** — fleshed-out proposed changes, not yet implemented. Each is
  a concrete plan. Removed once landed or abandoned.
- **`plans/`** — working plans for multi-phase reworks in flight: the
  integration HQ for a change too large for a single proposal. One subdirectory
  per rework. Removed once the rework lands.

Canonical docs never reference proposals or plans. References never link
out to other prose docs.

## Canon doc spine

Every canon doc except `canon/INDEX.md` (the index) opens:

1. `# Title` on line 1.
2. A blockquote anchor on line 3. Its `**Implementation**` line is the
   navigational hook from concept to code: it points at a folder or module,
   never a file and never a line number — files rot. Other lines
   (`**Related**`, `**Package**`) are optional.
3. `## TL;DR` as the first section — two or three sentences.

Title, anchor, and TL;DR are mandatory; other sections (When to use, How,
Gotchas, Links) are optional — add them when they help. No `Status` line:
membership in canon means settled and implemented. Mark status only for
genuine exceptions (e.g. a draft specification).

`scripts/check-canon.mjs` enforces the spine and both link invariants in CI.

