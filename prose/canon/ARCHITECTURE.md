# Quillmark Architecture

> **Implementation**: `crates/` (workspace overview)

## TL;DR

Quillmark is a schema-driven document engine: it turns Markdown with card-yaml blocks into a fully typeset document (PDF, SVG, PNG). A `Quill` is portable, declarative data whose schema drives validation and scaffolding (parse / validate / schema / seed / blueprint / compile); the `Quillmark` engine is the thin-but-mandatory core every render routes through — a backend registry + render dispatcher; backends do the heavy compilation.

## Data Flow

1. **Parse** — card-yaml block extraction, bidi stripping, HTML fence normalization
2. **Normalize** — Type coercion, schema defaults, field validation
3. **Compile** — Backend's `open()` receives plate + JSON data and returns a `RenderSession`; `RenderSession::render()` produces artifacts

## Crate Structure

### `quillmark-core`

Foundation types and traits. No backend dependencies; backends depend on this crate.

Key exports: `Backend`, `Artifact`, `OutputFormat`, `RenderOptions`, `RenderSession`, `Document`, `Quill`, `FileTreeNode`, `QuillIgnore`, `RenderError`, `Diagnostic`, `Severity`, `Location`, `RenderResult`, `QuillValue`, `QuillReference`, `Version`, `VersionSelector`. `Quill` is the single quill type — portable, validated data with the pure config-read operations (`validate`, `schema`, `metadata`, `blueprint`, `seed_*`, `compile_data`, `dry_run`); construct it with `Quill::from_tree`.

### `quillmark` (orchestration)

High-level API: `Quillmark` (the engine — a backend registry + render dispatcher) plus the `quill_from_path` loader. Re-exports core's `Quill`. Handles backend resolution at render time and auto-registration. Filesystem walking for `quill_from_path` lives here; core is filesystem-agnostic (in-memory loading is `Quill::from_tree` in core). The engine does not construct quills — it only renders them.

### `backends/quillmark-typst`

Implements `Backend` for PDF, SVG, and PNG. Converts Markdown fields to Typst markup inside `open()`. Resolves fonts and assets. See [PLATE_DATA.md](PLATE_DATA.md).

### `bindings/*`

Language surfaces over the one core engine: `quillmark-python` (PyO3, PyPI),
`quillmark-wasm` (wasm-bindgen, npm), `quillmark-dotnet` (P/Invoke, NuGet), and
`quillmark-cli` (the `quillmark` binary). See [BINDINGS.md](BINDINGS.md).

### `quillmark-fixtures`

Test resources under `resources/`. Helper functions for test setup.

### `quillmark-fuzz`

Property-based fuzz tests (proptest): `parse_fuzz` (YAML/Markdown parsing), `convert_fuzz` (Markdown→Typst conversion + escaping), `emit_roundtrip_fuzz` (emit roundtripping), `filter_fuzz` (filter injection safety), `coerce_fuzz` (type coercion).

## Core Interfaces

- **`Quillmark`** — Engine: a backend registry + render dispatcher. Auto-registers `TypstBackend` when the `typst` feature is enabled. Resolves a quill's declared backend at render time (erroring `UnsupportedBackend` on no match) and owns the backend-dependent surface — `render`, `open`, `supported_formats(&quill)`, `supports_canvas(&quill)`. It no longer constructs quills
- **`Quill`** — The single quill type in `quillmark-core`: portable, declarative data (file bundle + config + metadata, tagged with a declared backend id). Held by value. Exposes the pure config-read operations: `dry_run`, `compile_data`, `backend_id`, plus `validate` (editor-facing: returns every schema diagnostic, including the non-fatal `validation::must_fill` warning raised for each `!must_fill` marker) and the `seed_document` / `seed_main` / `seed_card` starters that emit committed example documents and cards. Construct with `Quill::from_tree` or `quillmark::quill_from_path`
- **`Backend`** — Trait for output formats (`Send + Sync`): `id()`, `supported_formats()`, `supports_canvas()` (default `false`), `open(plate, &Quill, json)`
- **`RenderSession`** — Opaque handle returned by `Backend::open()`; call `render(opts)` to produce artifacts. Exposes `page_count()` and `warnings()` for consumers (e.g. canvas previews) that don't go through `render()`. Backends with richer typed surfaces expose them via a downcast helper that goes through `RenderSession::handle()` + `SessionHandle::as_any` (Typst uses this for canvas preview — see `quillmark_typst::typst_session_of`).
- **`Document`** — Typed in-memory representation of a Quillmark Markdown file (root block, body, cards). Serializes via `serde` to a versioned JSON envelope (`StoredDocument`) for database persistence, decoupled from the evolving Markdown syntax — see [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)
- **`Diagnostic`** — Structured error with severity, code, message, location, hint, source chain
- **`RenderResult`** — Output artifacts + accumulated warnings

## Data Injection

`Backend::open()` receives:
- `plate_content` — raw plate string from `Quill.plate` (empty string for plate-less backends)
- `source` — `&Quill` with static assets/packages, config, metadata
- `json_data` — JSON object after coercion, defaults, normalization

See [PLATE_DATA.md](PLATE_DATA.md) for the Typst helper package.

## Backend Implementation

Implement `id()`, `supported_formats()`, and `open()` of the `Backend` trait; optionally override `supports_canvas()` (defaults to `false`) if the backend can paint to a canvas. Return a `RenderSession` wrapping a `SessionHandle` that handles format-specific rendering.

See `backends/quillmark-typst` for the reference implementation.
