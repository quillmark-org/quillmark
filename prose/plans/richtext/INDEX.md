# Richtext rework — integration HQ

Working plan for the content-model rework tracked in
[#831](https://github.com/quillmark-org/quillmark/issues/831). This branch
(`integration/richtext`) is the long-range integration point; phases land here
behind their spike gates, not on `main` piecemeal.

## Objective

Replace markdown-string content fields with a canonical corpus value —
`RichText`: one text sequence per field carrying line attributes, anchored
marks, and embedded islands — and demote markdown to a projection (import /
export codecs). Delivers #829's paragraph-level regions as the step-2
degenerate case. Product frame: a web form with rich prose fields is the
primary authoring surface; the LLM/MCP whole-document markdown flow and human
`.qmd` files stay co-equal writers; a Notion-class block canvas is a non-goal.

The full model spec — `RichText` shape, lines / marks / islands, codecs,
storage, schema — lives in the body of
[#831](https://github.com/quillmark-org/quillmark/issues/831). This HQ is the
canonical direction: it records what is **decided**, the naming, and how the
work is **sequenced**; it does not restate the model. The earlier working docs
on `claude/issue-830-context-uuia5t` (proposal + #830 reviews) are superseded —
their live conclusions are distilled here.

## Locked decisions

### Model — corpus, not block tree

Settled in #831. One `RichText` per richtext field over a single coordinate
space (Unicode scalar values): `text` + `lines` + `marks` + `islands`, every
edit a splice. Supersedes the #830 block tree; the Peritext/Automerge research
#830 cited points at the corpus shape, not the tree.

### Seam — Option A: structured RichText-JSON across the data seam

The core→backend seam (`Backend::open(source, json_data)`, `core/src/backend.rs`)
stays a **data** contract; content crosses it as **structured RichText-JSON**,
not a markdown string.

Grounding for the call:

- **Codegen is not a universal seam.** `typst` is a source backend (lowers JSON
  → Typst dict literal + `#let` content blocks); `pdfform` is a data backend
  (`pdfform/src/lib.rs`, `resolve_field_specs` stamps `json_data` values into an
  AcroForm — no codegen, none possible). A codegen seam would force `pdfform` to
  un-lower source back to values.
- **Three tiers, named.** The seam is the *data model*; codegen is `typst`'s
  private lowering of it (and the only place the source map can be produced);
  JSON is the *serialization* of the model. Content-as-markdown-string was a
  lossy encoding *inside* the data seam, not a codegen seam.
- **Why A over a typed-`Document` seam (Option C).** Keep JSON as the
  language-agnostic cross-backend contract: bindings already serialize it,
  `PLATE_DATA.md` publishes it, and both backends lower from it uniformly
  (`typst` → source + map; `pdfform` → plaintext via `RichText.text` minus
  island slots). C (promote the seam to the typed model, demote JSON to storage
  serialization) is cleaner conceptually but rewrites the `Backend` trait and
  `pdfform`'s field resolution. Because both A and C carry richtext
  *faithfully*, A↔C is a later backend refactor that changes no session API —
  so A is the low-regret starting point.
- **`pdfform` cost is zero today.** No pdfform fixture binds a content field
  (`sample_form` sets `body.enabled: false`; all fields are scalar). Option A
  ships with a trivial `RichText → .text` lowering and no fixture churn.

The durable contract is **"richtext crosses the seam faithfully"** (never as a
lossy markdown string). The refactorable detail is the encoding (JSON vs typed).

**Confirmed by [phase 0](phase-0.md) (Spike C):** canonical
RichText-JSON serializes byte-deterministically, the seam JSON and the storage
serialization are the *same* canonical bytes (one encoding, not two to keep
aligned), and both backends lower trivially — `typst` via the shipped
`mark_to_typst`, `pdfform` via `RichText.text` minus island slots, with zero
`sample_form` fixture churn.

### Naming — `richtext`

The type is `richtext` at every author-facing surface (`type: richtext`,
`array<richtext>`, `richtext(inline)`, blueprint `# richtext<markdown>`) and in
code (`RichText`); **corpus** stays internal shape vocabulary. The blueprint
format slot carries the surface encoding (`# richtext<markdown>`) — the type
names the role, the refinement names the syntax. Follows the `datetime`
one-word precedent. Rejected, each with the criterion it failed: `markdown`
(names one projection's encoding, not the role), `content` (saturated;
permanent `typst::foundations::Content` collision), `prose` (undersells
lists/tables/islands), `corpus` (implementation shape; outsider prior is a
*collection* of documents), bare `rich` (still live as an ordinary adjective).

## Technical takeaways carried into the phases

Risk register. Each item states the risk in one line; a resolved item collapses
to its status and points at the phase doc that owns the detail. The [phase
0](phase-0.md) items reported with no red flag; the source of truth for their
results is `phase-0.md` (and the assertions in `crates/richtext-spikes/tests/`),
not this list.

- **Freeze ordering.** Phase-1 serialization fixes the mark set + overlap / edge
  / identity semantics, exactly where ProseMirror / Lexical / Quill disagree —
  so the editor spike must *precede* the freeze (hence phase 0). The open mark
  set absorbs new mark *types*, not changed semantics of known ones.
  → **Resolved (phase 0·A):** editor-independent by construction — the model
  encodes only the mark set + three normalization rules and delegates
  edge-expand / adjacent-merge to the editor. Phase 1 may freeze now; a live
  binding is the residual gate ([phase-0.md](phase-0.md#residual-gate)).
- **Seam encoding, not map production.** `locate` / `position_at` / regions key
  on `(field, corpus range, revision)` however `typst` builds the map — but the
  map is a codegen artifact, so content must cross the seam *faithfully* (not as
  a re-parsed string) from phase 2 on. Map production itself is deferrable.
  → Encoding confirmed under **Seam — Option A** above (phase 0·C).
- **Navigation is cluster-exact, not character-exact.** Point↔corpus resolves at
  the shaping cluster, inverting the source map through `escape_markup` and the
  glyph's intra-node offset (`glyph.span.1`, unused at `span_scan.rs:197`).
  Origin-less ink (list markers, numbering, decorations) is nav-ignored;
  `regions()` still owes the run-machine rework (grounding §3.2) for highlight
  boxes. → **Resolved (phase 0·B):** cluster-exact, invertible by recomputation;
  `escape_markup` char-local except the `//`→`\/\/` coupling. See
  [phase-0.md](phase-0.md).
- **Determinism has an honest boundary.** Migration is mint-free (legacy bodies
  hold no islands); island IDs mint at creation, so phase-4 hashing of
  island-bearing documents inherits mint-nondeterminism. Text stays
  deterministic. → **Resolved (phase 0·C):** mint is the *sole* source; **new
  carry-forward** — `serde_json` `preserve_order` leaks island `props` key order
  into the hash unless keys are recursively sorted before hashing (phase 2).
- **The move-annotation weak spot lands on the flagship writer.** The stale-text
  writer (MCP `update_document`, saved `.qmd`) rebases via cold-parse + corpus
  diff; a reorder is delete+insert, so annotations on moved text drop unless a
  move detector confines it. → **Resolved (phase 0·A):** move detector re-homes
  a single near-verbatim block move; residual drop (moved *and* rewritten)
  accepted and stated. See [phase-0.md](phase-0.md).
- **USV coordinates are a standing cross-binding tax.** JS editors are UTF-16,
  Rust UTF-8; every delta crossing WASM/Python converts at its boundary, and the
  property suite owns surrogate-pair / astral-plane correctness. → Validated in
  phase 0 (mid-surrogate rounds down to its owning char).

## Phase map

Detail per phase in its own doc as it opens. Rough shape:

- **[Phase 0 — spikes](phase-0.md)** — de-risk the freezes before any
  schema lands. Editor binding (gates mark semantics), source-map/navigation
  inversion (gates the phase-2 emit design), seam + determinism prototype (gates
  Option A). Nothing here ships to `main`. **Reported — no red flag** (see
  [phase-0.md](phase-0.md); runnable probe `crates/richtext-spikes/`). Phase 1
  may open on the Spike-A contract, with a live-editor binding tracked as its
  residual gate.
- **Phase 1 — type + codecs, engine-off.** `RichText` + canonical serialization
  (frozen on the [Spike-A contract](phase-0.md): mark set + three normalization
  rules) + markdown⇄corpus import/export codecs +
  property suite (round-trip modulo loss class; diff-import preserves
  marks/islands). Exercised against the fixture corpus; engine untouched. Carry
  from phase 0: the move detector and USV conversions have working shapes in
  `crates/richtext-spikes/` to port and harden.
- **Phase 2 — engine consumes RichText (delivers #829).** Seam carries
  RichText-JSON (Option A, [confirmed](phase-0.md)); `typst` emit
  with per-line windows + source map; `pdfform` `.text` lowering; `locate` /
  `position_at` + region re-key on `(field, corpus range, revision)`; storage
  cutover (new `StoredDocument` version, deterministic cold-import migration).
  Two carry-forward items from phase 0: **(a)** character-precision nav = add
  `glyph.span.1` to the resolved node range then invert the run map (Spike B);
  **(b)** recursively canonicalize island `props` keys before hashing, or
  `preserve_order` leaks insertion order into the content hash (Spike C).
- **Phase 3 — edit surface.** Per-field delta (Quill-Delta semantics) + monotonic
  revision + bounded change log with position mapping; form-editor binding built
  on the phase-0 spike's frozen semantics. Opens the **residual Spike-A gate**:
  bind one real rich editor and confirm no editor forces an edge-expand /
  adjacent-merge semantic back into the model (if one does, it enters as editor
  config, not a serialization change).
- **Phase 4 — islands + collab.** First real island type (tables), then a
  text-CRDT sync binding when wanted; core stays CRDT-free.

Sequencing invariant: nothing a later phase needs is frozen before the phase-0
spike that validates it, and no phase discards another's output.

## Related

- #831 (this rework), #830 (block-tree predecessor, superseded), #829 (regions,
  delivered by phase 2)
- `prose/canon/DOCUMENT_STORAGE.md`, `QUILL_VALUE.md`, `PREVIEW.md`,
  `CONVERT.md`, `PLATE_DATA.md`
- `crates/core/src/backend.rs`, `crates/core/src/region.rs`,
  `crates/backends/typst/src/overlay/span_scan.rs`,
  `crates/backends/typst/src/helper.rs`
