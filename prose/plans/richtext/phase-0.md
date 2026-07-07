# Phase 0 — spikes

Throwaway probes answering the questions a schema freeze couldn't cheaply
revise later, run before phase 1 froze anything. Nothing here landed on
`main`; the runnable probe, if still present, is `crates/richtext-spikes/`
(held outside `default-members`), one assertion per claim below.

**Status: all three reported. No red flag.**

---

## Spike A — mark semantics (gated the phase-1 freeze)

Validated the model-side normalization contract in Rust and derived the
editor-behavior half from ProseMirror / Lexical / Quill's documented
disagreements rather than a live binding — sound because the contract's
central move is to *not* encode what editors disagree on (edge-expand,
adjacent-merge-at-insertion are editor-owned, never stored). Confirmed: the
frozen mark set + three normalization rules (pinned in [phase-1.md](phase-1.md)
and canon) are editor-independent by construction. Confirmed the stale-text
writer's move weak spot — a paragraph reorder is delete+insert to any char
differ, so a naive rebase detaches an anchor in the moved text — and its
mitigation, a verbatim-block move detector restricted to inserted text.
Confirmed USV coordinates round-trip correctly at the UTF-16 surrogate-pair
boundary.

**Residual gate (Phase 3, closed in PR-A):** bind one real rich editor and confirm
none forces an edge-expand / adjacent-merge semantic back into the model. If
one does, it enters as editor config, not a serialization change. ProseMirror
probe on `spike/richtext-phase-3` (`crates/richtext-spikes/editor-pm/`) — all
matrix scenarios green, no serialization change. Findings in
[phase-3.md](phase-3.md) § Spike-A.

---

## Spike B — source-map inversion + navigation (gated the phase-2 emit design)

Found glyph-offset inversion through `escape_markup` usable, cluster-exact,
invertible by pure recomputation (no stored per-character tables), confirmed
on real typeset output. This underwrote phase 2's `locate` / `position_at`
design (now canon: PREVIEW.md).

**Correction (surfaced in phase-2 PR-F — see [phase-2.md](phase-2.md) § PR-F):**
"cluster-exact, never sub-char" holds for markup text but **not** for a
multi-line `#raw` code fence. Every physical line's glyphs resolve to the same
Typst node — the whole `#raw(...)` call expression, one node wider than any
per-line run `emit.rs` records — so per-run inversion structurally fails
there, not just loses precision. This spike's probe carried no code fence, so
the gap didn't surface. Landed behavior (PR-F): navigation degrades to the
code segment's corpus start, not a per-line offset, in this case.

---

## Spike C — seam encoding + determinism (confirmed Option A, gated phase-2 storage)

Confirmed canonical RichText-JSON serializes byte-deterministically (mark /
island order and `props` key order both closed by sorting before emit; float
formatting unexercised, flagged for real island types), that the seam and
storage serialization are the *same* canonical bytes, and that both backends
lower trivially. Confirmed the migration determinism boundary: cold-import is
a pure function, so a legacy body — including one holding tables or images —
migrates deterministically to sequential island ids (`isl-N`); the one
determinism boundary the content hash must still tolerate is *real*
per-creation island-id minting, which doesn't appear until Phase 4.

---

## Gate

Phase 1 opened on Spike A; phase 2's emit design and storage cutover opened on
Spikes B and C. All three reported, no red flag, no reopening since.
