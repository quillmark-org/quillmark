# Quillmark Architecture

> **Implementation**: `crates/` (workspace overview)

## TL;DR

Quillmark converts Markdown with card-yaml blocks into output artifacts (PDF, SVG, PNG, TXT). A `Quill` (the renderable shape) orchestrates the pipeline; backends do the heavy compilation.

## Data Flow

1. **Parse** — card-yaml block extraction, bidi stripping, HTML fence normalization
2. **Normalize** — Type coercion, schema defaults, field validation
3. **Compile** — Backend's `open()` receives plate + JSON data and returns a `RenderSession`; `RenderSession::render()` produces artifacts

## Crate Structure

### `quillmark-core`

Foundation types and traits. No backend dependencies; backends depend on this crate.

Key exports: `Backend`, `Artifact`, `OutputFormat`, `RenderOptions`, `RenderSession`, `Document`, `QuillSource`, `FileTreeNode`, `QuillIgnore`, `RenderError`, `Diagnostic`, `Severity`, `Location`, `RenderResult`, `QuillValue`, `QuillReference`, `Version`, `VersionSelector`.

### `quillmark` (orchestration)

High-level API: `Quillmark` (engine), `Quill` (renderable source + backend). Handles parse → normalize → compile, schema coercion, and backend auto-registration. Filesystem walking for `engine.quill_from_path` lives here; core is filesystem-agnostic.

### `backends/quillmark-typst`

Implements `Backend` for PDF, SVG, and PNG. Converts Markdown fields to Typst markup inside `open()`. Resolves fonts and assets. See [PLATE_DATA.md](PLATE_DATA.md).

### `bindings/quillmark-python`

PyO3 bindings published as `quillmark` on PyPI.

> **Status: experimental, second-class binding.** The Python surface lags
> the WASM binding in coverage and in error-shape uniformity. New
> diagnostics / contract work lands in WASM first; Python catches up on a
> best-effort basis. Do not gate releases on Python parity.

### `bindings/quillmark-wasm`

wasm-bindgen bindings published as `@quillmark/wasm`. Supports bundler and Node.js targets. Builds with `--weak-refs` so wasm-bindgen handles are reclaimed by `FinalizationRegistry`; `.free()` remains as the eager teardown hook. Requires Node 14.6+ / current evergreen browsers.

In addition to the byte-output verbs (`Quill.render`, `RenderSession.render`), exposes a Typst-only **canvas preview** path on `RenderSession`: `pageCount`, `pageSize(page)`, `paint(ctx, page, opts?)`, plus `backendId`, `supportsCanvas`, and `warnings`. The painter rasterizes pages directly from the cached `PagedDocument` into a `CanvasRenderingContext2D` or `OffscreenCanvasRenderingContext2D`, sizes the canvas backing store itself, and returns the chosen layout/pixel dimensions. Skips PNG/SVG round-trips. See [PREVIEW.md](PREVIEW.md).

### `bindings/quillmark-cli`

Standalone binary. See [CLI.md](CLI.md).

### `quillmark-fixtures`

Test resources under `resources/`. Helper functions for test setup.

### `quillmark-fuzz`

Property-based fuzz tests (proptest): `parse_fuzz` (YAML/Markdown parsing), `convert_fuzz` (Markdown→Typst conversion + escaping), `emit_roundtrip_fuzz` (emit roundtripping), `filter_fuzz` (filter injection safety), `coerce_fuzz` (type coercion).

## Core Interfaces

- **`Quillmark`** — Engine managing registered backends; auto-registers `TypstBackend` when the `typst` feature is enabled
- **`Quill`** — Renderable shape in `quillmark`: pairs a `QuillSource` with a resolved `Backend`. Exposes `render`, `open`, `dry_run`, `compile_data`, plus `validate` (editor-facing: returns every schema diagnostic, including the non-fatal `field_absent` signal render demotes) and the `seed_document` / `seed_main` / `seed_card` starters that emit committed example documents and cards
- **`QuillSource`** — Pure data in `quillmark-core`: file bundle + config + metadata; no render ability
- **`Backend`** — Trait for output formats (`Send + Sync`): `id()`, `supported_formats()`, `open(plate, &QuillSource, json)`
- **`RenderSession`** — Opaque handle returned by `Backend::open()`; call `render(opts)` to produce artifacts. Exposes `page_count()` and `warnings()` for consumers (e.g. canvas previews) that don't go through `render()`. Backends with richer typed surfaces expose them via a downcast helper that goes through `RenderSession::handle()` + `SessionHandle::as_any` (Typst uses this for canvas preview — see `quillmark_typst::typst_session_of`).
- **`Document`** — Typed in-memory representation of a Quillmark Markdown file (root block, body, cards). Serializes via `serde` to a versioned JSON envelope (`StoredDocument`) for database persistence, decoupled from the evolving Markdown syntax — see [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)
- **`Diagnostic`** — Structured error with severity, code, message, location, hint, source chain
- **`RenderResult`** — Output artifacts + accumulated warnings

## Data Injection

`Backend::open()` receives:
- `plate_content` — raw plate string from `QuillSource.plate` (empty string for plate-less backends)
- `source` — `&QuillSource` with static assets/packages, config, metadata
- `json_data` — JSON object after coercion, defaults, normalization

See [PLATE_DATA.md](PLATE_DATA.md) for the Typst helper package.

## Backend Implementation

Implement three methods of the `Backend` trait: `id()`, `supported_formats()`, `open()`. Return a `RenderSession` wrapping a `SessionHandle` that handles format-specific rendering.

See `backends/quillmark-typst` for the reference implementation.
