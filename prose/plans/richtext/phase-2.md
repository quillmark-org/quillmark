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

**Status: in progress.** PR-A and PR-B are **landed** on `integration/richtext`
(PR #836); PR-C, PR-D, and an **Option A — structured table cells** step
(inserted between PR-D and PR-E, § Sub-PRs) are **landed as commits on this
branch** (`claude/richtext-phase-2-review-1hgtwk`), not yet merged to
integration. **PR-E is next**, and its
handover is fully specified — decisions locked, no re-derivation needed — in
[§ PR-E handover](#pr-e-handover--seam-flip-option-a-live). The decisions below
are settled; the decomposition (§ Sub-PRs) is the landing order, and
[§ PR-B landing log](#pr-b-landing-log--pr-c-handover) records what PR-B and
PR-C actually shipped and the one deviation PR-G still inherits.

## The pivot — one parse site, at ingest

Five of the seven decisions hinge on one move: **the markdown parse crosses from
render time to ingest time, and the corpus is the only in-memory content model.**
`crates/richtext` stays a **separate crate** — `quillmark-richtext`, the leaf
holding the model, canonical serialization, edit deltas, and the markdown codecs
— but the dependency arrow **inverts**: phase 1 had richtext depend on core; now
**core depends on richtext**. `import` (and `pulldown-cmark`) live in that leaf,
reachable from the two core entry points that need it — the storage migration and
`Document::from_markdown`, which every binding (including the no-feature `pkg/core`
WASM build) uses to turn a markdown document body into the live model. The `typst` backend
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
- Rejected — **a hand-mirrored `RichTextV0_93_0` DTO tree**: a second copy of a
  freeze the phase-1 golden-bytes test already pins, free to drift from it.

Shape (`document/dto.rs`):

```rust
struct CardV0_93_0 { payload: PayloadV0_93_0, body: CanonicalRichText }

// newtype whose serde IS the canonical serializer — no parallel struct tree
struct CanonicalRichText(RichText);
//   Serialize   = sorted_value(serial::to_value(&self.0))
//   Deserialize = serial::from_value → normalize → validate  (reject invalid at load)
```

Bytes discipline, stated exactly: within `quillmark/document@0.93.0` the envelope
is `serde_json` compact under frozen struct order with payload values insertion-
ordered; every `body` subtree is byte-identical to `content_key(&rt)`
(`richtext/serial.rs:349`), independent of `preserve_order`. A golden test asserts
`&envelope_bytes[body_range] == content_key(rt)`. The live-model invariant — every
`RichText` in a `Document` is normalized at construction — keeps `PartialEq` and
byte-equality aligned.

### Migration — a fallible cold-import hop inside core

The new version's read hop cold-imports the legacy body:
`TryFrom<CardV0_92_0> for CardV0_93_0` runs `quillmark_richtext::import::from_markdown(&card.body)`
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
markdown → `Document` → markdown now **canonicalizes** body markdown (`__b__` re-emits as
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
  align:, table.header(…), …)`; each cell is structured `{text, marks}` (Option A,
  landed), so its inline marks lower through the same mark sweep — `**bold**` in a
  cell renders `#strong[bold]`, not an escaped source slice. `image` →
  `#image("url", alt:)`; unknown island types emit nothing (documented, parallel
  to the HTML rule).
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

Stated honestly: a richtext *field* authored as a string in a markdown document re-imports at
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
2. **PR-B — live model `Card.body: RichText` (landed).** `from_markdown` imports /
   `to_markdown` exports; `is_blank()`; over-nested bodies → `ParseError::BodyImport`.
   Wire/seam/bindings held at markdown by writing through `export(body)` — a
   branch-private bridge, sound only because the phase merges atomically. **Two
   deviations from the plan as written**, both to keep each intermediate green and
   respect the dependency order — see [PR-B landing log](#pr-b-landing-log--pr-c-handover):
   (a) the binding `body → corpus` / `bodyMarkdown` split is **deferred to PR-E**
   (where corpus-JSON becomes the real seam); PR-B keeps `body` returning markdown
   so no binding test churns on a value the seam will re-shape anyway. (b) the
   infallible construction paths (seed, blueprint, `replace_body`) use an
   `import_body_lossy` bridge (over-nesting degrades to empty) pending PR-G's
   load-time example import.
3. **PR-C — storage cutover V0_93_0 (landed).** `quillmark/document@0.93.0`;
   `CanonicalRichText` newtype whose serde *is* the canonical serializer; fallible
   92→93 cold-import migration; goldens (body-subtree byte-identical to
   `content_key`; a table-bearing legacy body migrates deterministically to
   sequential `isl-N` ids); DOCUMENT_STORAGE.md updates. *Closed the Spike-C
   storage gate.* Starting state and steps, kept as the historical record, in the
   [PR-C handover](#pr-b-landing-log--pr-c-handover).
4. **PR-D — typst emitter (`emit.rs`) + segment maps (landed).** `emit_richtext`
   walks the corpus to Typst markup + per-segment source maps; island lowering,
   block-quote render (`#quote(block: true)`, the one intentional divergence),
   escape tripwire, parity suite vs the still-present `mark_to_typst`: **103
   inputs byte-identical** at landing. Engine-off (no production caller;
   `mark_to_typst` stays the oracle until PR-E deletes it). *Discharged the emit
   half of handover 3; staged Spike-B's map.*
5. **Option A — structured table cells (landed).** Table islands previously
   stored each cell as a raw markdown source slice — the last place markdown was
   load-bearing inside the corpus. Cells now carry inline structure `{text,
   marks}` (marks are USV offsets into the cell's own text, reusing the frozen
   `Mark` wire shape). Import parses cells once; export reconstructs the pipe
   table; the emitter renders `#strong[...]`/`#raw`/`#link` inside `#table(...)`.
   `normalize()` canonicalizes cell marks before key-sorting props; `validate()`
   bounds them by the cell's own text. **Amends the @0.93.0 freeze**
   (branch-private, pre-release — the golden was regenerated, not re-versioned)
   and **retires PR-D's two table-cell parity residuals**: formatted and
   escaped-pipe cells now byte-match `mark_to_typst`. Parity: **116 inputs
   byte-identical**; the only remaining diffs are the block-quote render
   (intentional) plus two inherent import canonicalizations (coincident
   `***`→`#strong[#emph[...]]`; empty-text link dropped) — both corpus-level, so
   no emitter and no future table type can differ from them. *Materially retires
   risk 3 (§ Risk register).*
6. **PR-E — seam flip (Option A live).** `compile_data` / `to_plate_json` emit
   canonical corpus JSON for `$body` + richtext fields; typst consumes via
   `emit_richtext` (retiring `convert_content_value`); `ContentWindow → ContentMap`;
   pdfform `.text`-minus-slots lowering; schema rename + coercion + blueprint slot +
   alias warning; fixtures/goldens regen; **delete** `mark_to_typst` / fixer /
   pulldown from the backend; `__meta__` cleanup; PLATE_DATA.md + CONVERT.md.
   *Discharges handover 2 and 4.* Self-contained, decisions-locked handover in
   [§ PR-E handover](#pr-e-handover--seam-flip-option-a-live) — implement from
   there, not from this summary line.
7. **PR-F — regions + navigation (#829).** Two-tier windows + segment run machine;
   `RenderedRegion.span`; `position_at` / `locate` with `glyph.span.1`; session/wasm
   surface; PREVIEW.md rework; INDEX.md revision-defer amendment. *Discharges
   handover 3 and 5; the Spike-B carry; delivers #829.*
8. **PR-G — `richtext(inline)` + load-time schema-value import + seed-commits-corpus.**
   Separable from E for reviewability.

Dependency order: **A → B → {C, D} → Option-A-cells → E → F → G** (C and D
parallel; Option-A-cells follows D, retiring its two table-cell parity residuals
before E multiplies the seam's consumers; E needs B, D, and Option-A-cells, and
follows C so the storage freeze was de-risked first).

## PR-B landing log & PR-C handover

**PR-B is landed** on `claude/phase-2-readiness-review-pbsdq9` (PR #836 → `integration/richtext`).
**PR-C has since landed too** (see the handover below, kept as the historical
record of its starting state and steps). This section is now historical —
the record of where PR-B's and PR-C's landed code diverged from the plan text
above — and the deviations later PRs still owe. The live, actionable handover
for the next PR is [§ PR-E handover](#pr-e-handover--seam-flip-option-a-live).

### What PR-B actually shipped

`Card` holds `body: RichText` (`document/mod.rs`). Reached only through:
- `Card::body() -> &RichText` (the corpus) and `Card::body_markdown() -> String`
  (the export projection). `RichText::is_blank()` is new in `quillmark-richtext`.
- **One markdown→corpus boundary**: `import_body` (empty ⇒ `RichText::empty()`, else
  `import::from_markdown`) and `import_body_lossy` (over-nesting → empty) in
  `document/mod.rs`, `pub(crate)`. Every string→body path routes through them:
  `assemble::decompose` (parse, fallible → `ParseError::BodyImport`, code
  `parse::body_import`), `wire.rs` `TryFrom<CardWire>` (→ `WireError::InvalidField`
  key `$body`), `dto.rs` `TryFrom<CardV0_92_0>` (→ `StorageError::Malformed`),
  `seed.rs`, `blueprint.rs`, and `edit.rs::replace_body` (the last three lossy).
- `normalize::normalize_document` **no longer touches the body** — a body is already
  a normalized corpus at import; it keeps only the NFC field-name pass.
- Validation keys on `body().is_blank()` (`quill/validation.rs:239,270`).
- `Document::to_markdown` re-emits `body_markdown()` and inserts one blank line
  after the closing fence (`emit.rs::append_body`).

**Branch-private bridges (still markdown strings, by design):** the plate-JSON
`$body` (`to_plate_json`), the `CardWire.body`, and the storage envelope
(`From<&Card> for CardV0_92_0` writes `body_markdown()`). The typst backend is
therefore unchanged in PR-B.

**Author-visible consequence (documented):** a document round-trip canonicalizes the
body — leading blank lines dropped, one trailing `\n`, `__b__`→`**b**`, and
inline-HTML-in-prose (`<<placeholder>>`, where `<placeholder>` reads as a CommonMark
tag) mangles per CommonMark. Rendered output is unchanged (the render path already
parsed markdown). ~50 test expectations were updated to the projection; the exact
chevron / code-context behavior is pinned in `assemble_tests.rs`.

**Verification at landing:** `quillmark-core` (604) + `quillmark-richtext` (110) +
full workspace + clippy + `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked`
green; Python bindings 113 passed (built locally); WASM job green on CI. A separate
commit on the branch fixes a **pre-existing** YAML scalar round-trip bug surfaced by
the `emit_roundtrip_fuzz` proptest (`String("_0")` emitted bare re-parsed as
`Number(0)`) — unrelated to the body change; the fix quotes payload scalars whose
plain emission would not round-trip.

### Deviations from the plan text — carry into later PRs

- **Bindings still expose markdown, not corpus JSON.** The plan's PR-B line said
  `body → corpus, bodyMarkdown via export`. Deferred to **PR-E**: the corpus-JSON
  seam is PR-E's contract, and flipping the binding accessor before then would churn
  JS/Python tests twice. **PR-E owes** this, and it is now fully specified — not
  restated here — under "Bindings" in
  [§ PR-E handover](#pr-e-handover--seam-flip-option-a-live)'s locked decisions,
  with concrete step 8.
- **`import_body_lossy` on seed/blueprint/`replace_body`.** Over-nesting degrades to
  the empty corpus (never reachable for real examples). **PR-G owes** the honest
  version: import + validate schema examples at `QuillConfig::from_yaml` and cache on
  the schema, so seed/blueprint read a pre-validated corpus and the lossy bridge is
  removed.
- **`blueprint()` canonicalizes example bodies** (it goes through `to_markdown` /
  `body_markdown`). The plan says "blueprint keeps reading the authored markdown"; in
  practice the example content is preserved but its whitespace canonicalizes. If PR-G
  needs byte-exact authored examples in the blueprint, special-case it there.

### PR-C — concrete handover (landed)

**Landed as specified below**: `quillmark/document@0.93.0` shipped with the
`CanonicalRichText` newtype, the fallible 92→93 migration, and the goldens in
step 4. Kept as the historical record of the starting state and steps — nothing
below is prescriptive anymore.

Goal: cut storage over to `quillmark/document@0.93.0`, embedding the **canonical
richtext corpus** in the envelope instead of a markdown string. The freeze
(`serial.rs`, golden-bytes) is already pinned in `quillmark-richtext`; PR-C is its
first storage consumer.

Starting state (all in `crates/core/src/document/dto.rs`): today the newest DTO is
`CardV0_92_0 { payload, body: String }`; `SCHEMA_V0_92_0 = "quillmark/document@0.92.0"`
(dto.rs:52); the `StoredDocument` enum + serde `rename`s (dto.rs:80,84,88); the
reader chain `TryFrom<StoredDocument> for Document` (dto.rs:419) dispatches per
version; `From<&Document> for StoredDocument` (dto.rs:298) writes the newest.
PR-B already made `TryFrom<CardV0_92_0> for Card` cold-import the body string
(dto.rs:479) and `From<&Card> for CardV0_92_0` export it (dto.rs:306) — those become
the **legacy** hop.

Steps:
1. Add `SCHEMA_V0_93_0 = "quillmark/document@0.93.0"`; add `StoredDocument::V0_93_0`
   (newest) with its serde `rename`; make new documents serialize as V0_93_0
   (dto.rs:293 currently asserts V0_92_0 — the write path moves to 93).
2. `struct CardV0_93_0 { payload: PayloadV0_93_0, body: CanonicalRichText }`. Reuse
   `PayloadV0_92_0`'s shape if unchanged (payload is not part of this freeze).
   `struct CanonicalRichText(RichText)` whose `Serialize` **is** the canonical
   serializer — `serial::to_value` then recursive key-sort (see `serial.rs`
   `content_key`/`to_canonical_json`/`sorted_value`, richtext crate) — and whose
   `Deserialize` = `serial::from_value` → `normalize()` → `validate()` (reject
   invalid at load). Do **not** hand-mirror a `RichTextV0_93_0` DTO tree; the newtype
   delegates to the one frozen serializer (rejected alternative in § Seam + storage).
3. Migration: `TryFrom<CardV0_92_0> for CardV0_93_0` runs `import_body(&card.body)`
   (fallible; `NestingTooDeep` → `StorageError::Malformed`). The reader chain gains a
   V0_93_0 arm (direct) and reworks the V0_92_0 arm to migrate 92→93→live. Legacy
   read paths (V0_81/82→92) stay; only the *newest* hop changes.
4. Envelope bytes discipline (two disciplines in one envelope): the envelope stays
   compact `serde_json` in frozen struct order with payload values in **insertion**
   order (`preserve_order`); every `body` subtree is byte-identical to
   `content_key(&rt)`. Golden test: `&envelope_bytes[body_range] == content_key(rt)`,
   plus a table-bearing legacy body migrating deterministically to sequential
   `isl-N` island ids (Spike-C is mint-free for text/marks; islands mint sequentially
   on import — a pure function).
5. `DOCUMENT_STORAGE.md`: add the two-discipline byte-stability paragraph; amend the
   "Adding a Schema Version" playbook step 5 for the `TryFrom`-when-a-migration-can-
   reject case; record the migrated-row conditional-stability caveat (byte-stability
   of migrated rows is now conditional on `pulldown-cmark`; recommend read-repair).

Verify: `cargo test -p quillmark-core`, the new goldens, `cargo doc -Dwarnings`,
clippy. PR-C is **independent of PR-D** (typst emitter) — both branch off PR-B.

## PR-E handover — seam flip (Option A live)

This is the grounding for whoever implements **PR-E** — self-contained; the
decisions below are locked, not proposals to re-evaluate.

**Prereqs landed:** PR-C, PR-D, Option-A structured cells, `.qmd` removal — all
on this branch.

**Goal:** flip the render seam from markdown-string to canonical corpus JSON
(Option A); typst backend consumes `emit_richtext`; delete `mark_to_typst` /
fixer / `pulldown-cmark` from the backend; schema rename; wire bindings;
regenerate goldens.

### Locked decisions — do not re-litigate

- **Seam = Option A (JSON).** `Backend::open(source, json_data)` stays a JSON
  data contract; the typst backend deserializes the `$body`/richtext-field
  corpus JSON → `RichText` → `emit_richtext`. Do **not** reshape the `Backend`
  trait to a typed seam (Option C) — C is a deferred, API-stable later
  refactor, explicitly out of scope for E. Rationale: the JSON payload doubles
  as the published plate contract (`to_plate_json`, PLATE_DATA.md); E is
  already the widest PR; the Rust-internal `RichText → JSON → RichText`
  round-trip is cheap next to a Typst compile.
- **Bindings.** `card.body` returns canonical corpus JSON (source-of-truth
  model); add `card.bodyMarkdown` = the export projection. `CardWire.body`
  flips to corpus JSON. Two ingest paths: editor/structured writes corpus JSON
  via `CardWire`; LLM/markdown writes via `from_markdown` (imports). Breaking
  for JS/Python consumers reading `body` as a string — update the binding
  tests.
- **pdfform.** Lower via `RichText.text` minus island slots → **plaintext
  only**. `/RV` rich-text AcroForm fields are **deferred indefinitely**
  (Adobe-only; other viewers ignore `/RV`). Future direction (noted, not
  built): a `ui` attribute on richtext fields that disables rich formatting
  while still riding the content model.
- **Atomicity relaxed.** Work lands on `integration/richtext`, which merges to
  `main` atomically as a whole — so E need not keep every intermediate commit
  green-to-main. Internal two-commit sequencing (flip+consume, then delete) is
  a convenience, not a requirement.

### Concrete steps (with anchors)

1. **Core seam flip.** `to_plate_json` (`crates/core/src/document/mod.rs`,
   ~lines 347-374): emit `$body` and each card `$body` as canonical corpus JSON
   via the richtext serializer (`content_key`/`to_canonical_value`), not
   `body_markdown()`. Same for richtext payload fields (coerced to corpus).
   Remove the branch-private markdown-bridge comment there.
2. **Typst consumes the corpus.** Retire `convert_content_value`
   (`crates/backends/typst/src/lib.rs:659,813`); `is_markdown_field` →
   `is_richtext_field` on media type `application/quillmark-richtext+json`
   (`lib.rs:582`). Deserialize each content field's corpus JSON → `RichText` →
   `emit_richtext`. Relocate `convert::emit` to a crate-root `mod emit` in
   `lib.rs` (PR-D declared it via `#[path]` to avoid touching lib.rs).
3. **ContentWindow → ContentMap.** `crates/backends/typst/src/helper.rs`:
   `generate_lib_typ` takes the emitted `{markup, segments}`, splices markup
   into `#let _qm_cN = [ … ]`, and rebases segment/run offsets by the block
   start (as it rebases the bracket window at `helper.rs:83-88`).
   `ContentWindow { path, range }` → `ContentMap { path, block: Range, segments:
   Vec<SegmentMap> }`.
4. **Segment-map offset test (MANDATED, do not defer to PR-F).** For a known
   corpus, assert each `segment.gen` range slices the expected substring of the
   generated `lib.typ`, and each run inverts to the right corpus range.
5. **pdfform lowering.** `RichText.text` minus island slots → plaintext field
   value. No fixture exercises it (`sample_form` binds no content field);
   recommend adding one synthetic pdfform quill that binds a richtext field to
   exercise the path (else it ships untested — note which).
6. **Schema surface.** `FieldType::RichText { inline }` replaces `Markdown`;
   keep `markdown` as a deprecated alias (load-time warning → `RichText{inline:
   false}`). Transform-schema marker → `{type:object,
   contentMediaType:"application/quillmark-richtext+json"}`. Blueprint format
   slot emits `richtext<markdown>`. Coercion (`config.rs`): string →
   `import::from_markdown` → corpus (error → `CoercionError` at field path);
   object → `from_value`+normalize+validate. (`richtext(inline)` enforcement is
   PR-G, not E.)
7. **Delete the oracle** (parity is green — 116/116 + block-quote + 2
   canonicalizations): remove `mark_to_typst`, the backend `MarkdownFixer`
   copy, and the backend `pulldown-cmark` dep. Gate: after E, no render path in
   the workspace parses markdown.
8. **Bindings:** `card.body` → corpus JSON, add `bodyMarkdown`; `CardWire.body`
   → corpus JSON; update wasm+python getters and their tests.
9. **Cleanup + goldens.** Remove the `__meta__` drift (`lib.rs`, ~line 830, vs
   PLATE_DATA.md). Regenerate fixtures/goldens under this **audit discipline:**
   the ONLY legitimate rendered-output changes are (a) block-quote fixtures now
   rendering `#quote`, (b) the two import canonicalizations. Any other golden
   delta is a regression. Table-bearing fixtures' storage/seam goldens carry
   structured cells; their rendered output is unchanged (parity holds).
10. **Docs:** rewrite CONVERT.md ("RichText → Typst lowering"), PLATE_DATA.md
    (corpus JSON for content fields; `__meta__` removed), SCHEMAS.md /
    BLUEPRINT.md (markdown → richtext, the alias).

### Gates

Parity green (done); the segment-map offset test; the golden audit; the
"no render path parses markdown" invariant after deletion.

### Out of scope (later)

Revision + regions/nav/#829 (PR-F); `richtext(inline)` enforcement + load-time
example import + seed-commits-corpus (PR-G); typed seam Option C (deferred
refactor); pdfform `/RV` (deferred indefinitely).

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
   table-alignment corners) — **materially retired.** PR-D's parity suite closed
   most of this; the two remaining residuals (formatted and escaped-pipe table
   cells) closed when Option A gave cells structure instead of a raw markdown
   slice. Parity now stands at **116/116 inputs byte-identical**, with two
   enumerated, corpus-level intentional diffs (block quotes render as
   `#quote(block: true)`; two inherent import canonicalizations — coincident
   `***`, empty-text link) that no future emitter or table type can differ from.
   Residual: none tracked; re-open only if a new island or mark type reopens a
   parity gap.
4. **`pkg/core` WASM growth** from pulldown — **measured: ~75 KB gzipped** (the
   pulldown crate; the import codec adds ~9 KB more), ~+24% on the ~0.34 MB core
   bundle, well inside the 1.5 MB `CORE_MAX_GZIP_BYTES` guard. It lands only when a
   body/import path becomes reachable from `pkg/core` (the PR-B/E cutover), tree-
   shaken until then. Feature-gating import out of core builds is rejected
   (`fromMarkdown` is that build's purpose); accept or slim.
5. **Document-body canonicalization** on round-trip — author-visible git churn on save;
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
