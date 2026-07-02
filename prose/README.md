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

Canonical docs never reference proposals or plans. References never link
out to other prose docs.

Deferred cleanups and simplifications are tracked as GitHub issues, not in a
checked-in markdown file.

