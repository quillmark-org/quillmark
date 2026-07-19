# Migration Guides

Quillmark evolves through deliberate, documented releases. When a release
changes the document syntax, the plate-JSON wire format, or a public API in a
way that is not backward compatible, it ships with a migration guide describing
exactly what changed and how to update your documents, quills, and host code.

Many of these are **hard cutovers** — the old form stops parsing or compiling,
so a guide is the path forward, not an optional read. Each guide is scoped to a
single version step; to cross several versions, work through the relevant
guides in order.

!!! note "0.93 was never separately published"

    The 0.93 milestone folded into 0.94.0 — no 0.93.x release was tagged.
    Upgrading from 0.92.1 means following the **0.92 → 0.93** and
    **0.93 → 0.94** guides in sequence.

## Available guides

- [0.94 → 0.95](0.94-to-0.95.md) — WASM `pushCard` folds into
  `insertCard(card, at?)` (one insertion verb; `insertCard`'s args reorder to
  `(card, at?)`) and the deprecated `replaceBody` alias is deleted (use
  `revise({}, md)` / `writer.setBody`); the typed `addCard` / `add_card` gains a
  position, `TypedWriter::remove_card` and a JS `CardWriter.kind` getter close
  writer/cursor asymmetries (#961). Core `Payload::insert` / `insert_fill`
  validate the field-name/depth invariant at the boundary and return
  `Result<_, FieldViolation>`, closing the `payload_mut().insert(...)` hole (a
  `pub(crate)` `insert_unchecked` serves pre-validated callers; the `Card`/writer
  mutators are unchanged) (#958). Parse warnings move fully onto `ParseOutput`:
  `Document::warnings()` and the `from_main_and_cards` `warnings` param are
  removed and `Document` `PartialEq` becomes a plain derive (#959); the two
  parse functions then collapse into one entry `Document::parse(md) -> Parsed`
  (`from_markdown` / `from_markdown_with_warnings` removed, `ParseOutput`
  renamed `Parsed`; a document-only caller writes `parse(md)?.document`) (#964).
  Additive, no action:
  single-card reads `card(i)` / `cardIndexById(id)` / `seedOverlay(kind)` backed
  by core `Document::card(i)` / `find_card(id)` (#956). The typed writer becomes
  the one schema-bound door: `writer.reviseField` (typed *and* anchor-preserving)
  lands, the quill-taking `commit*` ABI is underscored and hidden from the
  `.d.ts`, and `EditError::BodyImport` renames to `Import` (#957, #966).
  `getMarkdown` / `get_markdown` stop conflating absent with
  present-but-not-richtext: a present field that does not decode now throws
  `FieldRichtextDecode` instead of reading back blank — absence returns, mismatch
  raises (#968). The `datetime` field type splits into strict `date` and
  `datetime`: `date` accepts a bare `YYYY-MM-DD` and rejects any time component,
  `datetime` accepts offset-less wall-clock `YYYY-MM-DDThh:mm[:ss]` and rejects
  offsets/space/fractional/bare-date (offsets are rejected, never dropped). Most
  `datetime` fields hold a bare date and rename to `type: date` with no data
  change (#991).
- [0.93 → 0.94](0.93-to-0.94.md) — `type: richtext(inline)` retires; declare
  `type: richtext` with `inline: true` instead. Blueprint still emits
  `richtext(inline)<markdown>`; `build_transform_schema` gains
  `quillmark:inline: true`. Typed field writes land: one schema-dispatched
  writer (`Card::commit_field` / wasm `commitField` / Python `commit_field`) for
  every field type, plus the schema-bound `TypedWriter`; strict writes fail a
  mismatch at the write, not at render (#893). Live field edits go through the
  writer + `apply(doc)` (the experimental `applyFieldDelta` / change-log surface
  was removed, #886). Card-write verbs become mechanical twins of their
  main-card names — `updateCardField`/`updateCardFields` rename to
  `setCardField`/`setCardFields` (#895). The wasm `Card`
  shape splits by direction: a read `Card` always has `body: Content`, while
  `pushCard` / `insertCard` take a `CardInput` whose `body` still accepts a
  markdown string and whose non-`kind` fields are optional (#917). The richtext
  write grid then collapses into a document-free content codec (`importMarkdown`
  / `exportMarkdown` / `rebase` / `mapPos`) plus the addressed content verbs
  (`install` / `revise` / `applyChange`); the eager
  `bodyMarkdown`/`fieldMarkdown` projections and the per-address body writers
  retire pre-release, `replaceBody` / `replace_body` / `update_card_body` alias
  for one cycle, and richtext fields gain the anchor-preserving `revise_field`
  (#925). On-disk (markdown) identity stays markdown-lossy — the storage DTO is
  the lossless carrier. The binding write surface then settles into two tiers:
  `quill.writer(doc)` (WASM and Python alike) is the documented default —
  typed `set` / `set_all` / `setBody` / `addCard` / `card(i)` and quill-free
  `get` / `getMarkdown` reads — over the content lane and the opaque `setField`
  primitive; the addressed `commit(addr, …)` is deleted (subsumed by the
  writer) and a core-vs-bindings parity table governs drift (#932). Two field
  types join the schema: `plaintext` (navigable unformatted prose over the
  richtext content, via a literal codec) and a promoted first-class `enum`
  (`type: enum` + `values:`, the `enum:` modifier on `string` deprecated for one
  release); `string` narrows to open scalar data (#938). The `pdfform` backend's
  `form.json` slims to a binding layer — `form@0.2.0`: bound `fields` drop
  `type`/`options`/`multiline` (derived from the schema field's kind, `enum`
  values, and `ui.multiline`), unbound widgets move to a `widgets` section,
  binding runs at load so a bad `schema_field` fails with
  `pdfform::dangling_binding` / `pdfform::unbindable_field` instead of a silent
  blank, `form@0.1.0` is rejected, and `$cards` absolute-index addressing is
  removed (#940). Groups gain a card-level `ui.groups` registry: `ui.group`
  becomes a validated reference to a snake_case id (`quill::unknown_group`),
  registry declaration order fixes display order, labels derive from ids with a
  `title:` override, and bare label-as-identity groups are deprecated
  (`quill::implicit_group`); plus two `ui.*` fixes — `ui.group` in a nested
  position is now a load error (`quill::nested_group_not_supported`), and
  typed-dictionary / typed-table-row properties render in declaration order
  instead of alphabetically (#941). Field ordering then goes fully structural:
  `ui.order` is removed (an authored `order:` is a load error), field and
  card-kind display order becomes the key order of the emitted schema
  (declaration order), and the auto-stamped `order:` integer disappears from
  `QuillConfig::schema()` — consumers walk the maps in key order (#941).
- [0.92 → 0.93](0.92-to-0.93.md) — the blueprint placeholder is rebuilt on two
  orthogonal axes (value and marker): blueprints now stamp the `!must_fill` tag
  instead of the `<must-fill>` string sentinel, and bare-null / `field:` now
  falls back to default/zero instead of failing. The fatal
  `validation::must_fill_sentinel` becomes the non-fatal `validation::must_fill`
  warning (it never gates render), `validation::field_absent` is removed, and
  bare scalars (`true`, `47`, `1.0`) coerce into `string` fields.
- [0.91 → 0.92](0.91-to-0.92.md) — the additive `$seed` system key carries
  per-card-kind seed overlays (`seedCard` gains an optional overlay argument);
  **and** the placeholder tag `!fill` is renamed to `!must_fill` with no alias —
  a stale `!fill` silently loses its placeholder status (warning, not error), so
  rewrite your sources. The storage schema bumps to `quillmark/document@0.92.0`
  (gaining `seed` and per-field `nested_fills`; old blobs migrate).
- [0.90 → 0.91](0.90-to-0.91.md) — the closing `~~~` of a card-yaml block must
  be at column zero (an indented `~~~` is payload, fixing silent truncation of
  block-scalar values containing tilde fences); data-field names
  (`[a-z_][a-z0-9_]*`) and the 100-level nesting limit are enforced on every
  input path (parse, storage, wire, mutators — `set_ext` now returns
  `Result`); quill loading skips symlinks and caps file size; Python binding
  raises `ValueError` for non-finite floats, out-of-64-bit integers, and
  over-deep values. Plus no-action round-trip/output fixes (YAML 1.2 comment
  handling, image alt text, nested-key quoting).
- [0.89 → 0.90](0.89-to-0.90.md) — `Quill` becomes engine-free data: the engine
  no longer loads quills (`Quill.fromTree` / `quillmark::quill_from_path`
  replace the factory) and now owns rendering and capability
  (`engine.render` / `open` / `supportedFormats` / `supportsCanvas` take the
  quill). The WASM package exposes a single root `@quillmark/wasm` import — the
  canonical layer over an internal Typst-less core build, with the Typst backend
  loaded lazily on first render; `supportedFormats` leaves
  `Quill.metadata`;
  the backend is resolved at render time; and `QuillSource` collapses into a
  single core `Quill` (`Backend::open(&Quill)`).
- [0.88 → 0.89](0.88-to-0.89.md) — `$quill` mismatches become hard errors: a
  document rendered against a quill whose name differs, or whose version falls
  outside the `$quill` selector, now fails (`quill::name_mismatch` /
  `quill::version_mismatch` via the new `RenderError::QuillMismatch`) instead of
  warning.
- [0.87 → 0.88](0.87-to-0.88.md) — the schema-aware form view (`quill.form`,
  `blankMain`, `blankCard`) is removed in favor of `quill.validate(doc)`; the
  absence diagnostic is renamed `must_fill_absent` → `field_absent` (cell axis
  "Must Fill" → **Unendorsed**); the `example` reference document folds into
  `seedDocument()`; and a single `Card` shape flows in and out — the flat
  `CardInput` is replaced by `Document.makeCard`, and `pushCard` / `insertCard`
  accept the shape they return.
- [0.86 → 0.87](0.86-to-0.87.md) — array fields require an `items` element
  schema, `type: date` folds into a unified `type: datetime`, and schema load
  rejects empty `properties` maps and deeper array nesting.
- [0.85 → 0.86](0.85-to-0.86.md) — partial documents render without error, and
  the canonical card-yaml fence becomes a bare `~~~`.
- [0.83 → 0.84](0.83-to-0.84.md) — the Must Fill / Endorsed schema model
  replaces `required:`, with Python ↔ WASM parity.
- [0.82 → 0.83](0.82-to-0.83.md) — `$`-prefixed plate JSON wire format retires
  the legacy uppercase reserved keys.
- [0.81 → 0.82](0.81-to-0.82.md) — the card-yaml metadata syntax replaces the
  `---`/`QUILL:` frontmatter and fenced cards.
- [`@quillmark/wasm` 0.77 → 0.80](wasm-0.77-to-0.80.md) — migration notes for
  WASM consumers crossing the card-syntax release.

## Related

For how Quills themselves are versioned and how authors target a version, see
[Quill Versioning](../quills/versioning.md).
