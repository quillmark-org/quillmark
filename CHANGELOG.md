# Changelog

## v0.87.0 - 2026-06-01

- Consolidate schema literal validation into shared primitive (#680)
- Remove FieldType::Date; unify datetime under YAML 1.1 timestamp grammar (#679)
- Reject object fields with empty properties maps (#678)
- Make object zero values shape-valid by recursively filling properties (#677)
- Fix doc/comment nits from array-items review (#675)
- docs: add overview index page to migration section (#674)
- Collapse array/markdown handling into recursive passes (#673)
- Make primitively typed arrays first-class via items (#672)
- Migrate documentation hosting from Read the Docs to GitHub Pages (#671)
- canon + docs: partial documents are first-class citizens (#670)


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

