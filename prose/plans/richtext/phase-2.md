# Phase 2 — engine consumes RichText (delivers #829)

The engine stops passing markdown strings and consumes the corpus. `Card.body`
is `RichText`; the seam carries canonical RichText-JSON; the `typst` backend
lowers the corpus to markup while recording a per-segment source map; storage
embeds the corpus structurally; regions key on `(field, corpus range)` and
navigation exposes `locate` / `position_at`. This is the phase that delivers
[#829](https://github.com/quillmark-org/quillmark/issues/829)'s paragraph-level
regions — as the degenerate case of the segment map, not a special path.

Gated on the [phase-0 spikes](phase-0.md) (all reported, no red flag) and the
[phase-1 freeze](phase-1.md) (landed).

**Status: complete.** PR-A through PR-F landed and merged into
`integration/richtext` (#838, #839); PR-G lands the final piece
(`richtext(inline)` enforcement, load-time example import + corpus cache,
seed-commits-corpus, hard `markdown`-alias cutover). The behavior lives in canon
(SCHEMAS.md); Phase 3 (edit surface) is the next open work — see
[INDEX.md](INDEX.md).

## Landed

1. **PR-A — leaf-crate arrow inversion.** `normalize_markdown` +
   `MAX_NESTING_DEPTH` relocate into `quillmark-richtext`; `core` depends on
   it (arrow inverted from phase 1); richtext flips `publish = true`.
2. **PR-B — live model `Card.body: RichText`.** `from_markdown` imports /
   `to_markdown` exports; `is_blank()`; over-nested bodies →
   `ParseError::BodyImport`. Bindings and wire stayed markdown-string, bridged
   through `export`, until PR-E.
3. **PR-C — storage cutover, `quillmark/document@0.93.0`.**
   `CanonicalRichText` newtype whose serde *is* the canonical serializer; a
   fallible 92→93 cold-import migration; goldens pin the body subtree
   byte-identical to `content_key`.
4. **PR-D — Typst emitter (`emit.rs`) + segment maps.** `emit_richtext` walks
   the corpus to markup plus per-segment source maps, built and
   parity-tested (103/103) against the still-live `mark_to_typst` oracle
   before anything switched over.
5. **Option A — structured table cells.** Cells carry inline `{text, marks}`
   instead of a raw markdown slice; parity reaches 116/116.
6. **PR-E — seam flip.** `compile_data`/`to_plate_json` emit canonical corpus
   JSON; the typst backend consumes it via `emit_richtext`; **`mark_to_typst`,
   `convert.rs`, and the backend's `pulldown-cmark` dependency are deleted**;
   `FieldType::Markdown` → `RichText { inline }` (`markdown` kept as a
   deprecated alias, retired in PR-G); bindings flip `card.body` to corpus
   JSON and add `card.bodyMarkdown`; pdfform lowers to plaintext.
7. **PR-F — regions + navigation (#829).** Two-tier `(window, segment)` scan;
   `RenderedRegion.span`; `position_at` / `locate`. Landed carrying two
   corrections a pre-landing spike surfaced: run-machine transparency (the
   `(window, None)` no-op arm) is scoped to the **same window** as the
   currently-accruing segment, not global — a different field's own
   structural ink must still suspend the current run, or an interleaved
   second placement silently merges into one lying box. And `glyph.span.1`
   inversion degrades to the **segment's** start, not the resolved node's
   start, inside a multi-line `#raw` code fence — every physical line shares
   one node wider than any per-line run, so per-run inversion is a structural
   non-starter there (not merely imprecise), and node-start would point at
   bytes outside every line's own text.
8. **PR-G — `richtext(inline)`, load-time example cache, alias cutover.**
   `RichText::is_inline()` (one `Para` line, no container, no islands),
   enforced at coercion, validation (`richtext::not_inline`, `TypeMismatch`
   fatality), and load-time example import. `QuillConfig::from_yaml` imports
   every richtext `default`/`example`/`body.example` once into `#[serde(skip)]`
   corpus companions (a pure function of the Quill.yaml bytes) — the authored
   markdown literal is retained as the canonical projection, the corpus is the
   derived cache. Seed-commits-corpus: seeding and the render floor read the
   cache, so seeded documents and zero-fills are corpus (retires
   `import_body_lossy`; `zero_value` for richtext is the empty corpus). Hard
   cutover: `type: markdown` is a schema **load error**, not a silent alias.

All landed code and its rationale now live in canon (ARCHITECTURE.md,
DOCUMENT_STORAGE.md, CONVERT.md, PLATE_DATA.md, PREVIEW.md, SCHEMAS.md,
BLUEPRINT.md) — this doc no longer re-describes it.

## Design spine

### The pivot — one parse site, at ingest

Five of the landed decisions hinge on one move: the markdown parse crosses
from render time to ingest time, and the corpus is the only in-memory content
model. `quillmark-richtext` stays a separate leaf crate — holding the model,
canonical serialization, edit deltas, and the markdown codecs — but the
dependency arrow inverts from phase 1: **core now depends on richtext**, not
the other way around. The `typst` backend never re-parses; it walks the
corpus.

> The markdown engine appears exactly once in the workspace, in
> `quillmark-richtext::import`. No render path parses markdown.

Net parser count is unchanged (one), moved from every render to each ingest.
This is the invariant every later PR had to preserve, and the one PR-G's
alias cutover closed off.

### Seam + storage — one canonical form, three consumers

The phase-1 canonical serializer (`serial.rs`) is the single encoding for
storage, the seam, and `content_key` — a corpus becomes JSON only ever
through it. Storage embeds the corpus structurally inside the envelope (not
as an escaped string), sorted; the envelope itself stays insertion-ordered,
because payload field order is authored content, not a hash input. Considered
and rejected at design time: an embedded canonical-JSON *string*
(double-encoded, un-inspectable), a whole-envelope key-sort (reorders
authors' field maps), and a hand-mirrored DTO tree (a second copy of the
frozen serializer, free to drift). Mechanism, migration steps, and the
byte-stability contract are in DOCUMENT_STORAGE.md.

### Crate layering

`quillmark-richtext` stays a leaf crate `core` depends on, rather than
dissolving into `core::richtext` — the frozen wire contract (a
schema-version-bumping golden) stays isolated from the large engine crate's
blast radius, and a codec crate layered *above* core would be circular (both
the storage `TryFrom` chain and `Document::from_markdown` need `import`
inside or below core). See ARCHITECTURE.md.

### Emit, regions, navigation

The Typst emitter, the two-tier segment scan, and `position_at`/`locate` are
documented in full in CONVERT.md and PREVIEW.md — read those for "how it
works now." One piece of durable rationale worth keeping here, since it still
governs Phase 3: **revision defers past Phase 2.** The region key is `(field,
corpus range)` with no revision counter — `apply` is transactional and the
consumer single-owner and serial, so there is no cross-edit reader for a
counter to protect. A revision earns its keep only in Phase 3, alongside the
change-log, when a stale position must be *mapped* forward
(`delta::map_pos`), not merely detected. `RenderedRegion` is
additive-optional by construction so Phase 3 can append `revision` without a
break.

## Sequencing invariant

Nothing embeds the canonical bytes before they are re-pinned in core: A
before C and E — one freeze (`serial.rs`), three consumers (storage, seam,
`content_key`). The storage DTO froze forever at C, with its migration
goldens and the two-discipline bytes rule landing in the same PR.
`mark_to_typst` was not deleted before D's (then Option-A's) parity suite
went green. `RenderedRegion`'s wire shape froze additive-optional at F, so
Phase 3 extends rather than breaks it.

## Risk register

Only risks still open — parser-parity, emitter-parity, and
pkg/core-bundle-size risks closed at PR-D/E/F landing (see git history if the
numbers are needed again).

1. **Migrated-blob byte-stability is conditional on `pulldown-cmark`.**
   Cross-release byte-stability of *migrated* (not freshly-authored) rows
   depends on `pulldown-cmark` parsing identically release to release. A
   forced security/version bump on the parser can move migrated bytes even
   though the schema tag doesn't change. Mitigation on record in
   DOCUMENT_STORAGE.md: pin the exact version, ship golden migration
   fixtures as a tripwire, recommend read-repair (rewrite rows post-migration
   so they leave the conditional-stability class).
2. **String-authored richtext *fields* re-import per `compile_data`.** Unlike
   `$body` (a typed corpus on `Card`, never re-parsed), a richtext field
   authored as a string in a markdown document imports at each
   `compile_data` — the same tier as date parsing, deterministic, so its
   regions' corpus ranges are stable, but nav precision for it is
   field-level until the field is stored structurally. Watch `usaf_memo`'s
   `references: array<richtext>` field for cost and correctness as a first
   real user of this path.

## Related

- #831 (this rework), #829 (regions, delivered here), #830 (block-tree
  predecessor, superseded), #801 (span-excluding `page_hashes`, preserved)
- [INDEX.md](INDEX.md), [phase-0.md](phase-0.md), [phase-1.md](phase-1.md)
- `prose/canon/`: ARCHITECTURE.md, DOCUMENT_STORAGE.md, CONVERT.md,
  PLATE_DATA.md, PREVIEW.md, SCHEMAS.md, BLUEPRINT.md
