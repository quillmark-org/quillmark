# Phase 0 — spikes

Throwaway probes that answer the questions a schema freeze cannot cheaply
revise. Nothing here lands on `main`; each spike exits with a finding that
unblocks a specific later freeze. The three gate different phases and share no
code.

The one rule: **no canonical `RichText` serialization is written until Spike A
reports.** Phase 1 freezes the mark set and its semantics for storage; the
editor spike is what tells us those semantics are the ones a real editor needs.

**Status: all three reported. No red flag.** The runnable probe is
`crates/richtext-spikes/` — a workspace member held outside `default-members`,
so `cargo test --workspace` runs it while a bare `cargo test` skips it. Every
claim in a finding below maps to an assertion in `tests/spike_{a,b,c}_*.rs`.
Delete the crate when the phases it de-risks land.

---

## Spike A — mark semantics (gates the phase-1 serialization freeze)

**Question.** Which mark semantics must the model encode, and which does it
leave to the editor? ProseMirror / Lexical / Quill disagree exactly where the
canonical serialization commits: mark identity across split/join, adjacent
same-type merge, and edge-expand (does typing at a bold boundary extend it?).

**Scope note.** No browser here, so the spike validates the two halves it can
run deterministically — the **model-side normalization contract** and the
**stale-text writer's annotation rebase** (the novel, project-specific risk) —
in Rust, and derives the editor-behavior half from the editors' documented
semantics rather than a live binding. That is sound because the contract's
central move is to *not encode* what editors disagree on; a live binding is the
one residual gate (below).

### The frozen mark set

`strong` · `emph` · `underline` · `strike` · `code` · `link { url }` — the
round-trippable projection marks — plus `anchor { id }`, a **zero-width identity
mark** carrying a comment thread or stable anchor. The set is **open**: an
unknown kind round-trips opaque, absorbed as a new *type*, never a changed
semantics of a known one. Two classes with different algebra:

- **Formatting** (`strong`…`link`): a property of a range. Two are redundant if
  they coincide.
- **Identity** (`anchor`): a handle, not a property. Two anchors over the same
  range are two distinct things (two comments), never merged.

### Normalization the serialization commits to

Marks are stored **normalized**; discovery order (parser walk) is not canonical,
so serialization sorts by `(start, end, kind-ord, attrs)` first. Three rules,
and nothing more:

1. **Same-kind formatting marks union** when adjacent *or* overlapping.
2. **Different-kind marks overlap freely** — never split into runs
   (Peritext free-overlap, not a run model).
3. **Identity marks never merge.**

It does **not** encode **edge-expand** or **adjacent-merge-at-insertion**: those
are typing-time decisions the editor owns (ProseMirror `inclusive`, Lexical's
active-format set, Quill Delta attribute carry). The model only ever stores the
resulting range, so the stored form is identical whatever the editor did —
**this is why the freeze is editor-independent.**

### Split / join

The line tree is derived from `\n` boundaries, never stored. A split is a single
`\n` insert, a join a single `\n` delete; marks are ranges and ride the splice.
Splitting mid-mark keeps the mark — no "which half keeps the ID" crisis, because
there are **no paragraph IDs**. A zero-width anchor at the split point survives
as a point.

### The stale-text writer and the move weak spot

An MCP `update_document` / saved `.qmd` is a full new document: cold-parse →
corpus → char-diff against the base → delta → rebase marks/anchors through it
(CodeMirror `ChangeDesc.mapPos` semantics). No preservation contract on the LLM.
An edit *around* an annotation rebases cleanly. A **paragraph reorder** is
delete-here + insert-there to any char differ, so a naive rebase collapses an
anchor in the moved text to the deletion point — **detached**. This lands on the
flagship writer (LLM "reorganize/tighten").

**Policy: confine, don't pretend.** A longest-common-substring move detector
re-homes an anchor onto its moved block. Limit, stated not hidden: it handles a
**single, near-verbatim block move**; text both *moved and rewritten* in one
round shrinks the match or misses (back to detach), and multiple moves detect
only the longest. The residual drop is accepted.

### USV coordinates

Positions count Unicode scalar values (Rust `char`) — never bytes, never UTF-16
units. The property suite owns the boundary conversions: an astral char is 1 USV
/ 4 UTF-8 bytes / 2 UTF-16 units, and a mid-surrogate UTF-16 index rounds down
to its owning char. JS editors count UTF-16, so every delta crossing WASM/Python
converts at the seam.

### Residual gate

Phase 1 may freeze on this contract now — it commits only to the mark set + the
three rules + USV, all validated, and delegates every disputed semantics to the
editor. The one owed step before phase 3 builds the editor binding: **bind one
real rich editor** and confirm none forces an edge-expand / adjacent-merge
semantic back into the model. If one does, it enters as editor config, not a
serialization change.

---

## Spike B — source-map inversion + navigation (gates the phase-2 emit design)

**Question.** Does glyph-offset inversion yield a usable point↔corpus map
through `escape_markup`, cluster-exact, on real typeset output — and is the
per-run map invertible by *recomputation*, with no per-character stored tables?

**Found: yes, cluster-exact, recomputation-only** — with one named coupling.

- **The escape transform is character-local except one 2-char coupling.**
  `escape_markup` is a per-char map (`*`→`\*`, …) except `//`→`\/\/`, a 2-char
  lookahead. The run map reconstructs per-char generated spans in one scan that
  absorbs it, and is cross-checked byte-for-byte against the shipped
  `escape_markup`. **That cross-check is the run-alignment tripwire** — it fails
  the moment the backend grows an escape rule the scan doesn't mirror. No other
  construct breaks alignment.
- **Invertible by recomputation, no stored tables.** `forward`/`invert` come
  from cumulative span lengths that are a pure function of the run text;
  rebuilding from the same text yields an identical map. A real emitter
  recomputes per run — it need not persist a character table.
- **Cluster-exact, never sub-char.** A char that escapes to several bytes
  (`*`→`\*`; `你`→3; `😀`→4) is one cluster: every byte of its span inverts to
  that single char. Say **cluster-exact**, not "character-exact", everywhere.
- **The chain resolves on real output.** Driving the actual Typst backend, the
  content field surfaces a positive-area region and a point inside it resolves
  back to the field via the shipped `regions()`/`field_at()` — exercising
  `glyph.span.0` → `DiagSpan`/`Source::range` → byte range → window containment.
- **Origin-less ink is nav-ignored.** A point off all field ink resolves to
  nothing — the forward counterpart to "list markers / package numbering are
  nav-ignored".

**Remaining phase-2 step (scoped, not a red flag).** Today's `Classifier`
resolves `glyph.span.0` to a node range and tests containment — field
granularity, all `regions()` needs. Character precision adds one mechanical
step: add `glyph.span.1` (the intra-node offset **unused today at
`span_scan.rs:197`**) to the range start for the exact generated byte, then
invert through the run map (proven cluster-exact). No new Typst capability, no
per-render parse — the map is a codegen artifact built once at emit.

**Out of scope.** The `regions()` run-machine rework for highlight boxes
(grounding §3.2) is phase-2 work; this spike touches navigation only.

---

## Spike C — seam encoding + determinism (confirms Option A, gates phase-2 storage)

**Question.** Does canonical RichText-JSON serialize byte-deterministically,
align with the storage serialization, and lower trivially to both backends —
and where does island-mint nondeterminism enter the content hash?

**Found: confirmed on all counts. No red flag for Option A.**

- **Byte-deterministic.** Equal values serialize identically, insensitive to
  mark/island order. Three sources closed: mark/island order (sorted before
  emit), object-key order in island `props` (recursively sorted — the
  workspace's `serde_json` `preserve_order` would otherwise leak insertion
  order into the bytes), float formatting (not exercised; flagged for real
  island types).
- **One encoding, not two to keep aligned.** Seam and storage are modelled as
  the one canonical form, and `deserialize ∘ serialize` is a fixed point on the
  canonical bytes. A future split (seam serializer ≠ storage serializer) is the
  drift this pins against.
- **Dual-lower is trivial.** `typst`: the corpus lowers via the shipped
  `mark_to_typst` (the converter the engine calls in `convert_content_value`).
  `pdfform`: `RichText.text` minus island slots — plaintext, marks/structure
  dropped; `sample_form` binds only scalars (`body.enabled: false`), so the path
  is unexercised by any fixture and ships with **zero fixture churn**.
- **The island-mint hash boundary.** Text/marks/lines hash identically across
  cold imports, so **migration is mint-free** (legacy bodies hold no islands).
  Island IDs are minted at creation: two values identical except the minted `id`
  hash differently; with `id` fixed the value is fully deterministic. This is
  the single boundary the content-hash contract must tolerate once tables ship
  (phase 4) — options then (not decided): derive IDs from content, or exclude
  minted IDs from the hash the way `page_hashes` excludes spans (#801).

---

## Gate

Phase 1 opens when Spike A reports; phase 2's emit design when Spike B reports;
its storage cutover when Spike C reports. A red-flag spike reshapes the phase it
gates before that phase starts — the point of running them first.

**All three reported; no red flag.** Phase 1 may open on the Spike-A contract
(live-editor binding tracked as its residual gate); phase 2's emit design and
storage cutover on Spikes B and C. Two items carry into phase 2: the
`glyph.span.1` character-precision step (B) and recursive `props` key
canonicalization before hashing (C).
