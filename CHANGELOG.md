# Changelog

## Unreleased

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

