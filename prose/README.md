# prose/

Long-form project documentation, in two tiers by maturity:

- **`canon/`** — canonical documentation: high-level, stable captures of the
  codebase's systems, specifications, and intent. The settled truth. Start at
  [`canon/INDEX.md`](canon/INDEX.md). Canon describes *what is* and points into
  the code; it does not re-document implementation detail.
- **`proposals/`** — fleshed-out proposed changes, not yet implemented. Each is
  a concrete plan. Removed once landed or abandoned.
- **`BOOKMARKS.md`** — known simplifications and refactors deliberately
  deferred. Lighter-weight than a proposal: just a placeholder so the
  insight isn't lost between releases.

Canonical docs never reference proposals or plans.
