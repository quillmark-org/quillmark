# Phase 2 — engine consumes RichText (delivers #829)

The engine stops passing markdown strings and consumes the corpus. `Card.body`
becomes `RichText`; the seam carries canonical RichText-JSON (Option A); the
`typst` backend lowers the corpus to markup while recording a per-segment source
map; storage cuts over to a new `StoredDocument` version; regions re-key on
`(field, corpus range)` and navigation gains `locate` / `position_at`. This is
the phase that delivers [#829](https://github.com/quillmark-org/quillmark/issues/829)'s
paragraph-level regions — as the degenerate case of the segment map, not a
special path.

Gated on the [phase-0 spikes](phase-0.md) (all reported, no red flag) and the
[phase-1 freeze](phase-1.md) (landed). This doc is the grounding design for the
run-machine rework #829 needs — the reference the superseded #830-era "§3.2"
grounding pointed at, written here because that doc never migrated off the dead
branch.

**Status: planned.** No code has landed. The decisions below are settled; the
decomposition (§ Sub-PRs) is the landing order.

## The pivot — one parse site, at ingest

Five of the seven decisions hinge on one move: **the markdown parse crosses from
render time to ingest time, and the corpus is the only in-memory content model.**
`crates/richtext` stays a **separate crate** — `quillmark-richtext`, the leaf
holding the model, canonical serialization, edit deltas, and the markdown codecs
— but the dependency arrow **inverts**: phase 1 had richtext depend on core; now
**core depends on richtext**. `import` (and `pulldown-cmark`) live in that leaf,
reachable from the two core entry points that need it — the storage migration and
`Document::from_markdown`, which every binding (including the no-feature `pkg/core`
WASM build) uses to turn a `.qmd` body into the live model. The `typst` backend
**drops** its `pulldown-cmark` dependency; markup is produced by walking the
corpus, never by re-parsing.

Phase 1's handover item 1 ("re-home the *type* into core, keep the codecs
parser-side") is **superseded**: the type does *not* move into core — the codecs'
crate becomes the leaf core depends on, which places the model and its frozen
wire format one layer *below* the document engine (rationale in
[Crate re-homing](#crate-re-homing--keep-a-leaf-crate-core-depends-on)). The
"markdown-engine-free core" invariant (asserted only in a comment,
`core/Cargo.toml`) is not relaxed — it **inverts** into a stronger one:

> The markdown engine appears exactly once in the workspace, in
> `quillmark-richtext::import`. No render path parses markdown.

Net parser count is unchanged (one), moved from every render to each ingest.
**Landed on this branch** (the arrow-inversion groundwork): `normalize_markdown`
and `MAX_NESTING_DEPTH` are relocated into `quillmark-richtext`, `core` depends
on it, and richtext is `publish = true`. `pulldown` now sits in core's dependency
graph but is tree-shaken from the no-feature `pkg/core` build until a body/import
path is reachable (the phase-2 body cutover). The invariant lived only in that
comment and in `phase-1.md`'s prose — no canon doc states it — so recording the
flip is additive (ARCHITECTURE.md gains the statement; nothing is retracted).

## Locked decisions

### Seam + storage — one canonical form, three consumers

The phase-1 canonical serializer (`serial.rs`, recursive key-sort, feature-
independent bytes) is the **single** encoding for storage, the seam, and
`content_key`. A corpus becomes JSON only ever through it. Both A↔C seam options
stay open per Option A; nothing here forecloses the later typed-`Document` seam.

**Storage embeds the corpus structurally, two disciplines in one envelope.** The
`StoredDocument` envelope keeps today's contract exactly — compact `serde_json`,
frozen struct order, `Vec` order, payload `QuillValue`s in **insertion** order
via `preserve_order`, no sorting. The `body` subtree is the **canonical richtext
form** — recursively key-sorted, normalized mark/island order — embedded as a
nested object, not an escaped string.

The split is semantic, not stylistic: YAML mapping insertion order in payload
values is authored content that round-trips into re-emitted markdown, so sorting
the envelope would reorder authors' field maps on every save. Sortedness is
semantic *inside* the corpus; insertion order is semantic *outside* it —
different data, different disciplines.

- Rejected — **canonical JSON as an embedded string**: double-encoded, escape-
  bloated, parsed twice by every non-Rust consumer, and blobs stop being
  inspectable.
- Rejected — **whole-envelope key-sort**: reorders authored field maps (a content
  change), failing a semantic criterion, not a style one.
- Rejected — **a hand-mirrored `RichTextV0_NN_0` DTO tree**: a second copy of a
  freeze the phase-1 golden-bytes test already pins, free to drift from it.

Shape (`document/dto.rs`):

```rust
struct CardV0_NN_0 { payload: PayloadV0_NN_0, body: CanonicalRichText }

// newtype whose serde IS the canonical serializer — no parallel struct tree
struct CanonicalRichText(RichText);
//   Serialize   = sorted_value(serial::to_value(&self.0))
//   Deserialize = serial::from_value → normalize → validate  (reject invalid at load)
```

Bytes discipline, stated exactly: within `quillmark/document@0.NN.0` the envelope
is `serde_json` compact under frozen struct order with payload values insertion-
ordered; every `body` subtree is byte-identical to `content_key(&rt)`
(`richtext/serial.rs:349`), independent of `preserve_order`. A golden test asserts
`&envelope_bytes[body_range] == content_key(rt)`. The live-model invariant — every
`RichText` in a `Document` is normalized at construction — keeps `PartialEq` and
byte-equality aligned.

### Migration — a fallible cold-import hop inside core

The new version's read hop cold-imports the legacy body:
`TryFrom<CardV0_92_0> for CardV0_NN_0` runs `quillmark_richtext::import::from_markdown(&card.body)`
(reachable because richtext is the leaf `core` depends on).
Import is a pure function (`normalize → pulldown → corpus → normalize`), so the
migration is deterministic. The reader chain (`dto.rs:413`) gains a `?` per hop —
a one-line amendment to the DOCUMENT_STORAGE.md "Adding a Schema Version" playbook
step 5 (`From`, or `TryFrom` when a migration can reject).

- Rejected — **migrate above core in `quillmark`**: fails DOCUMENT_STORAGE Design
  Principle 4 (transparent serde API) — WASM `Document.fromJson` is
  `serde_json::from_str::<quillmark_core::Document>` (`bindings/wasm/src/engine.rs:522`),
  so old blobs fail there while new ones parse: a silent version trap. It also
  solves only half the problem — `Document::from_markdown` still needs import in
  core — and saves no bundle bytes (`quillmark` is in every binding build).
- Rejected — **an importer-hook trait registered into core**: serde `TryFrom` has
  no context parameter, so the hook is process-global mutable state
  (`OnceLock<Box<dyn RichTextImporter>>`); an unregistered importer turns "load an
  old row" into a runtime failure dependent on link order and feature flags.
- Rejected — **lazy migration / a `Markdown(String) | Rich(RichText)` body**:
  makes `PartialEq` and byte-determinism bimodal (one content, two unequal
  values); every consumer branches forever.
- Rejected — **store both string and corpus transitionally**: leaves the authority
  question open (which is truth after an edit?) and still needs the import
  somewhere.

Determinism boundary, corrected from INDEX.md: legacy bodies **can** hold tables
and images — today's `mark_to_typst` renders both — which import as islands with
**sequential** ids (`isl-N`). That is still a pure function, so the Spike-C
"migration introduces no mint nondeterminism" conclusion survives in substance;
INDEX.md's phrasing "legacy bodies hold no islands" is wrong and is corrected
(real per-creation minting is still Phase 4). `NestingTooDeep` (> `MAX_NESTING_DEPTH`,
now owned by `quillmark-richtext`, re-exported by `core::error`) →
`StorageError::Malformed`: such a document never rendered (`mark_to_typst` rejected
the same depth), so nothing renderable is lost.

`Document::from_markdown` imports per card after the unchanged `assemble::decompose`
(fence/YAML only); an empty body ⇒ `RichText::empty()`; body-disabled validation
(`validation.rs:239,270`) moves from `body().trim().is_empty()` to a new
`RichText::is_blank()`. Author-visible consequence, stated not hidden:
`.qmd → Document → .qmd` now **canonicalizes** body markdown (`__b__` re-emits as
`**b**`, fence-adjacent whitespace normalizes) — inherent to demoting markdown to
a projection; documented in markdown-spec and release notes.

### Crate re-homing — keep a leaf crate core depends on

`quillmark-richtext` stays a **separate crate** — `model`, `serial`, `import`,
`export`, `delta`, `usv`, and the relocated `normalize` — and `core` **depends on
it**. The freeze (canonical serialization, golden-bytes pinned) and the RichText
primitive sit one layer *below* the document engine. `quillmark-richtext` is
`publish = true` because the published `quillmark-core` publicly depends on it.
Final crate graph: `quillmark-richtext` (+`pulldown-cmark`) ← `core` ←
`quillmark`, `backends/typst` (−`pulldown-cmark`), `backends/pdfform`, bindings.

- Rejected — **dissolve richtext into `core::richtext`** (an earlier pick): loses
  the layering — RichText + delta are a rich-text primitive with no need for
  cards/quills/schemas/documents — and puts the *frozen wire contract* (a
  schema-version-bumping golden) inside the large engine crate, where its blast
  radius and test surface are no longer isolated. Dissolving buys one fewer crate;
  it does **not** buy a parser-free core (core needs `import` for migration +
  `from_markdown` regardless), so the only saving is a public-crate commitment —
  which, for a wire format, is a home worth having.
- Rejected — **move only model+serial to core, keep a codec crate above** (the
  phase-1 plan): both the `TryFrom` chain and `from_markdown` need import *inside
  or below* core; a codec crate *above* core is circular.
- The circularity that made a leaf crate look impossible ("richtext depends on
  `core::normalize`") **dissolves by relocating `normalize_markdown`** — a pure
  string primitive that belongs with the codecs, not the engine — into
  `quillmark-richtext` (**landed**; `MAX_NESTING_DEPTH` moved with it,
  re-exported by `core::error` for the backend). richtext no longer touches core;
  the arrow is now core → richtext.

`MarkdownFixer` unification (handover item 4) resolves by **deleting the backend
copy** with `mark_to_typst`, leaving one fixer at `quillmark-richtext`'s
`import.rs` — one parse site.

### Typst emit — a corpus walker that records the source map

Net-new `backends/typst/src/emit.rs` is the backend's private lowering (the
codegen tier of Option A — the one place a source map can be produced). It walks
the corpus to markup reusing `escape_markup`, recording per-segment generated
windows and one `(corpus range ↔ generated range)` pair per text run.

```rust
struct EmittedContent { markup: String, segments: Vec<SegmentMap> }
struct SegmentMap {
    corpus: Range<usize>,                               // USV
    gen:    Range<usize>,                               // bytes, relative to markup
    runs:   Vec<(Range<usize>, Range<usize>, EscapeCtx)>, // corpus ↔ gen, per text run
}
enum EscapeCtx { Markup, StringLit }
fn emit_richtext(rt: &RichText) -> Result<EmittedContent, EmitError>
```

- **Segments.** A segment is a maximal run of lines joined by `continues = true` —
  one paragraph, one heading, one whole code fence, one island line. This is what
  "paragraph-level" means against the corpus, and the unit a region keys on.
- **Walk.** Diff container paths between consecutive segments to open/close lists
  (`- ` / `+ `, explicit first number when `start ≠ 1`, nesting indent) and quotes;
  `Heading{level}` → `=`×level; `continues` para lines joined by `#linebreak()`;
  a code segment buffers its lines into one `#raw(block: true, lang:, "…")` whose
  per-line runs map under `EscapeCtx::StringLit`.
- **Marks.** Boundary sweep over normalized marks; open priority
  `(start, longer-first, kind-ord)`; close-and-reopen at overlap boundaries
  (Peritext free overlap → properly nested markup). `strong/emph/underline/strike`
  → `#strong[`/`#emph[`/`#underline[`/`#strike[`; `link{url}` → `#link("…")[`
  (`escape_string` on the url); `code` → `#raw("…")`. `anchor` / `unknown` emit
  nothing.
- **Islands are mandatory here, not Phase 4.** Migrated bodies carry tables and
  images; skipping them regresses rendering. `table` props → `#table(columns:,
  align:, table.header(…), …)` (today's grammar); `image` → `#image("url", alt:)`;
  unknown island types emit nothing (documented, parallel to the HTML rule).
- **The 2→4 coupling and recomputation.** Each run records only its `(corpus, gen)`
  pair; per-char spans are **recomputed** (Spike B: invertible, no stored tables)
  by a one-scan that treats `//`→`\/\/` as a 2-char/4-byte cluster and every other
  char as its own. A tripwire test ships with the emitter: scan-reconstructed
  bytes must equal `escape_markup(run)` / `escape_string(run)` byte-for-byte, so a
  future escape-rule change fails loud (the Spike-B run-alignment tripwire, now
  productionized).
- **Block quotes render.** `Container::Quote` → `#quote(block: true)[…]` — the
  handover's recommendation, landed as an explicit tested decision (a superset
  behavior change, not a silent consequence of the flattening arm disappearing).

Rejected — **keep `mark_to_typst` as a fallback** (recreates the dual-lowering
drift phase 1 flagged on the duplicated fixer) and **refactor to share a walker**
(one consumes a pulldown event stream, the other `lines`+`marks` ranges — a shared
abstraction would be fictional). `mark_to_typst`, the backend fixer, and the
backend's pulldown dependency are **deleted** after a parity suite (below) is
green, not kept.

Codegen nesting: `helper.rs` keeps emitting `#let _qm_cN = [\n{markup}\n]`
verbatim, now taking `EmittedContent` and rebasing segment/run offsets by the
block start (as it rebases the bracket window today, `helper.rs:83-88`);
`ContentWindow` becomes `ContentMap { path, block: Range, segments: Vec<SegmentMap> }`.
Canonical byte-identity (#801) holds by construction — the emitter is a pure
function of the corpus, dict keys stay sorted, and the reorder-only-apply identity
test carries over. The `__meta__` drift (`lib.rs:830` injects what
`PLATE_DATA.md:41` says is gone) is cleaned up on the seam-flip PR.

### Regions + navigation — segments, no revision yet (#829)

**Revision defers to Phase 3.** The Phase-2 region key is **`(field, corpus range)`**
— a USV `[start, end)` into the field's `RichText` in the session's current
compile. PREVIEW.md's no-counter argument still holds: `apply` is transactional,
the consumer single-owner and serial, so no cross-edit reader exists for a counter
to protect. A revision earns its keep in Phase 3 only alongside the change-log,
when a stale position can be *mapped* forward (`delta::map_pos`), not merely
detected. INDEX.md's phase-2 line ("re-key on `(field, corpus range, revision)`")
is corrected to defer `revision`; PREVIEW.md keeps its stance and gains a forward
pointer.

- Rejected — **a session-monotonic counter now**: fails PREVIEW.md's own test
  (nothing to protect) and adds surface Phase 3 redefines (its revision is
  per-field and log-anchored).
- Rejected — **content-hash-as-revision**: detects staleness, never maps a
  position.

**Two-tier windows, one unchanged run machine.** The emitter emits, per content
field, the block window it emits today **plus** its ordered segment windows. The
single-cursor run machine (`span_scan.rs:249-322`) runs **unmodified** over the
flattened key space `(window, Option<segment>)` — same states
(`NotSeen / Suspended{last_page} / Done`), same single cursor, same page-`+1`
continuation tolerance.

```rust
struct FieldWindow { path: String, file: FileId, range: Range<usize>, segments: Vec<Segment> }
struct Segment { corpus: Range<usize>, gen: Range<usize>, runs: Vec<RunPair> }

struct RenderedRegion {
    field: String,
    page: usize,
    rect: [f32; 4],
    span: Option<[usize; 2]>,   // USV [start,end) for content ink; None for scalars/widgets
    // Phase 3 appends `revision: Option<u64>` — additive, no break.
}
```

- Rejected — **per-line regions**: a 30-line fence yields 30 regions and a hard-
  break paragraph splits mid-block; #829 asks for paragraphs.
- Rejected — **a multi-cursor machine enumerating placements**: fails the
  placement-ambiguity theorem PREVIEW.md records — span data cannot tell package
  chrome inside one placement from a second placement, so enumeration reintroduces
  the lying-union the disjointness invariant exists to prevent.
- Rejected — **Typst-side `#metadata` paragraph tags**: a `show`-rule rebuild
  drops sibling markers (why spans were chosen), and injected content perturbs
  codegen and the #801 fingerprint.

`Classifier::classify` resolves `span → (file, byte range)` exactly as today, tests
block-window containment, then binary-searches `segments` by `gen` for the
innermost. A hit classifying to `(win, None)` — block ink between segments
(brackets, container-open syntax; rare, usually inkless) — is **transparent**:
provably the same field's ink, so it neither suspends the current segment run nor
accrues a box. Boxes come only from segment-classified ink, so no rect can lie.
Regions emit one entry per `(segment × page)` with `span: Some(corpus)`; widgets
and scalar sites stay `span: None`. **Field-level boxes are derived, not emitted** —
per `(field, page)`, the union of that page's segment rects, documented in
PREVIEW.md as the consumer formula. Visible change (the point of #829): a whole-
field highlight no longer covers inter-paragraph whitespace.

New navigation seam methods, default-`None` on `SessionHandle`, surfaced on
`LiveSession` and WASM:

- `position_at(page, x, y) -> Option<CorpusHit>` where `CorpusHit { field, pos }`:
  hit glyph → resolved node range **+ `glyph.span.1`** (the intra-node byte offset
  unused at `span_scan.rs:197` — the Spike-B carry) → generated byte → segment →
  invert that run's recomputed escape scan (cluster-exact floor).
- `locate(field, pos) -> Option<RenderedRegion>` (a caret rect): segment containing
  `pos` → forward-map to the generated byte → the frame glyph whose resolved range
  covers it → its box, page-indexed.

`page_hashes` stays span-excluding (#801): segments are scan-side metadata, not
frame content. `field_at` is unchanged (coarse, cheap).

### Schema surface — the `richtext` type

`FieldType::RichText { inline: bool }` replaces `Markdown`; `markdown` stays
accepted as a deprecated alias (load-time warning → `RichText { inline: false }`).
The transform-schema marker becomes
`{ "type": "object", "contentMediaType": "application/quillmark-richtext+json" }`;
the blueprint format slot stays **emission-only** (`type_expression` emits
`richtext<markdown>` / `richtext(inline)<markdown>`); `is_markdown_field`
(`typst/lib.rs:582`) → `is_richtext_field` on the new media type.

- Rejected — **a separate `constraint:` field on `FieldSchema`**: `richtext(inline)`
  is a type expression at every surface (Quill.yaml, blueprint slot), so splitting
  it across `type` + a sibling key makes two sources of truth for one token.
- Rejected — **validating the format slot on re-parse**: blueprint annotations are
  comments (`prescan.rs` treats them as decoration); Quill.yaml is the type
  authority.
- Rejected — **importing payload fields at document parse**: impossible — `Document`
  is schema-free (parses without its quill), so field-level typing cannot exist
  before `compile_data`.

`richtext(inline)` means, against the corpus: exactly one `Para` line, empty
`containers`, no islands (`continues` impossible); marks unrestricted. Enforced in
`validation.rs` as a type error (`richtext::not_inline`, the TypeMismatch fatality
class) and in coercion. Editors read it to mount single-line editors; the emitter
may lower an inline field without block wrapping (headers).

- **Coercion** (`config.rs`, replacing the shared String branch for this type):
  string → `import::from_markdown` → canonical corpus (error → `CoercionError` at
  the field path); object → `from_value` + normalize + validate (editors pass
  corpora through untouched); the length-1-array unwrap and bare-scalar-stringify
  leniencies are preserved (stringify then import).
- **Load-time import + cache.** `QuillConfig::from_yaml` imports every richtext
  `default` / `example` / `body.example` once into `#[serde(skip)]` companions
  (`default_corpus` / `example_corpus: Option<QuillValue>` on `FieldSchema` and
  `BodyCardSchema`) — the cache lives on the schema object, keyed structurally,
  computed eagerly: a pure function of Quill.yaml bytes, so determinism is
  inherited and no `OnceLock` enters a serde type. The render floor
  (`resolve_fields`) and **seeding commit the corpus form** — seeded documents are
  canonical from birth; `blueprint()` keeps reading the authored markdown (its
  output *is* the markdown surface). `$seed` overlays import at `seed_card` time (a
  user action, not a render loop).

Stated honestly: a richtext *field* authored as a string in a `.qmd` re-imports at
each `compile_data` (the same tier as date parsing — deterministic, so its regions'
corpus ranges are stable). This does **not** found #829 on a per-render parse —
`$body`, the #829 payload, is a typed corpus on `Card` and never re-parses. Full
`position_at` precision for string-authored fields reaches consumers that hold the
corpus; Phase-3 form editors write corpus-JSON and get it directly.

## Sub-PRs

The phase merges to `main` atomically off `integration/richtext`; intermediate
wire states below are branch-private.

1. **PR-A — leaf-crate arrow inversion (landed).** Relocate `normalize_markdown`
   + `MAX_NESTING_DEPTH` into `quillmark-richtext` (re-export the latter from
   `core::error`); `core` depends on `quillmark-richtext`; richtext drops its
   `core` dep and flips `publish = true`. Retire the "Quill-Delta" framing on the
   edit surface (doc-only). *Discharges handover 1 (revised); stages the
   seam/storage freeze.* Measured `pkg/core` cost of the eventual parser reach:
   ~75 KB gzipped (see risk 4).
2. **PR-B — live model `Card.body: RichText`.** `from_markdown` imports /
   `to_markdown` exports; `is_blank()`; wasm/python accessors (`body` → corpus,
   `bodyMarkdown` via export). Wire format held at V0_92_0 by writing through
   `export(body)` — a branch-private bridge, sound only because the phase merges
   atomically.
3. **PR-C — storage cutover V0_NN_0.** New DTO + `CanonicalRichText` + fallible-hop
   migration + goldens (a table-bearing legacy body; the `content_key`-equality
   assertion on the body subtree); DOCUMENT_STORAGE.md updates. *Closes the Spike-C
   storage gate.*
4. **PR-D — typst emitter (`emit.rs`) + segment maps.** `emit_richtext`, island
   lowering, block-quote render, escape tripwire, parity suite vs the still-present
   `mark_to_typst`. Engine-off (no production caller yet). *Discharges the emit half
   of handover 3; stages Spike-B's map.*
5. **PR-E — seam flip (Option A live).** `compile_data` / `to_plate_json` emit
   canonical corpus JSON for `$body` + richtext fields; typst consumes via
   `emit_richtext` (retiring `convert_content_value`); `ContentWindow → ContentMap`;
   pdfform `.text`-minus-slots lowering; schema rename + coercion + blueprint slot +
   alias warning; fixtures/goldens regen; **delete** `mark_to_typst` / fixer /
   pulldown from the backend; `__meta__` cleanup; PLATE_DATA.md + CONVERT.md.
   *Discharges handover 2 and 4.*
6. **PR-F — regions + navigation (#829).** Two-tier windows + segment run machine;
   `RenderedRegion.span`; `position_at` / `locate` with `glyph.span.1`; session/wasm
   surface; PREVIEW.md rework; INDEX.md revision-defer amendment. *Discharges
   handover 3 and 5; the Spike-B carry; delivers #829.*
7. **PR-G — `richtext(inline)` + load-time schema-value import + seed-commits-corpus.**
   Separable from E for reviewability.

Dependency order: **A → B → {C, D} → E → F → G** (C and D parallel; E needs B and
D, and follows C so the freeze is de-risked before the seam multiplies its
consumers).

## Sequencing invariant

Nothing embeds the canonical bytes before they are re-pinned in core: **A before C
and E** — one freeze (`serial.rs`), three consumers (storage, seam, `content_key`).
The storage DTO **freezes forever at C** and lands with its migration goldens and
the two-discipline bytes rule in one PR. `mark_to_typst` is not deleted before D's
parity suite is green. `RenderedRegion`'s wire shape does not freeze (F) before the
revision deferral is recorded in INDEX.md, and freezes additive-optional so Phase 3
extends it. The gate on E's deletion step is the flipped invariant itself: after E,
no render path in the workspace parses markdown.

## Risk register

1. **Parser drift vs migrated-blob byte-stability.** Cross-release byte-stability of
   *migrated* rows is now conditional on `pulldown-cmark` behavior. Pin exact, ship
   golden migration fixtures as a tripwire, state the conditional guarantee in
   DOCUMENT_STORAGE.md, recommend read-repair (rewrite rows post-migration).
   Residual: a forced security bump means a schema-version event or accepted hash
   movement on unmigrated rows.
2. **`glyph.span.1` beyond markup text** — raw string literals, enum-numbering ink,
   shaping/hyphenation clusters are asserted-plausible, not spike-proven. PR-F's
   first commit is a probe test; degrade path: cluster resolution falls back to
   node-start (segment-level correctness kept, char precision lost locally).
3. **Emitter/importer parity gaps** (`***` fixups, list starts, tight/loose lists,
   table-alignment corners). The parity suite is the gate; intentional diffs (block
   quotes render; import canonicalizations) are enumerated, everything else matches
   byte-for-byte.
4. **`pkg/core` WASM growth** from pulldown — **measured: ~75 KB gzipped** (the
   pulldown crate; the import codec adds ~9 KB more), ~+24% on the ~0.34 MB core
   bundle, well inside the 1.5 MB `CORE_MAX_GZIP_BYTES` guard. It lands only when a
   body/import path becomes reachable from `pkg/core` (the PR-B/E cutover), tree-
   shaken until then. Feature-gating import out of core builds is rejected
   (`fromMarkdown` is that build's purpose); accept or slim.
5. **`.qmd` body canonicalization** on round-trip — author-visible git churn on save;
   document in markdown-spec and release notes.
6. **String-authored richtext *fields*** keep a per-`compile_data` import and get
   only field-level nav precision until stored structurally; watch the
   `usaf_memo` `references: array<richtext>` field for cost and correctness.
7. **Segment-region UX shift** — striped whole-field highlights (union formula)
   replace one solid box; intentional per #829, but the consumer guidance lands with
   PR-F, not after.

## Canon + phase-1 rework this forces

- **ARCHITECTURE.md** — record the inverted invariant (markdown engine appears once,
  in `quillmark-richtext::import`), and the crate layering (`quillmark-richtext` is
  the leaf primitive `core` depends on).
- **Edit-language framing** — the "Quill-Delta semantics" label is retired
  (**landed**): the `Delta` is `retain`/`insert`/`delete` text splices (CodeMirror
  `ChangeSet`), not attributed Quill-Delta ops; marks/lines are separate op channels
  (phase 3), not op attributes. Carry the corrected framing into any phase-3 doc.
- **DOCUMENT_STORAGE.md** — two-discipline byte-stability paragraph; the fallible-hop
  playbook amendment; the migrated-row conditional-stability caveat + read-repair.
- **PREVIEW.md** — `regions()` rewritten (segment regions, `span` key, union
  formula, `locate` / `position_at`); no-counter stance kept with a Phase-3 pointer.
- **CONVERT.md** — rewritten as "RichText → Typst lowering" (the element table
  survives; the pulldown pipeline moves conceptually to `import.rs` / markdown-spec §6).
- **PLATE_DATA.md** — corpus JSON for content fields; `__meta__` drift removed.
- **SCHEMAS.md / BLUEPRINT.md** — `markdown` → `richtext`, the `richtext(inline)` row,
  the slot grammar, the seed-commits-corpus note.
- **INDEX.md** — revision-defer amendment; the "legacy bodies hold no islands"
  correction; phase-2 line updated to `(field, corpus range)`.
- **`core/Cargo.toml`** — **done**: `core` depends on `quillmark-richtext`; the
  dev-dep comment reflects that the production parser lives in that dependency.
- **Phase-1 handover** — item 1 is superseded (leaf crate, not dissolve-into-core;
  the arrow-inversion groundwork landed here); item 4 is discharged by PR-E.

## Related

- #831 (this rework), #829 (regions, delivered here), #830 (block-tree predecessor,
  superseded), #801 (span-excluding `page_hashes`, preserved)
- [INDEX.md](INDEX.md), [phase-0.md](phase-0.md), [phase-1.md](phase-1.md)
- `prose/canon/DOCUMENT_STORAGE.md`, `PREVIEW.md`, `CONVERT.md`, `PLATE_DATA.md`,
  `QUILL_VALUE.md`, `SCHEMAS.md`, `BLUEPRINT.md`
- `crates/core/src/document/dto.rs`, `crates/core/src/region.rs`,
  `crates/backends/typst/src/overlay/span_scan.rs`, `crates/backends/typst/src/helper.rs`
