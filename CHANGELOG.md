# Changelog

## Unreleased

- **breaking** wasm: fold `pushCard` into `insertCard(card, at?)` — one insertion verb per lane, absent `at` appends; `insertCard`'s parameters reorder to `(card, at?)`. Delete the deprecated `replaceBody` alias (use `revise({}, md)` or `writer.setBody`) (#961)
- feat(core,wasm): positioned card insert — `TypedWriter::add_card` / `writer.addCard` and the `addCard` ABI take an `at` position, so a positioned typed insert is one atomic call instead of `addCard` + `moveCard`; add `TypedWriter::remove_card` (mirrors JS `writer.removeCard`) and a JS `CardWriter.kind` getter (mirrors core `CardWriter::kind()`) (#961)
- **breaking** core: `Payload::insert` / `insert_fill` now validate the field-name and value-depth invariant at the boundary and return `Result<_, FieldViolation>`, closing the `payload_mut().insert(...)` hole that let a direct caller build an invalid document; pre-validated internal callers use the new `pub(crate)` `insert_unchecked` / `insert_fill_unchecked` (#958)
- feat(wasm,core): single-card reads — `doc.card(i)` (throws out of range), `doc.cardIndexById(id)` (first match; `$id` is non-unique), and `doc.seedOverlay(kind)`, backed by core `Document::card(i)` / `find_card(id)`. Reading one card, resolving a `$id`, or fetching a `$seed` overlay no longer serializes the whole `cards` array or main card (#956)
- **breaking** core: parse warnings live only on `ParseOutput` — the redundant `Document::warnings` field + `warnings()` getter are dropped and `Document::from_main_and_cards` no longer takes a `warnings` param (`Document` `PartialEq` is now a plain derive). `from_markdown` / `from_markdown_with_warnings` are unchanged (#959)
- feat(wasm,python): keyed card reads `getCardField(index, name)` / `getCardMarkdown(index, name?)` (py `get_card_field` / `get_card_markdown`) — the card-indexed twins of `get` / `getMarkdown`, mirroring the `commitCardField` / `setCardField` write verbs so card reads no longer require a `payloadItems` walk (#953)

## v0.94.0 - 2026-07-15

- Delete prose/plans directory
- feat(wasm): promote the live-session/canvas surface to stable
- docs(release): reconcile changelog and guides for 0.94.0 (skip 0.93)
- test(core,quillmark): remove redundant and low-value tests
- core: drop public→private rustdoc link in UiFieldSchema docs
- core/seed: collapse merge-then-sort into one declaration-order pass
- docs: declaration order is the field ordering contract; ui.order removed
- core, bindings: update tests, golden, and doc surfaces for structural order
- core: field declaration order becomes structural (IndexMap), ui.order retired
- Simplify group-order sort key and ui.order stamping
- Add card-level ui.groups registry for group identity and ordering
- pdfform: place widget geometry once at bind, not per render
- pdfform: gate form.json version before field-shape parse
- pdfform: derive widget intrinsics from the quill schema (form@0.2.0, #940)
- Reject nested ui.group; auto-order nested object properties
- docs: densify the plaintext/enum section of the 0.93->0.94 guide
- core, richtext: follow simplification cascades from plaintext
- docs: extend canon (BLUEPRINT, PLATE_DATA) for plaintext and enum
- docs: document plaintext and enum field types, narrow string
- quillmark: end-to-end render test for plaintext through typst
- core: test plaintext codec and enum promotion end-to-end
- core: add plaintext and enum first-class field types
- richtext: add from_plaintext literal codec and is_plain predicate
- fix(typst/overlay): stop underline/strike decoration ink truncating $body regions
- chore: migrate org references quillmark-org → borb-sh
- docs: fix cross-tree link that broke mkdocs --strict
- simplify: fix stale receiver refs and facade-prose misses after rename
- rename typed-write facade editor→writer; unify on quill.writer(doc)
- python: delete redundant commit_* verbs (subsumed by the editor)
- docs: two-tier binding surface — parity table + strata/ephemerality (#932)
- python: Document.editor(quill) front door + Editor/CardEditor + reads
- wasm: quill.editor(doc) front door + tier-1 gap verbs + reads
- wasm+python: delete addressed commit(addr) verb (subsumed by editor)
- core: TypedEditor set_body + add_card (tier-1 mirror verbs)
- simplify: follow the #925 cascades
- docs: finish migration index entry; tidy wasm doc link
- python: mirrored addressed write surface + corpus codec
- wasm: addressed write surface (install/revise/applyChange/commit) + codec
- richtext+core: pure Delta codec + install/revise body & field verbs
- test(wasm): update runtime.test.js editor sugar to the strict contract
- feat(core,wasm,python)!: typed field writes reject unknown names (#918)
- wasm: split Card read type from CardInput write type (#917)
- richtext: MarkOp::Remove subtracts the range instead of dropping the whole mark
- fix(quillmark-pdf): dict/array depth walkers skip comments and strings
- fix(richtext): markdown export never leaks a delimiter into the corpus
- fix(richtext): saturate ordered-list marker; doc/hint cleanups from final review
- #913: mirror the WASM typed batch on Python (commit_fields + set/commit reframe)
- #911: finish exposing the typed editor in WASM (commitFields batch + JS editor sugar + reframe)
- test(wasm): warm the lazy Typst backend load before timed Engine renders
- test: consolidate generator-arm round-trips and costume proptests (tier 3-4)
- test: remove orphan fixtures and redundant/low-value tests (tier 1-2)
- docs: audit of legacy/redundant/low-value tests, fixtures, logic
- richtext/core/bindings: address issues #902–#906
- richtext: escape image alt/URL and link URL on markdown export (#900)
- richtext: strip \r and bidi controls from apply_text_delta inserts
- Remove prose/proposals/: 893 proposal has landed
- Unify binding write-verb grammar: mechanical card-write twins (#895)
- Hard cutover to commit_field: remove legacy richtext writers
- wip: bindings + tests + docs for typed writes (pre-cutover)
- Typed field writes: conform dispatch, commit_field, TypedEditor (core)
- Review typed-field-writes proposal against codebase; resolve open questions
- Add proposal: typed field writes via schema-carried types (#893)
- feat(wasm): setCardBody — corpus writer for a non-main card body (#892)
- remove(core,richtext,wasm)!: delete the incremental-edit surface (#886)
- test(wasm): fix two applyFieldDelta tests left stale by the #881 un-gate
- feat(regions): field_boxes helper + CorpusHit granularity signal (#884)
- fix(core,wasm): resolve lint CI failure + re-sync applyFieldDelta type after merge
- wasm/canvas: report clamp on PaintResult; document paint compositing constraints
- fix(wasm): make applyFieldDelta and supportsCanvas types honest about their domain
- docs: migration guide for the richtext field write-surface unification (#881)
- richtext: un-gate applyFieldDelta to richtext fields (step 6)
- wasm: setRichtextField / updateCardRichtextField + per-field markdown getter (step 5)
- richtext: emit projects corpus fields to markdown; wire/DTO lossless (steps 2-4)
- richtext: Card::set_field_richtext writer + corpus read-back (step 1)
- fixtures: table_demo Quill + end-to-end table render test; document pdfform limit
- richtext: property-test structured tables through normalize + round-trip
- richtext: in-cell images degrade honestly instead of silent-Lossless
- richtext: canonical table shape — validate invariants, repair in normalize
- test: drop two redundant inline tests; dense-prose the runtime edit-surface note
- Prune landed simplification entries from the backlog
- Silence clippy nits in span_scan walk refactor
- test(wasm): keep runtime edit-surface guard focused on the #876 methods
- docs: fix rustdoc intra-doc links to satisfy the -Dwarnings lint gate
- Filter locate's glyph walk to the target segment
- Share frame-walk geometry between region scan and corpus walk
- feat(wasm): expose incremental editor edit surface through runtime (#876)
- Skip props re-clone in normalize when already key-sorted; document R2 hazard
- Carry the <u>/Strong distinction instead of re-sniffing source
- feat(typst): add plaintext(field) projection helper + document richtext migration hazard (#873)
- Lift island mark-dispatch out of model into serial
- Build canonical richtext tree once via move-based key sort
- feat(wasm): add setBody(corpus) mutator on Document (#874)
- Dedup emit_inline mark partition into wraps_and_codes
- fix(typst): lower richtext(inline) fields to inline content (#872); fix wasm TS types (#875)
- Remove unused ChangeLog/session forward surface
- Cut over-engineered simplification findings; refresh drifted citations
- docs(simplifications): remove the addressed findings
- docs(simplifications): record the sync-twins deferral rationale
- refactor(typst,richtext): dedup container dispatch and the #846 clip
- refactor(core): one shared decoder for the dual-shape richtext seam
- refactor(richtext): make apply_field_change all-or-nothing
- refactor(core): single carrier for the richtext inline flag
- refactor(richtext): drop unused usv UTF-16 helpers and content_key alias
- refactor: apply low-risk simplifications from prose/simplifications
- docs: record simplification-review findings in prose/simplifications/
- fix(richtext): apply a short field delta leniently (auto-append remainder)
- Trim unused test-only public methods from core
- docs(core,typst): dense-prose pass on comments
- docs(richtext): dense-prose pass on comments
- docs(core): update QuillValue fill-setter reference after set_fill removal
- Remove more dead/redundant logic
- docs: remove stale info across canon, docs, and comments
- test: remove redundant and low-value tests across the workspace
- Remove dead and redundant logic
- fix(docs): resolve broken intra-doc links (cargo doc -Dwarnings)
- typst/richtext: address #855 cleanup findings
- Fix #851 follow-up: transactional applyFieldDelta and fail-closed invalidation
- richtext: fix issue #854 cleanup findings
- fix(session): close the three delta-protocol lifecycle gaps (#851)
- Fix low-risk API-surface findings from #856
- fix(wasm): strip unreachable delta-API types from public runtime.d.ts (#850)
- chore: ignore dist/, .vite/, test-results/ to prevent build-artifact commits
- fix(richtext): cap single-line char diff to avoid quadratic blowup (#849)
- fix(richtext): balance markdown export for overlap, escape `&` and trailing heading `#` (#848)
- docs: fix richtext drift found in #853
- fix(richtext): keep islands in sync on text-channel edits (#847)
- fix(typst): balance markup for overlapping wrap+code marks (#846)
- Render thematic breaks (---/***/___) instead of dropping them
- fix(richtext): correct ChangeLog::map_pos staleness and future-revision handling
- chore(richtext): drop richtext-spikes from integration branch
- Move richtext inline shape to `inline: true` schema field.
- feat(richtext): PR-H findings — fixture, runtime nav, docs (PR-H)
- cargo fmt
- feat(richtext): preview revision stamp + WASM session delta API (PR-F/PR-G)
- feat(richtext): fallible document body mutators (PR-E)
- docs(richtext): drop .qmd and Quill-Delta from phase-3 plan
- docs(richtext): record PR-C/D landing in phase-3 plan
- feat(richtext): add mark/line op channels and apply bundle (PR-D)
- feat(richtext): add revision counter and bounded change log (PR-C)
- feat(richtext): replace coarse delta::diff with Myers/LCS (PR-B)
- Remove husky pre-commit hook
- cargo fmt
- docs(richtext): record phase-3 spike findings on integration branch
- docs(richtext): align canon + code comments to PR-G, wrap up phase 2
- fix(wasm): add span to canonical FieldRegion contract
- richtext PR-G: richtext(inline), load-time example cache, alias cutover
- Delete prose/review directory
- docs(richtext): trim phase plans to landed reality, fold spike findings
- test(richtext): wasm binding test for positionAt/locate round-trip (#829)
- PR-F: regions + navigation — two-tier segment scan, position_at/locate (#829)
- spike(richtext): PR-F de-risking — run-machine transparency + glyph.span.1 precision
- fix(richtext): accept bodyMarkdown in the wasm card field allowlist
- feat(richtext): phase-2 PR-E — bindings expose corpus body + bodyMarkdown
- docs(richtext): clarify hard vs soft break in the CONVERT lowering table
- docs(richtext): phase-2 PR-E canon rework for the corpus seam
- feat(richtext): phase-2 PR-E — pdfform lowers richtext to plaintext + fixture
- feat(richtext): phase-2 PR-E part 2 — delete the markdown oracle
- feat(richtext): phase-2 PR-E part 1 — seam flip + typst consumes corpus
- docs(richtext): reconcile Typst-emit section with structured cells
- docs(richtext): PR-E handover + phase-2 status refresh
- docs(richtext): remove the .qmd file-extension concept
- feat(richtext): structured table cells — markdown a pure projection
- feat(richtext): phase-2 PR-D — typst corpus emitter + segment source maps
- feat(richtext): phase-2 PR-C — storage cutover to quillmark/document@0.93.0
- fix(richtext): resolve broken RichText::validate intra-doc link
- fix(core): quote payload scalars that would re-parse as a non-string
- docs(richtext): PR-B landing log + PR-C handover on the phase-2 plan
- fix(richtext): clear rustdoc -Dwarnings lint on phase-2 PR-B docs
- test(bindings): update body expectations for corpus canonicalization
- feat(richtext): phase-2 PR-B — Card.body is RichText, markdown a projection
- docs(richtext): lock separate-crate topology + retire Quill-Delta framing in plan
- refactor(richtext): make quillmark-richtext a leaf crate core depends on
- docs(richtext): phase-2 plan — engine consumes RichText, delivers #829
- docs(richtext): note block-quote emit decision for phase-2 handover
- docs(richtext): phase-1 handover doc + integration HQ updates
- fix(richtext): address phase-1 review — freeze determinism, codecs, anchors
- feat(richtext): phase 1 — RichText corpus model, codecs, canonical serialization
- docs(richtext): land phase-0 spike findings on the integration HQ
- docs(richtext): make integration HQ canonical, self-contained
- docs(richtext): open integration HQ plan for the content-model rework
- Sync usaf_memo/0.2.0 fixture with airmark-quiver template
- #824: dedup bindings tests + add two coverage guards
- #823 items 2-9: dedup core + integration tests
- #821 follow-up: fix rustdoc private-intra-doc-link in winansi_encode
- #822: dedup typst backend + PDF crate tests (~280 lines)
- #825: cache only the wasm cargo build dir, always regenerate pkg/
- #806: resolve overlapping widget field_at by paint order, not name
- Dead-code sweep from minimize-0.92.1-main review (#817–#821, #823, #825)
- Ground #809/#810: memoize page checks, unify dict-splice + field-name filters (#816)
- Tidy deferred backlog: dedup error-summary, pdfform flatten-limit docs, cleanup (#804, #807, #810, #812) (#815)
- Fix silent-data-loss regression and blueprint/error/docs gaps (#803, #805, #808, #811) (#814)
- Land LiveSession.fieldAt delegation; harden page_hashes against source spans (#801) (#813)
- Helper codegen v2: markup blocks + generated data literal, retiring eval() and json() (#800)
- Disable clippy in lint CI job (#798)
- Satisfy clippy's manual_contains in the scalar-window walker
- Drop the #797 spike modules from the branch
- Harden the span scan against the review findings
- Delete .claude/skills/simplification-cascades directory
- Spike: generated data literal is Typst-equal to the json() blob
- Spike: content fields as markup blocks resolve with per-node spans
- Document the span-based region contract across canon, core, and the migration guide
- Expose fieldAt on the WASM session; align binding docs with the span contract
- Migrate usaf_memo off tagged(); rewrite region tests for the span contract
- Span-based region tracking: codegen'd eval windows + glyph-span scan
- Omit $body from the transform schema for body-disabled kinds
- feat(regions): any array is element-addressable; SchemaMeta drops its unreachable Option
- test(typst): own the widget-region test's schema instead of borrowing the fixture's
- refactor(typst): collapse SchemaMeta per-kind table boilerplate, test-owned widget width
- docs(migrations): document the compile-time region-binding validation in the working guide
- fix(ci): hash every pkg/ input in the wasm cache key, check key sets in the drift guard
- fix(wasm): sync canonical runtime.d.ts with the regions sidecar, wire typecheck into CI
- feat(regions): validate form-field paths, reject scalar index-tagging, cache schema meta
- docs(migrations): fix stale region-uniqueness line in 0.92→0.93 guide
- Reconcile canon and migration guide with the reworked error system
- Update binding surfaces to the current-compile warnings contract
- Thread Typst compile warnings through the session as current-compile state
- Collapse RenderError to a single diagnostics-carrying struct; drop Severity::Note
- Amend error-rework proposal per adversarial review
- Propose error system rework: one failure type, warnings on the compile seam
- dense-prose: drop issue reference and history narration from regions test
- feat(regions): tag the usaf_memo plate; canon + migration guide for the new region contract
- feat(regions): per-placement regions, public tagged() helper, one-shot sidecar
- docs: accuracy pass + dense-prose sweep across docs/, prose/canon/, comments, READMEs
- build-wasm.sh: dev builds stamp next-patch -dev.<sha>, releases stamp verbatim
- wasm runtime: snapshot caller handles before the first await
- ci: enable the Clippy lint gate
- Python: batched-error message embeds the first diagnostic
- docs(core): note apply's $quill-check boundary on LiveSession::apply (#778)
- perf(wasm): LiveSession retains QuillConfig, not the whole Quill (#778)
- docs: canonize LiveSession — live preview owns apply/ChangeSet (#778)
- feat(wasm): LiveSession.apply(doc) -> ChangeSet (#778)
- Canon: endorse programmatic document construction
- refactor!: collapse RenderSession into LiveSession (#778)
- Add blank-canvas Document constructor in core and all bindings
- feat: transactional apply() + ChangeSet on the render session (#778)
- Add scalar From impls for QuillValue; mutators take Into<QuillValue>
- Expose batched set_fields across Python, WASM, and .NET bindings
- feat(typst): persist the World in the session; evict comemo after each compile (#778)
- Add atomic batched Card::set_fields
- remove(dotnet)!: drop the .NET binding entirely
- Update README.md
- Update README.md
- Update README.md
- Update README.md
- Update README.md
- Update README.md
- Update README.md
- fix: update WASM region test for content auto-tag; fix doc links
- docs(region): tighten regions() doc per dense-prose
- feat(region): one region per logical schema field
- docs(region): fix grouping guidance and note blank/empty no-region case
- refactor(typst): expose a region only for schema-addressable fields
- feat(typst): auto-region tagging for content fields (#775)
- refactor(core)!: move field regions from RenderResult to RenderSession::regions()
- refactor(core)!: key regions on the quill schema field, not the backend widget
- docs: fold prune-evolutionary-info into dense-prose
- docs: canonize comment/doc style as the dense-prose skill
- refactor(test): drop dangling BACKLOG.md reference
- refactor(core): tighten doc-comments, prune evolutionary phrasing
- refactor(bindings): neutralize marketing and reframe legacy phrasing
- refactor(quillmark,cli,fuzz): tighten comments, drop bug-history narration
- refactor(typst): tighten module and item doc-comments
- docs(canon): prune evolutionary and marketing phrasing
- docs: neutralize marketing and reframe legacy aliases in consumer docs
- feat(pdfform): export PNG and SVG; remove the preview feature gate
- fix(ci): resolve lint and docs CI failures on pdfform branch
- fix(quillmark-pdf): make find_dict_value key/value-position aware
- Make the plate a Typst-backend concern, not a universal one
- Address branch-review findings: spine robustness, coercion parity, doc accuracy
- Prune resolved review items 01–06; trim 07 to remaining gaps
- Rework item 2: derive canvas capability, delete supports_canvas flag
- Address branch-review item 3: total FieldRegion round-trip
- Address branch-review items 1, 2, 4, 5, 6, 7
- docs(review): write up unaddressed branch-review items for discussion
- test(pdfform): restore flatten byte-level coverage at the unit level (preview-gated)
- refactor(pdfform): PDF output always AcroForm; remove the public flatten knob
- Update sample_form PDF title and field labels
- Make pdfform framing industry-neutral
- docs: add PDF Form backend page under quills section
- refactor(pdfform): remove the form.json -> Quill.yaml scaffold
- refactor(pdf): extract the incremental-update envelope into PdfUpdate
- pdfform: WinAnsi flatten encoding, value clipping, shared PDF writer
- review: resolve low-risk findings from pdfform branch review
- Update README.md
- Remove qualification layer and page composition: pdfform fills static forms only
- feat(quillmark-pdf): page-merge primitive + continuation foundation (#757)
- refactor(pdfform): centralize flatten value typography as one source of truth (#752)
- feat(wasm): ship pdfform canvas + make the per-backend canvas contract explicit (#755)
- feat(typst): generalize the plate API to arbitrary form-field(...) (#758)
- feat(pdfform): card-instance value addressing in the resolver (#757)
- feat(qualify): add quillmark-qualify — AcroForm PDF → form.pdf + form.json (#753)
- chore(fmt): apply rustfmt line-wrapping to pre-existing drift
- feat(pdfform,cli): scaffold Quill.yaml from a form.json field spec (#756)
- fix(pdfform): build the preview feature against hayro's vello_cpu re-export
- Expose form-field regions from stamped AcroForm backends (#759)
- Implement `pdfform` backend + shared `quillmark-pdf` stamping spine (V1) (#750)
- docs(proposal): design for pdfform backend + shared AcroForm spine (#744) (#748)
- fix(python): count container levels, not the scalar leaf, in depth bound (#742) (#746)
- Add generic hint for unresolvable Typst eval errors (issue #745) (#747)
- docs: fix prose/canon, docs, and code-comment consistency drift (#743)
- fix(core): count empty containers in json_depth_exceeds (#741)
- Update README.md
- docs: position Quillmark as a "schema-driven document engine" (#739) (#740)
- review(0.93.0): fix stale comments, document scalar coercion, cover card markers (#738)
- Unify blueprint emission on Document::to_markdown (#737)
- docs(blueprint): fix $-line comment inaccuracies in canon (#735)
- core: graciously coerce bare scalars into string fields (#733)
- docs(spike): propose unifying blueprint placeholder on the !must_fill tag (#732)


## v0.92.1 - 2026-06-22

- Accept uppercase field names; reserve only `$`-prefixed keys (#730)
- docs: canonize $ext.editor.title as per-card display name slot (#729)


## Unreleased

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

