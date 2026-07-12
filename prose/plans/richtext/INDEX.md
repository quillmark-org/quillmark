# Richtext rework — integration HQ

Working plan for the content-model rework tracked in
[#831](https://github.com/quillmark-org/quillmark/issues/831). This branch
(`integration/richtext`) is the long-range integration point; phases land here
behind their spike gates, not on `main` piecemeal.

## Status

Phases 0–2 are landed, **including PR-G** — `richtext(inline)` enforcement
(coercion + validation + load-time), load-time schema-example import into a
`#[serde(skip)]` corpus cache, seed-commits-corpus, and the hard `markdown`-alias
cutover (`type: markdown` is now a schema load error, not a silent alias).
PR-G's spec lives in [phase-2.md](phase-2.md); the landed behavior lives in
`prose/canon/` (SCHEMAS.md). **Phase 3 (edit surface) — PR-B–H landed** on
`integration/richtext` (PR-H findings; harness on `spike/richtext-phase-3`).
See [phase-3.md](phase-3.md) and [PREVIEW.md](../../canon/PREVIEW.md).

For how the system works *today* — the `RichText` corpus, the seam, storage,
schema surface, navigation — see `prose/canon/` (ARCHITECTURE, CONVERT,
PLATE_DATA, SCHEMAS, PREVIEW, DOCUMENT_STORAGE). This HQ records only
direction and sequencing, not implemented behavior.

## Objective

Replace markdown-string content fields with a canonical corpus value —
`RichText`: one text sequence per field carrying line attributes, anchored
marks, and embedded islands — and demote markdown to a projection (import /
export codecs). A web form with rich prose fields is the primary authoring
surface; the LLM/MCP whole-document markdown flow and human-authored markdown
documents stay co-equal writers; a Notion-class block canvas is a non-goal.

The full model spec — `RichText` shape, lines / marks / islands, codecs,
storage, schema — lives in the body of
[#831](https://github.com/quillmark-org/quillmark/issues/831). This HQ is the
canonical direction: what is decided, and how the work is sequenced; it does
not restate the model.

## Decided

- **Model is a corpus, not a block tree** (settled #831): one `RichText` per
  richtext field over a USV coordinate space — `text` + `lines` + `marks` +
  `islands`, every edit a splice. Superseded the #830 block tree.
- **Seam is structured RichText-JSON ("Option A")**, never a markdown string,
  across `Backend::open(source, json_data)`. Landed in phase 2 (PR-E). A typed
  `Document` seam ("Option C") stays available as a later, non-urgent backend
  refactor — not ruled out, just not needed yet.
- **Type name is `richtext`** at every author-facing surface and in code
  (`RichText`); `type: markdown` is a schema load error (PR-G cutover).
  Current surface: SCHEMAS.md.

## Phase map

- **[Phase 0 — spikes](phase-0.md).** Landed, no red flag. De-risked mark
  semantics, source-map inversion, and seam/determinism before phase 1 froze
  anything.
- **[Phase 1 — type + codecs, engine-off](phase-1.md).** Landed. `RichText` +
  canonical serialization + markdown⇄corpus codecs, in `crates/richtext`,
  engine untouched.
- **[Phase 2 — engine consumes RichText](phase-2.md) (delivered
  [#829](https://github.com/quillmark-org/quillmark/issues/829)).** Landed
  through PR-G: seam flip to corpus JSON, typst emitter + segment maps,
  storage cutover, regions + navigation (`locate`/`position_at`), and PR-G's
  `richtext(inline)` + load-time example cache + `markdown`-alias cutover.
- **[Phase 3 — edit surface](phase-3.md).** Landed (PR-B–H). Per-field delta
  (`retain`/`insert`/`delete` text splices, CodeMirror `ChangeSet` semantics;
  marks via separate op channels) + monotonic revision + bounded change log;
  form-editor binding on phase-0's frozen mark semantics. PR-A + PR-H probes on
  `spike/richtext-phase-3` — Spike-A closed the phase-0 residual gate (no model
  change); form POC confirms whole-doc `LiveSession.apply` + region
  cross-navigation for inline fields. PR-B–E landed the Myers diff, change log,
  mark/line ops, and fallible document mutators; PR-F/G landed the preview wire
  and revision stamp (`LiveSession.revision`, `applyFieldDelta`, `mapFieldPos`);
  PR-H landed the fixture and runtime nav docs. **#886 later removed the
  change log and everything built on it** — `revision`, the field-delta path
  (`applyFieldDelta` / `mapFieldPos`), and the geometry-read `revision` stamp —
  moving cross-edit position anchoring to the editor's own transaction mapping;
  the Myers diff, mark/line ops, and document mutators (the corpus substrate)
  stay. See
  [PREVIEW.md](../../canon/PREVIEW.md) for the landed edit-surface contract.
- **Phase 4 — islands + collab.** First real island type (tables, with
  per-creation id minting rather than import's sequential ids), then a
  text-CRDT sync binding if wanted; core stays CRDT-free.

Sequencing invariant: nothing a later phase needs is frozen before the
phase-0 spike that validates it, and no phase discards another's output.

## Related

- #831 (this rework), #830 (block-tree predecessor, superseded), #829
  (regions, delivered by phase 2)
- `prose/canon/DOCUMENT_STORAGE.md`, `QUILL_VALUE.md`, `PREVIEW.md`,
  `CONVERT.md`, `PLATE_DATA.md`, `SCHEMAS.md`
