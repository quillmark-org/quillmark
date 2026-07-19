# Changelog

## v0.95.0 - 2026-07-19

- release: re-cut to finish binding dists after the first publish run cancelled mid-flight
- **breaking** typst: a present `date` / `datetime` field lowers to a click-to-edit value-object — `(value: datetime(..), display: (..args) => text(value.display(..args)))` — instead of a bare Typst `datetime`, so the rendered glyphs are born at a generated `text(..)` node carrying a region keyed on the field's schema path: a date placed by a vendored package, or a card's date riding a shared loop variable, is click-to-edit. A blank date stays `none`, so `!= none` guards hold. Plates migrate two shapes: `(data.f.display)("…")` (paren form — the stored `display` is a closure on a dict, not a method) and `data.f.value` for anything native (comparison, `.year()`-family components, datetime-consuming packages). The tonguetoquill flagship quills' `display-date` dispatches on `type(date)`, since their `datetime.today()` blank-date fallback stays a native datetime (#990)
- **breaking** core,typst,pdfform: split the `datetime` field type into strict `date` and `datetime` (#717/#799 resolved) — `date` accepts a bare `YYYY-MM-DD` and rejects any time component; `datetime` accepts offset-less wall-clock `YYYY-MM-DDThh:mm[:ss]` (seconds zero-filled) and rejects timezone offsets, the space separator, fractional seconds, and bare dates. Offsets are rejected, never dropped (the engine does no zone math); storage stays verbatim; no truncation in either direction. A `date` lowers to the three-component Typst `datetime(year:, month:, day:)` (unchanged emission) and a `datetime` to the six-component constructor, carrying the wall-clock time. The transform schema marks `date` as `format: "date"` (keeping `format: "date-time"` for `datetime`), and the blueprint reads `date<YYYY-MM-DD>` / `datetime<YYYY-MM-DDThh:mm[:ss]>`. Most deployed `datetime` fields hold a bare date and migrate to `type: date` with byte-identical data; the fixtures (usaf_memo, cmu_letter) do so. No deprecation alias — `datetime` rejecting a bare date is the decided end-state (#991)
- **breaking** python: the binding commits to the typed lanes — field I/O flows through `quill.writer(doc)` / `quill.view(doc)` exclusively and `Document` is quill-free data and structure. Removed (WASM-only by scope, not lag — their audience is not a Python audience): the opaque field store (`store_field` / `store_fields` / `store_fill` and card twins), the content lane (`install` / `revise` / `apply_change`), the quill-free field reads (`get` / `get_card_field`), and the module codec fns (`import_markdown` / `export_markdown` / `rebase` / `map_pos`). The composable `$ext` / field-remove card twins fold onto a trailing `card=None` selector and `push_card` folds into `insert_card(card, at=None)`. Mirrored additions: `writer.revise_field` (the `Delta` receipt is not surfaced), `writer.add_card(kind, fields=None, body=None, at=None)`, `writer.card(i).kind`, and the `Writer` / `CardWriter` / `View` / `CardView` handle classes exported from the package (#970)
- **breaking** core,wasm,python: complete the `RichText` → `Content` residual sweep (#976 folds into #982) — retire the last informal-`corpus` / model-`RichText` *identifiers* the mechanical rename missed: `CorpusHit` → `ContentHit`, `EditError::CorpusApply` → `EditError::ContentApply`, `RichtextDecodeError::NotCorpus` → `NotContent` (the codec-specific `RichtextDecodeError` type itself is kept), storage DTO `CanonicalRichText` → `CanonicalContent` (serde is unchanged — no wire migration), and the model-generic Typst emitter `emit_richtext` / `emit_richtext_inline` → `emit_content` / `emit_content_inline` (they lower any `Content`, richtext *or* plaintext). Also fixes the `ParseError` display strings `richtext json …` → `content json …`, the doubled-word find-replace debris (`content content model` → `content model`), and stale prose/comments across canon and rustdoc. Schema/codec names are untouched — `richtext` / `plaintext` tokens, `FieldType::{RichText,PlainText}`, `field_richtext` / `FieldRichtext*` / `apply_field_richtext_change`, `richtext(inline)` — they name codecs, not the model. "corpus" is purged from the tree entirely, including the ordinary-English test-set names that meant a *collection* of fixtures (`fixture_corpus` → `fixtures`, `synthetic_corpus` → `synthetic_inputs`) (#982)
- **breaking** core,wasm,python: a schema-bound read view — `Quill::view(&doc)` / `quill.view(doc)`, the read twin of `quill.writer(doc)`. `view.get(addr)` interprets each field by its declared type (a `richtext` field → markdown, a `plaintext` field → its literal text via the plaintext codec, every other type → its canonical value verbatim), returns absent as `undefined` / `None`, and — the authority the quill-free `getMarkdown` lacks — throws `UnknownField` for a name the schema does not declare and `FieldRichtextDecode` for a content field holding an undecodable value. Core `TypedReader::get` returns a `ReadValue` (`Markdown`/`Plaintext`/`Value`); `view.card(i)` is the card cursor; core adds `Card::field_plaintext` (the `to_plaintext` twin of `field_markdown`). **`getMarkdown`'s field half retires**: `getMarkdown` / `get_markdown` / `get_card_markdown` are now body-only (WASM `getMarkdown` takes a `CardAddr`, a present `field` throws; Python drops the `name` parameter) — a field's markdown is read through `view.get`. The quill-free body projection stays on `Document` (#978)
- **breaking** content: one delta-application contract — implicit trailing retain is `try_apply`'s semantics (a short delta retains the untouched remainder; the error is over-consumption only), `apply` panics on an over-long delta instead of clamping (clamping is silent corruption), and `extend_to_base` is removed. `split_line` / `join_line` rebase marks through their one-char `\n` splice with `map_pos` — the same mapping the text-delta channel uses — so marks no longer drift across line ops and `apply_field_change` canonicalizes once (a single terminal normalize instead of one per stage); line sync rebuilds in one forward pass instead of per-`\n` `Vec` splices. Mark ops are specified in final-text coordinates (post-delta, post-line-op — the frame they validate against) (#926, #987)
- **breaking** core: storage blobs tagged `@0.81.0` / `@0.82.0` fail as an unknown schema version — the read-only `V0_81_0` / `V0_82_0` DTO trees and their forward migrations are retired (nothing persisted on this lineage predates `@0.92.0`; `0.82.0` was yanked). `V0_92_0` stays the oldest shape read, and its payload types back the current write path. DOCUMENT_STORAGE.md records variant retirement as the policy when no stored population remains (#929)
- **breaking** core,wasm,python: the markdown projection stops appending a trailing newline — `to_markdown` projects a *value*, not a file, so `field_markdown` / `body_markdown` (WASM `getMarkdown` / `exportMarkdown`, Python `export_markdown` / `get_markdown`) no longer grow a `\n`; `writer.set("subject", "Hello")` reads back as `"Hello"`, not `"Hello\n"`. `.qmd` files still end in one newline (owned by `Document::to_markdown`, the file writer) and the content fixed point is unchanged (import is newline-insensitive) (#965)
- **breaking** all: rename the content genus off its codec's name — crate `quillmark-richtext` → `quillmark-content`, type `RichText` → `Content` (and `RichTextLine`/`RichTextContainer`/`RichTextMark`/`RichTextIsland` → `ContentLine`/…), const `RICHTEXT_MEDIA_TYPE` → `CONTENT_MEDIA_TYPE` and its wire string `application/quillmark-richtext+json` → `application/quillmark-content+json`, `#[serde(skip)]` companion caches `FieldSchema::{default,example}_corpus` → `_content`, `SegmentMap.corpus: Range<usize>` → `.content`, Typst-emitter `EmittedContent` → `Emission` (it is markup + source map, not a Typst `content` value). Schema tokens `richtext` / `plaintext`, `FieldType::{RichText,PlainText}` variants, and the codec-specific `field_richtext` / `FieldRichtext*` / `apply_field_richtext_change` / `richtext(inline)` surface are unchanged — those name codecs, not the model. Canonical body JSON is nameless, so stored documents don't migrate; `contentMediaType` consumers pin to the new spelling. Retires the informal "corpus" noun to end the code/prose split (#976)
- **breaking** core,wasm,python: `getMarkdown` / `get_markdown` / `get_card_markdown` stop conflating an absent field with a present-but-not-richtext one — a present field that does not decode as richtext (a scalar/array/object a `storeField` wrote) now throws `FieldRichtextDecode` instead of reading back `undefined` / `""`; absence still returns the absent shape. Core `Card::field_markdown` becomes `Option<Result<String, RichtextDecodeError>>` (the projection twin of `field_richtext`). Rule: absence returns, mismatch raises; read the raw value with `get` (#968)
- feat(core,wasm): typed, anchor-preserving field revise — `TypedWriter::revise_field` / `CardWriter::revise_field` and `writer.reviseField` / `writer.card(i).reviseField` wrap core `Card::revise_field_checked` (diff-rebase surviving anchors, then schema-conform the result); the schema-bound verb lives on the writer, where the schema is (#957, #966)
- **breaking** wasm: the quill-taking `Document` methods become the hidden ABI under the writer — `commitField` / `commitFields` / `addCard` → `_commitField` / `_commitFields` / `_addCard`, dropped from the `.d.ts`; remove `doc.reviseChecked` (no runtime consumer — use `writer.reviseField`). The visible `Document` class then carries zero quill-taking methods (#966)
- **breaking** core: rename `EditError::BodyImport` → `EditError::Import` (message `body import failed:` → `markdown import failed:`) — the variant also fires on field-path imports (`revise_field`), where "body" misnamed it (#966)
- **breaking** wasm: fold `pushCard` into `insertCard(card, at?)` — one insertion verb per lane, absent `at` appends; `insertCard`'s parameters reorder to `(card, at?)`. Delete the deprecated `replaceBody` alias (use `revise({}, md)` or `writer.setBody`) (#961)
- feat(core,wasm): positioned card insert — `TypedWriter::add_card` / `writer.addCard` and the `addCard` ABI take an `at` position, so a positioned typed insert is one atomic call instead of `addCard` + `moveCard`; add `TypedWriter::remove_card` (mirrors JS `writer.removeCard`) and a JS `CardWriter.kind` getter (mirrors core `CardWriter::kind()`) (#961)
- **breaking** core: `Payload::insert` / `insert_fill` now validate the field-name and value-depth invariant at the boundary and return `Result<_, FieldViolation>`, closing the `payload_mut().insert(...)` hole that let a direct caller build an invalid document; pre-validated internal callers use the new `pub(crate)` `insert_unchecked` / `insert_fill_unchecked` (#958)
- feat(wasm,core): single-card reads — `doc.card(i)` (throws out of range), `doc.cardIndexById(id)` (first match; `$id` is non-unique), and `doc.seedOverlay(kind)`, backed by core `Document::card(i)` / `find_card(id)`. Reading one card, resolving a `$id`, or fetching a `$seed` overlay no longer serializes the whole `cards` array or main card (#956)
- **breaking** core: parse warnings live only on `ParseOutput` — the redundant `Document::warnings` field + `warnings()` getter are dropped and `Document::from_main_and_cards` no longer takes a `warnings` param (`Document` `PartialEq` is now a plain derive) (#959)
- **breaking** core: collapse the two parse functions into one entry — `Document::from_markdown` and `Document::from_markdown_with_warnings` are removed in favor of `Document::parse(md) -> Result<Parsed, ParseError>`, and `ParseOutput` is renamed `Parsed`. A document-only caller writes `parse(md)?.document`. Bindings are unaffected: WASM `Document.fromMarkdown` / Python `Document.from_markdown` keep their names and their `doc.warnings` getter (#964)
- feat(wasm,python): keyed card reads `getCardField(index, name)` / `getCardMarkdown(index, name?)` (py `get_card_field` / `get_card_markdown`) — the card-indexed twins of `get` / `getMarkdown`, mirroring the `commitCardField` / `setCardField` write verbs so card reads no longer require a `payloadItems` walk (#953)
- feat(content,wasm,python): `LineOp::SetContinues { line, continues }` — hard breaks lower op-wise. Split, join, and a text-delta `\n` all mint `continues: false` lines, so a within-block hard break (a paragraph hard break, a code fence's interior line) had no op and fell back to a whole-install, losing that edit's identity anchors. Threaded through the wire codec into WASM `applyChange` (TS union updated) and Python; `continues: true` on line 0 is rejected with `ApplyError::FirstLineContinues` before the write, leaving the content untouched (#949)
- feat(wasm): the runtime root re-exports the edit vocabulary its own signatures reference — `Content` / `ContentLine` / `ContentContainer` / `ContentMark` / `ContentIsland`, `Addr` / `Delta` / `Assoc` / `LineOp` / `MarkOp` / `ChangeBundle`, `CardInput` / `PathStep` — as type-only exports (single entry point preserved; no `/core` subpath), with a presence guard so a dropped re-export fails `npm run typecheck` (#948)

<!-- seed: commits since v0.94.0 — confirm the entries above cover them, then delete this comment
- chore: prune redundant logic and duplicate tests (post-0.94.0 residue scan) (#996)
- release: unbreak the crates.io publish lane; fold curated notes into the seed (#995)
- Emit date fields as click-to-edit value-objects (#990) (#994)
- Split `datetime` into strict `date` and `datetime` types (#991) (#993)
- Python binding: commit to the Tier-1 surface (#970) (#992)
- audit #982: complete the Content-genus residual sweep (retire "corpus") (#989)
- feat: schema-bound read view — `quill.view(doc)` and `TypedReader` (#988)
- Rebase marks through line ops; collapse bundle normalize (#987)
- core: retire the V0_81_0 and V0_82_0 storage read shims (#929) (#986)
- Document binding build performance guidance in CLAUDE.md (#984)
- richtext: to_markdown projects a value, not a file — no trailing newline (#965) (#977)
- docs(markdown-spec): scope $body wire claim, fix lossless→lossy projection (#983)
- Delete prose/review directory
- rename: content genus off its codec's name — RichText → Content, crate → quillmark-content (#976) (#981)
- Add note to not run cargo fmt (#980)
- fix(core,wasm,python): getMarkdown surfaces present-but-not-richtext instead of blanking (#968) (#979)
- docs: purge rogue .qmd file-extension mentions (#975)
- Rewrite CLAUDE.md for density (#974)
- docs: fix mkdocs strict build — drop cross-tree link to prose/canon
- wasm: name the main-card address (MAIN_CARD_ADDR), reject unknown addr keys (#969)
- core: collapse the two parse functions into one `Document::parse` -> `Parsed` (#964)
- core,wasm: writer-level reviseField; hide the quill-taking Document ABI (#966)
- core,wasm,docs: dense-prose pass over the #963 write surface
- docs: document the write-surface reshape (#955, #957, #960)
- python: rename opaque store verbs set_* → store_* (#960)
- wasm: unify Document on Addr addressing; store_* verbs; reviseChecked (#955, #960, #957)
- core: rename opaque store verbs set_* → store_*; add revise_field_checked (#960, #957)
- core: collapse the Payload insert helpers; tighten prose
- core: use plain code spans for pub(crate) refs in Payload::insert docs
- docs: 0.94→0.95 migration guide and BINDINGS parity refresh (#956, #958, #959, #961)
- wasm,core: writer/card-surface parity cleanups (#961)
- wasm,core: single-card, $id, and seed-overlay reads (#956)
- core: enforce field invariants at the Payload::insert boundary (#958)
- core: make ParseOutput the single owner of parse warnings (#959)
- docs(bindings): densify the card-read doc comments
- feat(wasm,python): keyed card reads mirroring the card write verbs (#953)
- docs: remove prose/simplifications backlog for greenfield re-analysis
- Add setContinues line op so hard breaks lower op-wise (#949)
- Re-export corpus edit vocabulary from @quillmark/wasm root (#948)
-->


## v0.94.0 - 2026-07-15

These notes cover everything since v0.92.1. No 0.93.x was separately
published — the 0.93 milestone folds into this release, so the upgrade path
from 0.92.1 is the `0.92-to-0.93` and `0.93-to-0.94` guides read in sequence.

- feat(wasm): the live-session / canvas-paint surface graduates from
  `@experimental` to stable — `Engine.open`, `LiveSession`, `apply` /
  `ChangeSet`, `paint` / `PaintOptions` / `PaintResult`, `PageSize`, and the
  `supportsCanvas` probe are now the committed preview API. The tag is dropped
  from the runtime `.d.ts` / `.js`, the wasm README, and `PREVIEW.md`; further
  shape changes follow the normal deprecation path rather than landing in any
  0.x. `Engine.render` / `supportedFormats` remain the one-shot path
- refactor(core)!: field ordering becomes fully structural — `ui.order` is
  removed and an authored `order:` is a load error. Field and card-kind display
  order is now the key order of the emitted schema (declaration order, backed by
  an `IndexMap`), and the auto-stamped `order:` integer disappears from
  `QuillConfig::schema()`; consumers walk the maps in key order instead of
  sorting on a stamped index. Typed-dictionary / typed-table-row properties
  render in declaration order, not alphabetically (#941). See
  `docs/migrations/0.93-to-0.94.md`
- feat(core)!: a card-level `ui.groups` registry gives groups identity and
  order. `ui.group` becomes a validated reference to a snake_case id
  (`quill::unknown_group` for a dangling ref); the registry's declaration order
  fixes group display order, labels derive from the id with a `title:` override,
  and a bare label-as-identity group is deprecated (`quill::implicit_group`). A
  nested `ui.group` is a load error (`quill::nested_group_not_supported`) (#941).
  See `docs/migrations/0.93-to-0.94.md`
- feat(core,typst,wasm,python)!: `plaintext` and a first-class `enum` join the
  schema. `plaintext` is navigable unformatted prose carried over the richtext
  corpus (a literal codec, with a `plaintext(field)` helper on the Typst side);
  `enum` is promoted to `type: enum` + `values:`, and the `enum:` modifier on
  `string` is deprecated for one release. `string` narrows to open scalar data
  (#938). See `docs/migrations/0.93-to-0.94.md`
- refactor(core)!: `type: richtext(inline)` retires — declare `type: richtext`
  with `inline: true`. The old token is a hard `quill::field_parse_error`, and
  `inline: true` on a non-richtext field is likewise rejected. Blueprint still
  emits `richtext(inline)<markdown>` and `build_transform_schema` gains
  `quillmark:inline: true`, both derived from the flag; documents and corpus
  wire shapes are unaffected. See `docs/migrations/0.93-to-0.94.md`
- refactor(pdfform)!: `form.json` slims to a binding layer (`form@0.2.0`). Bound
  `fields` drop `type` / `options` / `multiline` (derived from the schema
  field's kind, `enum` values, and `ui.multiline`); unbound widgets move to a
  `widgets` section; binding runs at load, so a bad `schema_field` fails with
  `pdfform::dangling_binding` / `pdfform::unbindable_field` instead of a silent
  blank. `form@0.1.0` is rejected and `$cards` absolute-index addressing is
  removed. Widget geometry is placed once at bind, not per render (#940). See
  `docs/migrations/0.93-to-0.94.md`
- refactor(core,richtext,wasm,python)!: the binding write surface settles into
  two tiers over a document-free corpus codec. `quill.writer(doc)` (wasm and
  Python alike) is the documented default — typed `set` / `set_all` / `setBody`
  / `addCard` / `card(i)` and quill-free `get` / `getMarkdown` reads — layered
  over the corpus lane (`importMarkdown` / `exportMarkdown` / `rebase` / `mapPos`
  plus the addressed `install` / `revise` / `applyChange` verbs) and the opaque
  `setField` primitive. The eager `bodyMarkdown` / `fieldMarkdown` projections
  and the per-address body writers retire pre-release; `replaceBody` /
  `replace_body` / `update_card_body` alias for one cycle; richtext fields gain
  the anchor-preserving `revise_field`; the addressed `commit(addr, …)` is
  deleted (subsumed by the writer). A core-vs-bindings parity table governs
  drift (#925, #932). See `docs/migrations/0.93-to-0.94.md`
- refactor(wasm)!: the `Card` shape splits by direction — a read `Card` always
  carries `body: RichText`, while `pushCard` / `insertCard` take a `CardInput`
  whose `body` still accepts a markdown string and whose non-`kind` fields are
  optional (#917). The card-write verbs become mechanical twins of their
  main-card names: `updateCardField` / `updateCardFields` rename to
  `setCardField` / `setCardFields` (#895). See `docs/migrations/0.93-to-0.94.md`
- fix(typst/overlay): underline / strike decoration ink no longer truncates
  `$body` field regions — the region geometry is taken before decoration strokes
  extend the glyph ink box, so a highlighted body field's box matches the text
  instead of the overrun (#937)
- chore: migrate org references `quillmark-org` → `borb-sh` across the tree
- fix(richtext): the markdown-export codec never leaks a delimiter into the
  corpus. An editor `apply_mark_ops` mark can wrap a span markdown can't
  represent (a `strong`/`emph`/`strike` edge on punctuation/symbols/whitespace,
  or abutting a literal `*`) — the run would re-import as literal `**`/`*`/`~~`
  text (bolding `a.` used to export `**a.**b`). `to_markdown` now verifies each
  rendered line by re-parse and drops any mark whose emission would alter the
  text, so the text always round-trips; only the unrepresentable formatting is
  lost. Import-domain corpora are unaffected (still an exact fixed point).
- feat(core,wasm,python)!: typed field writes via schema-carried types. One
  per-type write dispatch (`conform_value(value, schema, mode)`) unifies the
  render floor's coercion with a strict-write mode behind a `Leniency` flag; one
  typed writer per address, `Card::commit_field(name, value, &FieldSchema)`,
  dispatches on the schema — the write surface stays O(1) in field types. Adds
  `EditError::FieldConform` for non-richtext mismatches (richtext keeps
  `FieldRichtextDecode` / `FieldRichtextNotInline`). A schema-bound
  `TypedWriter` (`Quill::writer(&mut doc)`) is the front door: `set` / `set_all`
  resolve field types and strict-commit; a name the schema does not declare is a
  typo on the typed path, so it fails with `EditError::UnknownField` instead of
  falling to the opaque store (#918) — opaque storage stays available through the
  raw `set_field` / `setField` / `setCardField` verbs. Bindings gain
  `commitField` / `commitCardField` (wasm) and `commit_field` /
  `commit_card_field` (Python, net-new — Python had no richtext field writer).
  The pre-release richtext-specific writers are removed in the same cycle:
  `Card::set_field_richtext`, wasm `setRichtextField` / `updateCardRichtextField`
  — use the typed writer, which carries the `inline` constraint in the schema.
  Strict writes drop the render floor's cross-type `Boolean`↔`Number` coercions
  and fail a shape mismatch at the write, not at a later render (#893)
- remove(core,richtext,wasm)!: delete the incremental-edit surface — the
  per-field change log and everything layered on it: `richtext::ChangeLog` /
  `FieldChange` / `StaleRevision`; `LiveSession::revision` /
  `record_field_delta_at` / `record_field_change_at` / `ensure_base_revision` /
  `map_field_pos` / `apply_for_field_delta`; the WASM `applyFieldDelta` /
  `mapFieldPos` / `revision` and the `Delta` DTO; and the `revision` stamp on
  `RenderedRegion` / `CorpusHit` (and `FieldRegion` / `CorpusHit` on the wire).
  Anchoring a caret or selection across edits belongs to the editor's own
  transaction mapping (a ProseMirror / CodeMirror `StepMap`), not a parallel
  core-side position map: the bidirectional preview↔editor cursor bridge is
  `positionAt` / `locate` over the current compile, exact inverses that never
  consulted the change log. Whole-document `apply(doc)` stays the one edit verb.
  This dissolves #886's anchor-stranding half outright and drops the
  half-built delta path behind its per-keystroke-marshalling half; `Delta` /
  `diff` / `diff_import` / the mark & line op channels remain as the corpus
  writers' substrate (`replace_body`, `import_body_delta`, `apply_body_change`)
  (#886)
- feat(core,wasm): `field_boxes(field)` / `LiveSession.fieldBoxes(field)` derive
  the whole-field highlight — one union rect per page over the field's
  `span`-bearing content segments — so a "highlight the focused field" consumer
  stops reimplementing the span-filter + per-page union by hand. `regions()`
  stays the low-level disjoint truth (#829); the helper owns the union, and is
  content-only (a scalar-reference/widget-only field returns `[]`, its box being
  a single `regions()` rect). Core `field_boxes(&[RenderedRegion], field)` is a
  pure function so the one-shot `RenderResult.regions` sidecar gets it too (#884)
- feat(core,wasm): `CorpusHit.granularity` (`HitGranularity` = `cluster` |
  `segment`) reports whether `positionAt`'s `pos` resolved cluster-exact or
  floored to the containing segment's start (origin-less ink, a multi-line code
  fence's interior), so a caret UI trusts a `cluster` offset for the caret and
  treats a `segment` one as a segment selection instead of guessing. Additive-
  optional, omitted from the wire when the backend does not report it (#884)
- fix(wasm): `Engine.supportsCanvas` and `LiveSession.supportsCanvas` gain doc
  comments cross-referencing each other: the two are spelled identically but
  answer different questions (a pre-session backend estimate vs. this compile's
  authoritative answer, which can diverge — e.g. a 0-page document) — the
  divergence is now visible where each is used instead of only discoverable at
  runtime (#883)
- fix(core): drop two rustdoc intra-doc links from public items
  (`RichtextDecodeError`, `Card::set_field_richtext`) to the private
  `decode_richtext_value`, which `-D rustdoc::private-intra-doc-links` (part of
  the lint gate) rejects since the link can never resolve for a doc reader;
  reworded to a plain code span, matching the existing convention elsewhere in
  the same file for referencing a private helper from public docs
- fix(wasm): drop the `revision?` field from the public `CorpusHit`/`FieldRegion`
  types and the broken `{@link LiveSession.mapFieldPos}` / `.revision` references
  in `runtime.d.ts`. The delta API (`applyFieldDelta`/`revision`/`mapFieldPos`) is
  not forwarded through `runtime.js`, so no published consumer could reach the
  methods those fields pointed at, and the stamped `revision` was always `0` on
  the reachable read paths (whole-doc `apply` is revision-neutral). The public
  types no longer advertise a capability the shipped `LiveSession` doesn't expose
  (#850)
- refactor(core)!: `RenderSession` collapses into `LiveSession` — a persistent,
  incremental compiler that owns preview (#778). Reads (`render`, the canvas
  seam, `regions`) serve the session's current compile; the new transactional
  `apply(json_data)` recompiles in place (on `Err` every read keeps serving the
  last-good compile) and returns `ChangeSet { page_count, dirty_pages }` so a
  preview repaints `dirty ∩ visible`. Typst applies incrementally: the session
  persists its `QuillWorld` (fonts/packages/assets parsed once), swaps document
  data via `Source::replace`, and fingerprints visible page content for the
  dirty set; pdfform re-resolves + re-flattens (cheap by construction). New
  `RenderError::ApplyUnsupported` is the seam default. The callerless
  `typst_session_of` is removed. WASM: the `RenderSession` class is renamed
  `LiveSession` and gains `apply(doc): ChangeSet`; don't re-open per edit. The
  Typst backend now evicts `comemo`'s process-global cache after every compile,
  bounding memory over long editing sessions. See
  `docs/migrations/0.92-to-0.93.md`
- remove(dotnet)!: drop the .NET binding (`crates/bindings/dotnet`, the
  `quillmark-dotnet` crate, its `csharp/` managed layer, CI job, and NuGet
  publish workflow). Second-class and unmaintained relative to WASM/Python;
  removed rather than carried as bloat. Python and WASM are unaffected.
- refactor(core)!: field regions move from `RenderResult` to a session-level
  query, `RenderSession::regions()` (WASM `session.regions()`), and are keyed on
  the quill schema field path, not the backend widget. Only the interactive
  preview path wants region geometry; a one-shot byte render (PDF/PNG/SVG) does
  not, so `RenderResult.regions` is removed and the geometry is read once off the
  compiled session without a render. `RenderedRegion` (and the WASM
  `FieldRegion`) drop `name`/`kind`/`fieldType`/`value` for a single `field`
  carrying the schema address (e.g. `signature_block`); the pdfform AcroForm
  widget name no longer leaks. A region is emitted only for a schema-bound field
  — an unbound widget produces none. `RegionKind` is removed; the `quillmark-pdf`
  `FieldSpec` gains `schema_field` and `stamp`/`flatten` return plain bytes
  (`StampResult` is gone). Regions are geometry for overlays and canvas↔editor
  cross-navigation, never a compositing input (#773). See
  `docs/migrations/0.92-to-0.93.md`
- feat(pdfform)!: the `pdfform` backend now exports PNG and SVG as first-class
  `render()` output formats (`SUPPORTED_FORMATS == [Pdf, Svg, Png]`); PNG
  rasters at `RenderOptions::ppi` (default 144). The `preview` cargo feature is
  removed — the hayro raster/SVG/PNG seam is always linked, so SVG/PNG/canvas
  work out of the box rather than behind a flag. The `quillmark` crate's
  `pdfform-preview` feature is folded into `pdfform`; in the wasm crate both the
  `typst` and `pdfform` build variants link the `web-sys` canvas painter directly
- fix(quillmark-pdf): `find_dict_value` now walks the dict as strict
  key→value pairs, so a Name in *value* position (e.g. `/Subtype /Producer`)
  is no longer mis-matched as a key; the object/dict scanners also skip
  `%`-comments, so `endobj` or a key token inside a comment can't derail
  parsing of a base PDF. The `<<…>>`/`[…]` depth walkers (`extract_outer_dict`
  and `read_value_end`'s nested-dict/array branches) skip `%`-comments and
  literal `(…)` strings uniformly, so a `>>`/`]` carried inside a comment or
  string no longer truncates a dict/array and drops the keys after it
- feat(pdfform): add the Typst-free `pdfform` backend + shared `quillmark-pdf`
  AcroForm stamping spine; rewire Typst signatures onto the spine; thread a
  `regions` sidecar through `RenderResult` and generalize the raster-preview
  seam (#749, #750). See `prose/canon/ARCHITECTURE.md` and
  `docs/quills/pdfform-backend.md` for the shipped design.
- refactor(pdfform): PDF output is always an interactive AcroForm (Technique A).
  Value-flattening is internal machinery backing the SVG/PNG/canvas raster
  outputs, never a PDF deliverable. The public `RenderOptions.flatten` knob is
  removed across core and all four bindings (it was wired only in wasm, hardcoded
  `false` in Python, and ignored in .NET)
- fix(pdfform): the flatten path transcodes values to WinAnsi (with a
  `WinAnsiEncoding` font) so accented/Latin-1 text renders correctly in the
  raster output, and clips each value to its field box so long values can't
  overflow
- refactor(quillmark-pdf): hoist the shared PDF byte-serialization (object/text
  writers, `/Info /Producer` stamp) into `quillmark_pdf::writer`, consumed by
  both the stamp and flatten paths; `find_object_bytes` now matches any object
  generation and returns the live (last) revision
- docs(canon): canonize `$ext.editor.title` as the slot for a per-card display name
- refactor(core)!: remove the hand-set `Backend::supports_canvas()`; derive
  canvas capability from the one seam instead. `RenderSession::supports_canvas()`
  (authoritative, from `page_size_pt`) and `formats_support_canvas()`
  (pre-session hint, from output formats) replace it, so the capability can no
  longer disagree with what `paint` does. The engine and WASM `supportsCanvas`
  surfaces are unchanged in shape. See `docs/migrations/0.92-to-0.93.md`
- build(wasm)!: rename the WASM engine feature `render` → `typst` (now the
  default) and add a `pdfform` build variant, so a Typst-free
  PDF-form bundle can ship without Typst. From-source builders pass
  `--features typst` where they used `--features render`; the published JS API is
  unchanged. See `docs/migrations/0.92-to-0.93.md`

## v0.92.1 - 2026-06-22

- Accept uppercase field names; reserve only `$`-prefixed keys (#730)
- docs: canonize $ext.editor.title as per-card display name slot (#729)


## v0.92.0 - 2026-06-22

- 0.92 technical-debt sweep: correctness, $seed hardening, de-duplication (#727)
- dotnet: add $seed namespace writers (parity with Python/WASM)
- refactor(core): unify $ext/$seed into one out-of-band Meta concept
- Cleanup: simplify QuillWorld::font to a single expression
- Cleanup: de-narrate comments, sync canon binding tables with .NET
- dotnet: fix stale schema version (CI) + review-flagged polish
- docs(migration): cover the !fill → !must_fill rename in the 0.92 guide
- dotnet: fix native-lib copy to test project + two correctness bugs
- Remove Document.seed(kind) for strict ext/seed symmetry
- dotnet: expose $seed on the Card DTO
- refactor(dotnet): rename engine class Quillmark -> QuillmarkEngine
- Reject !fill: treat as a noncanonical tag, not a fill alias
- Fix binding build break + warn on unsupported fill positions
- fix(dotnet): resolve engine type in test namespace (CS0426)
- docs(dotnet): trim README to a dense, consumer-focused surface
- Address review nits: loud divergence, docs, coverage
- Add storage schema 0.92.0: persist nested !must_fill
- docs(canon): consolidate binding overviews into BINDINGS.md
- feat($seed): reject $seed on composable cards (root-only, like $quill)
- docs: move dotnet binding into canon, delete DESIGN.md
- docs: reframe QmBytes by-value return as a tested assumption, not a defect
- Carry nested !must_fill across the live wire (CardWire)
- Address review: trap FFI panics, fix depth-limit asymmetry, Equals contract
- Detect nested !must_fill on sequence-item inline first key
- docs($seed): correct two claims flagged in review
- Fix invalid '--' inside XML comment in Quillmark.csproj
- fix(docs): drop canon links that break mkdocs --strict
- Promote .NET binding: CI test job, NuGet release, first-class docs
- Spike: .NET binding symmetrical to the Python binding
- Capture and round-trip nested !must_fill markers
- Make QuillValue an annotated value tree (fill on nodes)
- fix(bindings): bump currentSchemaVersion to 0.92.0; add $seed JS test
- Rename !fill tag to !must_fill (accept !fill as deprecated alias)
- docs($seed): document the per-kind seed-overlay key across canon and spec
- test($seed): cover parse/emit/storage, overlay layering, advisory validation
- feat(core): first-class $seed key for per-card-kind seed overlays
- Remove RenderSession and canvas-preview APIs from Python binding (#722)


## v0.91.0 - 2026-06-17

- Upgrade Typst backend to 0.15 (#720)
- Security audit: resolve 10 findings, document 6 open issues (#719)
- Hygiene pass: simplifications, dead code removal, and docs cleanup (#718)


## v0.90.0 - 2026-06-10

- **Breaking (Rust API + bindings):** `Quill` is now engine-free, validated
  data. It no longer holds a backend; the `Quillmark` engine becomes a backend
  registry + render dispatcher. Rendering and capability move onto the engine:
  `render` / `open` / `supported_formats` / `supports_canvas` take `&quill`
  (JS: `engine.render(quill, doc)` etc.). The `engine.quill` / `quill_from_path`
  factory is removed — construct with `Quill::from_tree` (JS `Quill.fromTree`)
  or `quillmark::quill_from_path`. The backend-existence
  check moves from load time to render time (`UnsupportedBackend` now surfaces
  from the first engine call). `supportedFormats` leaves `Quill.metadata` (now
  pure config) for `engine.supportedFormats(quill)`. `Backend` gains a
  `supports_canvas()` capability method (default `false`; Typst `true`),
  retiring the `backend_id == "typst"` magic string. See
  [migration guide](docs/migrations/0.89-to-0.90.md).
- **Breaking (WASM/JS types):** `QuillMetadata` drops its `[key: string]: unknown`
  index signature. Code reading removed or unknown metadata properties (e.g.
  `quill.metadata.supportedFormats`) now fails at compile time with "Property
  does not exist" instead of silently returning `undefined` at runtime. Cast to
  `Record<string, unknown>` to reach extra `quill:` YAML keys if needed.
- **Breaking (Python API):** the Python binding adopts the engine-free shape.
  Render and capability move onto the `Quillmark` engine, taking a quill:
  `engine.render(quill, doc)` / `engine.open(quill, doc)` /
  `engine.supported_formats(quill)` / `engine.supports_canvas(quill)` (were
  `quill.render(doc)` etc.). `Quill.from_path(path)` replaces
  `Quillmark.quill_from_path(path)` — the engine is no longer a loader, and the
  loaded `Quill` is engine-free. `quill.metadata` no longer contains
  `supportedFormats` (read `engine.supported_formats(quill)`) and is now a pure,
  infallible config read. Backend resolution moves from load to render time:
  `UnsupportedBackend` surfaces from the first engine call, not from `from_path`.
  See the [migration guide](docs/migrations/0.89-to-0.90.md#python).
- **Breaking (Rust API):** `QuillSource` and the orchestration `Quill` collapse
  into one core type, `quillmark_core::Quill` (held by value; the vestigial
  `Arc` is dropped). `Backend::open` now takes `&Quill`; the consumer methods
  and the `seed` module move into core; `quill.source()` is gone
  (`quill.config()` is direct). Bindings already hid `QuillSource`, so JS/Python
  consumers are unaffected by the rename.
- **WASM packaging (single root export):** the root `@quillmark/wasm` import is
  now a hand-written **canonical layer** (`pkg/runtime/`) — it re-exports the
  Typst-less core's `Quill` / `Document` **verbatim** (same classes, no wrappers)
  and adds an async **`Engine`** (`render` / `open` / `supportedFormats` /
  `supportsCanvas`) as the canonical render API. The package `exports` map has
  exactly **one** public entry point, `.` (the canonical layer); the old
  `./render` and `./core` subpath exports are both **removed**. Engine-free
  editor/validation code (`Quill.fromTree`, `Document.fromMarkdown`) still loads
  only the small internal core binary (~0.66 MB gzip) — no backend is loaded
  until you render. The Typst backend binary is **private**
  (`pkg/backends/typst/`, not in the `exports` map): the `Engine`
  lazy-`import()`s it on first render, clones the quill/document into its memory as
  data (`Quill.toTree` → `fromTree`, `doc.toJson` → `fromJson`), and manages
  those clones internally (the validated quill clone is cached per instance;
  per-render document clones are freed) — consumers never import the backend or
  cross a WASM memory boundary themselves. `Quill.toTree()` is added to core for that crossing. A release-time
  size budget still guards the core artifact against Typst regressions.
- **WASM `Engine` (descriptor-only backend registry):** `new Engine({ backends })`
  takes backend entries in **descriptor form only** — `{ load, formats, canvas }`
  with `formats` and `canvas` **required**. The constructor validates each entry
  and throws (naming the backend id) at construction. The capability probes
  `supportedFormats` / `supportsCanvas` answer from this required manifest
  **unconditionally**, never loading a backend binary or cloning the quill. The
  bare-thunk loader form and its load+clone fallback path are removed.
- **WASM `Engine` (no invalidation API):** the unreleased
  `Engine.invalidate(quill)` / `invalidateAll()` methods are removed before
  release. The backend-clone cache is keyed on the canonical `Quill` instance in
  a `WeakMap`; a quill's contents never change after construction, so the only
  invalidation semantic is to drop/replace the instance (the clone is freed with
  it via the `WeakMap` + wasm-bindgen weak-refs). An explicit invalidation API
  will ship with its first real consumer. The load-bearing invariant — a
  canonical ref is immutable content within a runtime's lifespan — is now
  recorded in `prose/canon/VERSIONING.md` (Ref Immutability).
- **WASM `Engine` (session/canvas surface marked experimental):** `Engine.open`,
  `RenderSession`, `paint`, `PaintOptions`, `PaintResult`, `PageSize`, and the
  `supportsCanvas` probe are tagged `@experimental` in the shipped types and
  README: they ship ahead of their first production consumer (the designed
  canvas live-preview path) and may change shape in any 0.x release.
  `Engine.render` and `supportedFormats` are the stable surface.
- **WASM (typed error contract):** the root exports `QuillmarkError` — a
  structural interface (`Error & { diagnostics: Diagnostic[] }`) naming the
  shape every fallible method already throws — and an `isQuillmarkError(e)`
  guard to narrow caught `unknown`s. No runtime behavior change: the WASM
  layer still throws a plain `Error` with `diagnostics` attached (there is
  deliberately no error class — a structural check works across builds and
  WASM instances). Consumers can delete their hand-rolled
  `.diagnostics`-extraction casts.
- **Breaking (Rust API + bindings):** a document's `$quill` reference is now
  **enforced** against the loaded quill. Rendering with a quill whose *name*
  differs (`quill::name_mismatch`) or whose *version* falls outside the selector
  (`quill::version_mismatch`) is a hard error via the new
  `RenderError::QuillMismatch`, in both `render` and `dry_run`. Previously a name
  mismatch was only the `quill::ref_mismatch` warning and the version selector
  was unchecked. See [migration guide](docs/migrations/0.88-to-0.89.md).
- **Fix (WASM bindings):** `Document.makeCard`'s generated TypeScript now marks
  `fields` (and `body`) as optional (`fields?: Record<string, unknown>`,
  `body?: string`), matching the doc comment and runtime behavior. They were
  typed as required because `unchecked_param_type` drops the `?` marker; the
  bindings now use `unchecked_optional_param_type`. Callers can build a bare
  card with `Document.makeCard('kind')`.

## v0.89.1 - 2026-06-10

- chore(release): v0.89.1-rc.1 (#714)
- feat(wasm)!: 0.90 canonical API — engine-free Quill, single root export, typed errors; Python parity (#713)
- Proposal: WASM bindings split (core + render) via backend-decoupled Quill (#710)
- Add version selector matching and mismatch warnings (#708)
- docs: density-optimization pass on user-facing docs (#703)
- Remove role annotation from root block metadata header (#707)
- canon: audit and correct all prose/canon/ docs (#704)
- Fix makeCard fields/body typed as required in WASM .d.ts (#702)
- Update CLAUDE.md


## v0.89.1-rc.1 - 2026-06-10

- feat(wasm)!: 0.90 canonical API — engine-free Quill, single root export, typed errors; Python parity (#713)
- Proposal: WASM bindings split (core + render) via backend-decoupled Quill (#710)
- Add version selector matching and mismatch warnings (#708)
- docs: density-optimization pass on user-facing docs (#703)
- Remove role annotation from root block metadata header (#707)
- canon: audit and correct all prose/canon/ docs (#704)
- Fix makeCard fields/body typed as required in WASM .d.ts (#702)
- Update CLAUDE.md

## v0.88.0 - 2026-06-05

- **Breaking (bindings + Rust API):** a single canonical **`Card` wire shape** now
  flows in *both* directions. Core owns it as `quillmark_core::CardWire` (with
  `From<&Card>` / `TryFrom<CardWire>`); the WASM/Python bindings serialize and
  deserialize it instead of hand-rolling their own per-card translation. The
  flat `CardInput { kind, fields?, body? }` input type is **removed**:
  `Document.pushCard` / `insertCard` (`push_card` / `insert_card`) now accept the
  same `Card` shape they return (`{ kind, payloadItems, … }`), so a card from
  `cards` / `removeCard` / `quill.seedCard` feeds straight back in. Build a fresh
  card from a flat field map with the new **`Document.makeCard`** /
  `Document.make_card` helper. A stale `{ kind, fields }` object is now a loud
  error (`deny_unknown_fields`), not a silently-empty card. The seeded per-card
  getters `quill.seedMain` / `quill.seedCard` (`seed_main` / `seed_card`) are
  exposed on both bindings, mirroring the Rust `Quill::seed_main` / `seed_card`.
- **Breaking (Rust API):** `Document::push_card` now returns
  `Result<(), EditError>` and, with `insert_card`, validates that the card's
  `$kind` is a valid, non-reserved composable kind — the cards-list invariant is
  enforced at the edit op rather than incidentally at `Card::new`.
- **Breaking (bindings + Rust API):** the schema-aware **form view is removed**.
  `Quill::form` / `Quill::blank_main` / `Quill::blank_card` (and the
  `quill.form` / `blankMain` / `blankCard` bindings) are gone, along with the
  `Form` / `FormCard` / `FormFieldValue` / `FormFieldSource` types. Validation
  diagnostics now flow through `Quill::validate(&Document) -> Vec<Diagnostic>`
  (`quill.validate(doc)` in WASM/Python), which forwards the canonical
  `validation::*` diagnostics and keeps the non-fatal `validation::field_absent`
  completeness signal that `render` demotes. Field values/defaults/order are a
  `Document` × `quill.schema` join the consumer performs directly. See
  `docs/migrations/0.87-to-0.88.md`.
- **Breaking (diagnostics):** the validation code `validation::must_fill_absent`
  is renamed `validation::field_absent`. "Must-fill" is now scoped to the
  blueprint communication surface (the `<must-fill>` sentinel and the fatal
  `validation::must_fill_sentinel`); an *absent* field is a non-fatal
  completeness signal, not a fill requirement, since the render floor
  zero-fills it. The schema cell axis is renamed accordingly: the no-`default:`
  cell is **Unendorsed** (was "Must Fill"), the antonym of **Endorsed** —
  consumers routing on the old code or label must update. Internally
  `ValidationError::MustFillUnset { source }` splits into `FieldAbsent` and
  `MustFillSentinel` and the `MustFillSource` enum is removed.
- **Breaking (bindings + Rust API):** the `example` reference document is
  removed. `QuillConfig::example()` and the `Quill.example` (WASM) /
  `Quill.example` (Python) getters are gone. Its "show me a filled-out one"
  role is served by seeding — `Quill::seed_document()` / `Quill.seedDocument()`
  / `Quill.seed_document()` — which returns a committed `Document` rather than
  an annotated string. The CLI `render` with no input file now renders the
  seeded document. Nothing consumed the example document's annotations (the
  authoring surface is `blueprint()`), so the projection collapses into the
  seed: internally the `FillSource` fork in blueprint emission is gone and the
  blueprint always renders `default:` else the `<must-fill>` sentinel.
- **wasm:** lower the npm package `engines.node` floor from `>=24` to `>=22`.
  The runtime never required 24 — `--weak-refs` needs only Node 14.6+, and the
  `using` sugar that motivated the 24 floor is optional (a `try` / `finally`
  fallback covers Node 22). The aggressive floor hard-blocked installs on Node
  22 CI/dev images under `engine-strict`.
- **wasm:** `Document.makeCard(kind, fields?, body?)` now types `fields` as
  optional in the generated `.d.ts` (was required, contradicting its docs);
  omitting it yields an empty field map, as before.
- **docs:** fix the `Quill.schema` getter doc — the returned schema **includes**
  `ui` hints (it never stripped them); the stale "ui hints stripped" wording is
  corrected. The 0.87→0.88 migration guide now documents the `fill` flag's
  `!fill`-placeholder semantics and clarifies that seeding is example-filled,
  not a blank-form replacement.
- **blueprint:** flatten `group_fields` and drop the unused group label (#697).
- **docs:** document seeding (example → absent), fix a block-scalar prescan
  bug, and add commitment-ladder docs (#691).
- **docs(canon):** dedup field-resolution semantics into SCHEMAS (#692); note
  that released migration guides are era-accurate and immutable (#695); prune
  evolutionary information from comments and canon docs (#700).

## v0.87.3 - 2026-06-04

- Complete and consolidate the $ext mutator surface (#689)
- Complete the `$ext` mutator matrix with namespace-scoped removal and
  card-indexed namespace ops: `remove_ext_namespace` (Rust `Card`,
  `removeExtNamespace` WASM, `remove_ext_namespace` Python) plus
  `setCardExtNamespace` / `removeCardExtNamespace`. Deleting a sub-namespace
  is now the preferred way to clear `$ext` state — it preserves sibling
  consumers' slots and drops `$ext` entirely once empty, where `removeExt`
  remains a blunt clear-everything escape hatch.
- **Breaking (bindings):** the whole-map card mutator `updateCardExt` /
  `update_card_ext` is renamed `setCardExt` / `set_card_ext` for naming
  consistency with `setExt` on the main card.

## v0.87.2 - 2026-06-03

- Expose $ext write path through the editor surface and bindings (#687)
- Surface prose/canon entrypoint and fix canon documentation drift (#686)


## v0.87.1 - 2026-06-01

- Make $quill reference grammar a single source of truth (#684)
- Remove stale FieldType::Date references and add rejection test (#683)


## v0.87.0 - 2026-06-01

Arrays become first-class typed fields via a required `items` element
schema, datetime is unified under a single `type: datetime` accepting the
full YAML-1.1-style timestamp range (`FieldType::Date` is gone), and object
zero values are now shape-valid. This release tightens schema-load
validation in several places — empty `properties` maps and deeper array
nesting are now rejected — and consolidates the example/default conformance
checks behind one shared primitive. Documentation now ships from GitHub
Pages instead of Read the Docs.

### Breaking changes

These are schema-load cutovers for `Quill.yaml` authors; full before/after
steps are in `docs/migrations/0.86-to-0.87.md`.

- **Array fields now require an `items` element schema** (#672). Arrays
  previously carried a single untyped `Array` type; scalar arrays were
  never coerced or validated element-wise and were always annotated
  `array<string>`. Every array field must now declare `items`, and schema
  load rejects arrays without it. The bare-`properties`-on-an-array form
  (the old "typed table") is **removed** in favor of
  `items: { type: object, properties: … }`. Migration for a typed table:

  ```yaml
  # before
  rows:
    type: array
    properties: { name: { type: string }, qty: { type: integer } }
  # after
  rows:
    type: array
    items:
      type: object
      properties: { name: { type: string }, qty: { type: integer } }
  ```

  A scalar array adds `items` directly, e.g.
  `counts: { type: array, items: { type: integer } }`. Elements now coerce
  and validate against `items` (failing at the indexed path, e.g.
  `counts[1]`), and blueprint annotations reflect the element type
  (`array<integer>`, `array<markdown>`, …). Bundled quills and the
  `usaf_memo` golden schema are migrated.
- **`FieldType::Date` removed; use `type: datetime`** (#679). `type: date`
  no longer exists. `type: datetime` now accepts the full range from a bare
  `YYYY-MM-DD` date through RFC 3339 with offset (seconds optional, `T` or
  space separator). Datetime values gain calendar validation (e.g. Feb 30
  is now rejected), and JSON Schema output emits `format: date-time` for
  all datetime fields. The WASM `FieldType` union drops `"date"`. The
  blueprint hint is now `datetime<YYYY-MM-DD[Thh:mm:ss]>`.
- **Empty `properties: {}` on an object field is rejected** (#678). An
  empty properties map carries no information (the only conforming value is
  `{}`) and is almost always a mistake. It is now treated like a missing
  `properties` key and surfaces `quill::object_empty_properties`.
- **Deeper array nesting is rejected** (#673). The documented "one level of
  nesting" contract is now enforced in a single recursive pass, closing a
  gap where `array<object<array>>` and `object<array>` were silently
  accepted. A typed table row and a typed dictionary may carry scalar
  columns/properties only; deeper shapes fail with
  `quill::nested_array_not_supported`.

### Behavioral changes

- **Object zero values are now shape-valid** (#677). `zero_value` returned
  a bare `{}` for every object field, which failed validation on any object
  with `properties` (each absent property reported as `MustFillUnset`), so
  the zero-filled render path broke for object fields. An object with
  `properties` now recurses, zero-filling each property to its own
  type-empty leaf. `{}` remains the zero only for the property-less edge
  case.
- **`example:` values are now validated** (#680). The conformance check for
  `example`/`default` literals recurses into array items and object
  properties and validates datetime format — capabilities the old
  load-time path lacked, so previously-unvalidated `example:` values are
  now caught.

### Documentation & infrastructure

- **Docs hosting moved from Read the Docs to GitHub Pages** (#671). A new
  `docs.yml` workflow builds MkDocs (strict build as a PR check) and
  deploys to Pages on a published release; RCs are skipped. `.readthedocs.yaml`
  is removed and homepage/User Guide links point at the Pages URL.
- **Canon + docs: partial documents are first-class citizens** (#670). The
  docs and binding READMEs no longer claim Must Fill fields must be supplied
  before shipping. The only hard render gate is well-formedness (values
  coerce, no surviving `<must-fill>` sentinel); completeness is a hint
  surfaced by the form view. The `format-designer/` docs tree is renamed to
  `quills/`.
- A Migration section overview page was added and wired into the nav (#674).

### Internal

- Example/default validation is consolidated behind a single
  `validate_schema_literal` conformance core shared by `quillmark-core`
  config loading and the CLI `validate` command, with author-friendly
  diagnostics preserved (#680).
- Array and markdown handling collapse into recursive passes over the
  schema in both schema-shape validation and the Typst markdown transform
  (#673).
- Doc/comment fixes from the array-items review (#675).


## v0.86.0 - 2026-05-31

Documents now render even when incomplete, the canonical card-yaml fence
becomes a bare `~~~`, and the way placeholder/illustrative values are
produced is reworked. This release also fixes two markdown→Typst
conversion bugs and stamps a PDF `/Producer` field.

### Breaking changes

- **Bare `~~~` is now the canonical card-yaml fence** (was `~~~card-yaml`)
  (#662). Existing `~~~card-yaml` documents still parse, but `to_markdown`
  re-emits the bare `~~~` form, so a document's canonical bytes change on
  its first re-emit (relevant if you content-hash or byte-compare emitted
  markdown, or store blueprint goldens). A side effect: a column-zero
  `~~~` fence in a prose body is now read as a card-yaml block — use a
  backtick fence or a non-`card-yaml` info string (e.g. `~~~rust`) for a
  literal code block. Full details and corpus-migration steps:
  `docs/migrations/0.85-to-0.86.md`.
- **`fill_blueprint()` removed** from `quillmark_core` and `quillmark`,
  along with its re-exports (#657, #665). Callers no longer post-process a
  blueprint string: fillable/illustrative documents come from
  `QuillConfig::example()`, and the render path fills placeholders itself
  (see below).

### Behavioral changes

- **Incomplete documents render instead of erroring** (#665). An absent
  Must Fill field is no longer a render error. On the render path each
  schema field resolves to its authored value, else its `default:`, else a
  type-empty zero value — applied to the plate projection only, never
  persisted to the document. Only malformed input stays fatal: a surviving
  `<must-fill>` sentinel, or a value that won't coerce/validate.
  `quill.form(doc)` still reports completeness independently of the render
  gate.
- **`default` vs `example` clarified** (#665, #663, #658). `default` is the
  value most authors want and is interpolated when a field is omitted (an
  authored value always wins); `example` documents a field's shape only and
  never renders into output. Preview and illustrative fills now draw from a
  field's `example:` when present, falling back to the leanest type-valid
  value (`""`, `0`, `false`, `[]`, `{}`, first enum variant, empty body).

### Markdown → Typst fixes (#661)

- Code is now emitted as `#raw(...)` with a string literal instead of a
  backtick fence. This fixes fenced or inline code whose content contained
  a run of three-or-more backticks, which previously closed the block early
  and rendered as markup.
- Ordered-list start numbers are preserved — a list written `3.` / `4.` now
  renders starting at 3 instead of restarting at 1.

### New API

- `QuillConfig::example()`, plus `example` getters on the Python and WASM
  bindings (#665).
- `quillmark_core::zero_value` — the single source of truth for a field's
  type-minimal value, shared by blueprint emission and the render path
  (#665).
- `RenderOptions.producer` on the core, WASM, and Python render APIs (#656)
  — overrides the PDF `/Info` `/Producer` string, which now defaults to
  `Quillmark <version>` on every Typst-rendered PDF.

### Other fixes

- PDF rendering folds the `/Producer` stamp and the signature-field
  AcroForm injection into a single incremental-update pass, preserving
  Typst's `/Creator` (#656).
- `usaf_memo`: the signature widget is now overlaid at the 4.5in signature
  block (AFH 33-337) instead of the 1in left margin, and no longer consumes
  layout flow that could push the block out of position (#660); empty
  signature fields no longer carry the `APPEND_ONLY` flag (#654).

