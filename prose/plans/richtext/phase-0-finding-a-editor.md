# Phase 0 — Spike A finding: mark-semantics contract

Status: **reported.** Gates the phase-1 canonical-serialization freeze and the
phase-3 editor binding. Runnable evidence:
`crates/richtext-spikes/tests/spike_a_editor.rs` (11 assertions, one per claim
below).

## Scope note — what ran, what did not

The plan's Spike A binds two live JS editors. This environment runs no browser,
so the spike validates the two halves it *can* execute deterministically —
the **model-side normalization contract** and the **stale-text writer's
annotation rebase** (the novel, project-specific risk) — in Rust, and derives
the **editor-behavior half** from the editors' documented semantics rather than
a live binding. The freeze is safe on this basis because the contract's central
move is to *not encode* the semantics editors disagree on (below); a live
binding remains the one residual gate (see § Residual gate).

## The frozen mark set

`strong` · `emph` · `underline` · `strike` · `code` · `link { url }` — the
round-trippable projection marks — plus `anchor { id }`, a **zero-width identity
mark** carrying a comment thread or stable anchor. The set is **open**: an
unknown kind round-trips opaque, absorbed as a new *type*, never a changed
semantics of a known one.

Two mark classes with different algebra:

- **Formatting** (`strong`…`link`): a property of a text range. Value equality
  is range + kind + attrs; two are redundant if they coincide.
- **Identity** (`anchor`): a handle, not a property. Two anchors over the same
  range are two distinct things (two comments), never merged.

## Normalization the serialization commits to

The canonical form stores marks **normalized**; discovery order (parser walk) is
not canonical, so serialization sorts by `(start, end, kind-ord, attrs)` first.
Three rules, and nothing more:

1. **Same-kind formatting marks union** when adjacent *or* overlapping
   (`[0,3)`+`[3,6)`+`[1,4)` strong → one `[0,6)` strong).
2. **Different-kind marks overlap freely** — never split into non-overlapping
   runs (Peritext's free-overlap, not a run model). `strong[0,4)` and
   `emph[2,6)` are stored as-is.
3. **Identity marks never merge** — two anchors over identical ranges stay two.

`normalize_marks` (`model.rs`) is idempotent and is the whole commitment. It
does **not** encode:

- **Edge-expand** — whether typing at a bold boundary extends the bold. This is
  a *typing-time* decision the editor owns; the model only ever sees the
  resulting range. ProseMirror configures it per-mark (`inclusive`), Lexical
  derives it from the caret's active format set, Quill from Delta attribute
  carry. The model refuses to arbitrate: it stores what the editor produced.
- **Adjacent-merge at insertion** — the editor may merge as you type; the model
  merges at normalization regardless, so the stored form is identical whatever
  the editor did. This is why the freeze is editor-independent: the disagreement
  lives entirely in behavior the model does not represent.

## Split / join — the most common prose edit

The line tree is **derived** from `\n` boundaries in `text`, never stored. A
paragraph split is a **single `\n` insert**; a join is a single `\n` delete.
Marks are ranges over the corpus, so they ride the splice mechanically:

- Splitting mid-mark keeps the mark (it now spans the new line boundary) — no
  identity crisis over "which half keeps the ID", because there are **no
  paragraph IDs**.
- A zero-width `anchor` at the split point survives as a zero-width point (After
  bias keeps it on the following text).

## The stale-text writer and the move weak spot

An MCP `update_document` or a saved `.qmd` is a full new markdown document. The
rebase is: cold-parse → corpus → char-diff against the base corpus → delta →
rebase every mark/anchor through it (CodeMirror `ChangeDesc.mapPos` semantics).
No preservation contract on the LLM.

- An **edit around** an annotation (prose rewritten, the anchored word kept)
  rebases cleanly — the anchor lands back on its word (`Kept`/`Shrunk`).
- A **paragraph reorder** is `delete`-here + `insert`-there to any char differ
  (it has no move op). A naive position rebase collapses an anchor in the moved
  text to the deletion point — **detached** from what it annotated. This is the
  weak spot, and it lands on the flagship writer (LLM "reorganize/tighten").

**Policy: confine, don't pretend.** A longest-common-substring move detector
over the delta's deleted-vs-inserted text re-homes an anchor onto its moved
block (`detect_move` / `rebase_anchor_move_aware`, `diff.rs`). It handles the
common case — a single, near-verbatim block move — and degrades gracefully:

- a move that is **also edited mid-block** shrinks the matched run or misses,
  dropping back to the detach behavior;
- **multiple simultaneous moves** detect only the longest.

The residual drop is **accepted and stated**, not hidden: annotations on text
that is both moved and rewritten in one stale-text round may detach. Form
editors that emit deltas natively never hit this path; only the cold-diff writer
does.

## USV coordinates — the cross-binding tax

Positions count Unicode scalar values (Rust `char`), never bytes, never UTF-16
units. The property suite owns the conversions at every binding boundary
(`usv.rs`): an astral char is 1 USV / 4 UTF-8 bytes / 2 UTF-16 units, and a
UTF-16 index landing mid-surrogate rounds down to its owning char. JS editors
count UTF-16, so every delta crossing WASM/Python converts at the seam.

## Residual gate

Phase 1's serialization may freeze on this contract now: it commits only to the
mark set + the three normalization rules + USV coordinates, all validated here,
and delegates every semantics editors disagree on to the editor. The one owed
step before phase 3 builds the editor binding: **bind one real rich editor**
(ProseMirror or Lexical) and confirm no editor *forces* an edge-expand or
adjacent-merge semantic back into the model. If one does, it enters as editor
config, not a serialization change — but that confirmation has not run.
