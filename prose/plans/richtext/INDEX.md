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

**Confirmed by [phase 0, Spike C](phase-0-finding-c-seam.md):** canonical
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

Each takeaway below states the risk and — since **[phase 0](phase-0-spikes.md)
reported (no red flag)** — what its spike established. Full evidence in the
[findings](phase-0-spikes.md); every claim maps to an assertion in
`crates/richtext-spikes/tests/`.

- **Freeze ordering is the pivotal risk.** Canonical `RichText` serialization
  (phase 1) fixes the mark set and its overlap / edge / identity semantics for
  storage. Editors (ProseMirror / Lexical / Quill) disagree precisely there.
  The editor-binding spike must therefore **precede** the serialization freeze,
  not follow it (issue text puts it in step 3). Hence phase 0. The open mark set
  absorbs new mark *types*, not changed semantics of the known ones.
  → **Phase 0 ([A](phase-0-finding-a-editor.md)):** the freeze is
  **editor-independent by construction** — the model commits only to the mark
  set + three normalization rules (same-kind union, cross-kind free overlap,
  identity never merges) and *deliberately does not encode* the two things
  editors disagree on (edge-expand, adjacent-merge), delegating both to the
  editor. So phase 1 may freeze now; the live-editor binding downgrades from a
  hard blocker to a **residual confirmation gate** before phase 3 (verify no
  editor forces a semantic back into the model). No live JS editor ran here (no
  browser); the model-side contract and the diff-rebase risk did.
- **Source-map production is backend-internal and deferrable; the seam encoding
  is not.** The `locate` / `position_at` / region API keys on
  `(field, corpus range, revision)` regardless of how `typst` builds the map. But
  the map is a codegen artifact — if content crosses as a string, `typst`
  re-parses to build it, founding #829's deliverable on the per-render parse the
  model exists to kill. Faithful encoding is required from the step that first
  needs the map (phase 2).
  → **Phase 0 ([B](phase-0-finding-b-sourcemap.md)):** the per-run map is
  **invertible by recomputation, no stored per-character table** — a pure
  function of the run text. The span→byte→field chain resolves on a real render
  via the shipped `regions()`/`field_at()`.
- **Navigation is cluster-exact, not character-exact.** Point↔corpus resolves at
  the shaping cluster (the resolution a real caret has), inverting the source map
  through `escape_markup` and the glyph's intra-node offset (`glyph.span.1`,
  unused today at `span_scan.rs:197`). Ink with no corpus origin (list markers,
  package numbering, plate decorations) is nav-ignored. `regions()` cannot ignore
  it — the run-machine rework (grounding §3.2) is still owed for highlight boxes.
  → **Phase 0 ([B](phase-0-finding-b-sourcemap.md)):** confirmed cluster-exact
  (every generated byte of a multi-byte escape inverts to its one corpus char).
  `escape_markup` is character-local **except the `//`→`\/\/` rule**, a 2-char
  coupling a 1-char lookahead absorbs — the sole run-alignment hazard, guarded
  by cross-checking the reimplemented escaper against the shipped one (a test
  that fails the moment the backend grows an escape rule). Character precision
  is one mechanical step in phase 2: add `glyph.span.1` to the resolved node
  range, then invert — no new Typst capability, no per-render parse.
- **Determinism has an honest boundary.** Migration is mint-free (legacy bodies
  hold no islands). Island IDs are minted at creation, so once tables ship
  (phase 4) hashing of island-bearing documents inherits mint-nondeterminism;
  the content-hash contract must tolerate it. Text stays deterministic
  throughout.
  → **Phase 0 ([C](phase-0-finding-c-seam.md)):** confirmed island mint is the
  **sole** hash nondeterminism (text/marks/lines hash identically across cold
  imports, so migration is mint-free). One new carry-forward: the workspace's
  `serde_json` `preserve_order` means island `props` object-key order (and
  float formatting, when non-string props arrive) **leaks into the content hash
  unless recursively canonicalized** — phase 2 must sort keys before hashing.
- **The move-annotation weak spot lands on the flagship writer.** A stale-text
  writer (MCP `update_document`, saved `.qmd`) rebases via cold-parse + corpus
  diff. A reorder — common in LLM "reorganize/tighten" rewrites — is delete +
  insert to the differ, so annotations on moved text drop. Not a rare
  concurrency corner; confine it with move detection in the differ, and state
  the limit.
  → **Phase 0 ([A](phase-0-finding-a-editor.md)):** confirmed — a naive rebase
  detaches an anchor in moved text; a longest-common-substring move detector
  re-homes it. Precise limit: it handles a **single, near-verbatim block move**
  and degrades gracefully (text both *moved and rewritten* in one round shrinks
  the match or misses, dropping back to detach; multiple moves detect only the
  longest). The residual drop is **accepted and stated**, not hidden.
- **USV coordinates are a standing cross-binding tax.** JS editors are UTF-16,
  Rust is UTF-8; every delta crossing WASM/Python and every editor binding
  converts at its boundary. The property suite owns surrogate-pair / astral-plane
  offset correctness explicitly.
  → **Phase 0:** conversions validated across astral characters, including a
  UTF-16 index landing mid-surrogate (rounds down to its owning char).

## Phase map

Detail per phase in its own doc as it opens. Rough shape:

- **[Phase 0 — spikes](phase-0-spikes.md)** — de-risk the freezes before any
  schema lands. Editor binding (gates mark semantics), source-map/navigation
  inversion (gates the phase-2 emit design), seam + determinism prototype (gates
  Option A). Nothing here ships to `main`. **Reported — no red flag**
  ([A](phase-0-finding-a-editor.md) · [B](phase-0-finding-b-sourcemap.md) ·
  [C](phase-0-finding-c-seam.md); runnable probe `crates/richtext-spikes/`).
  Phase 1 may open on the Spike-A contract, with a live-editor binding tracked
  as its residual gate.
- **Phase 1 — type + codecs, engine-off.** `RichText` + canonical serialization
  (frozen on the [Spike-A contract](phase-0-finding-a-editor.md): mark set +
  three normalization rules) + markdown⇄corpus import/export codecs + property
  suite (round-trip modulo loss class; diff-import preserves marks/islands).
  Exercised against the fixture corpus; engine untouched. Carry from phase 0:
  the move detector and USV conversions have working shapes in
  `crates/richtext-spikes/` to port and harden.
- **Phase 2 — engine consumes RichText (delivers #829).** Seam carries
  RichText-JSON (Option A, [confirmed](phase-0-finding-c-seam.md)); `typst` emit
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
