# Migration Guides

Quillmark evolves through deliberate, documented releases. When a release
changes the document syntax, the plate-JSON wire format, or a public API in a
way that is not backward compatible, it ships with a migration guide describing
exactly what changed and how to update your documents, quills, and host code.

Many of these are **hard cutovers** — the old form stops parsing or compiling,
so a guide is the path forward, not an optional read. Each guide is scoped to a
single version step; to cross several versions, work through the relevant
guides in order.

## Available guides

- [0.89 → 0.90](0.89-to-0.90.md) — `Quill` becomes engine-free data: the engine
  no longer loads quills (`Quill.fromTree` / `quillmark::quill_from_path`
  replace the factory) and now owns rendering and capability
  (`engine.render` / `open` / `supportedFormats` / `supportsCanvas` take the
  quill). The WASM package splits into a Typst-less `@quillmark/wasm/core` and
  the Typst-backed root `@quillmark/wasm` superset; `supportedFormats` leaves
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
