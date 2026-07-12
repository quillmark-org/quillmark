# prose/

Long-form project documentation, in three tiers by maturity:

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
- **`simplifications/`** — point-in-time simplification-review findings,
  grouped by the crate where the fix lands. Entries are removed as they are
  fixed or spun into GitHub issues.

Canonical docs never reference proposals or plans. References never link
out to other prose docs.

