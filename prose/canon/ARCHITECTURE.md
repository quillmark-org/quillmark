# Quillmark Architecture

> **Implementation**: `crates/` (workspace overview)

## TL;DR

Quillmark is a schema-driven document engine: it turns Markdown with card-yaml blocks into a fully typeset document (PDF, SVG, PNG). A `Quill` is portable, declarative data whose schema drives validation and scaffolding (parse / validate / schema / seed / blueprint / compile); the `Quillmark` engine is the thin-but-mandatory core every render routes through — a backend registry + render dispatcher; backends do the heavy compilation.

## Data Flow

1. **Parse** — card-yaml block extraction, bidi stripping, HTML fence normalization
2. **Normalize** — Type coercion, schema defaults, field validation
3. **Compile** — Backend's `open()` receives the quill + JSON data and returns a `LiveSession`; `LiveSession::render()` produces artifacts

## Crate Structure

### `quillmark-core`

Foundation types and traits; depends on `quillmark-content` (the leaf rich-text
primitive one layer below it). No backend dependencies; backends depend on this
crate.

Key exports: `Backend`, `Artifact`, `OutputFormat`, `RenderOptions`, `LiveSession`, `Document`, `Quill`, `FileTreeNode`, `QuillIgnore`, `RenderError`, `Diagnostic`, `Severity`, `Location`, `RenderResult`, `QuillValue`, `QuillReference`, `Version`, `VersionSelector`. `Quill` is the single quill type — portable, validated data with the pure config-read operations (`validate`, `schema`, `metadata`, `blueprint`, `seed_*`, `compile_data`, `dry_run`); construct it with `Quill::from_tree`.

### `quillmark-content`

The leaf rich-text primitive `quillmark-core` depends on: the `Content` content
model (one USV text with line attributes, anchored marks, embedded islands), its
canonical byte-deterministic serialization (the frozen wire form storage and the
render seam share), the markdown⇄content import/export codecs, and edit deltas.
The workspace's only markdown parser (`pulldown-cmark`) lives here, in
`quillmark-content::import`, run once at ingest. **Invariant:** the markdown
engine appears exactly once in the workspace; no render path parses markdown —
backends lower the content.

### `quillmark` (orchestration)

High-level API: `Quillmark` (the engine — a backend registry + render dispatcher) plus the `quill_from_path` loader. Re-exports core's `Quill`. Handles backend resolution at render time and auto-registration. Filesystem walking for `quill_from_path` lives here; core is filesystem-agnostic (in-memory loading is `Quill::from_tree` in core). The engine does not construct quills — it only renders them.

### `backends/quillmark-typst`

Implements `Backend` for PDF, SVG, and PNG. Lowers each richtext content field's content to Typst markup at codegen (`emit::emit_richtext`), recording a per-segment source map. Resolves fonts and assets. See [CONVERT.md](CONVERT.md) and [PLATE_DATA.md](PLATE_DATA.md).

### `backends/quillmark-pdfform`

The second backend: fills an existing AcroForm PDF rather than typesetting from scratch. Resolves card values against the quill's `form.json` spec and stamps them onto the base `form.pdf` as real interactive fields (Technique A — `NeedAppearances`, no baked appearance streams). The PDF deliverable is always an interactive AcroForm; the backend also emits SVG and PNG (and a WASM canvas raster) by pre-flattening values into the page content streams (hayro raster). Field geometry is a session-level query (`LiveSession::regions()`) — per-field geometry keyed on the schema field path, no bound value. See [docs/quills/pdfform-backend.md](../../docs/quills/pdfform-backend.md) and [PREVIEW.md](PREVIEW.md).

### `quillmark-pdf`

The shared PDF stamp spine: Typst-free, `pdf-writer`-only leaf infrastructure consumed by `quillmark-pdfform`. A minimal byte-level reader plus a single incremental-update appender that splices a fresh `/AcroForm` (and `/Info` `/Producer` stamp) onto a base PDF. Deliberately small — it hard-errors on out-of-contract input (xref streams, encryption, indirect `/Annots`, non-zero-generation base objects) rather than parsing the full format.

### `bindings/*`

Language surfaces over the one core engine: `quillmark-python` (PyO3, PyPI),
`quillmark-wasm` (wasm-bindgen, npm), and `quillmark-cli` (the `quillmark`
binary). See [BINDINGS.md](BINDINGS.md).

### `quillmark-fixtures`

Test resources under `resources/`. Helper functions for test setup.

### `quillmark-fuzz`

Property-based fuzz tests (proptest): `parse_fuzz` (YAML/Markdown parsing), `convert_fuzz` (Markdown→Typst conversion + escaping), `emit_roundtrip_fuzz` (emit roundtripping), `filter_fuzz` (filter injection safety), `coerce_fuzz` (type coercion).

## Core Interfaces

- **`Quillmark`** — Engine: a backend registry + render dispatcher. Auto-registers `TypstBackend` when the `typst` feature is enabled. Resolves a quill's declared backend at render time (erroring `engine::backend_not_found` on no match) and owns the backend-dependent surface — `render`, `open`, `supported_formats(&quill)`, `supports_canvas(&quill)`. It does not construct quills.
- **`Quill`** — The single quill type in `quillmark-core`: portable, declarative data (file bundle + config + metadata, tagged with a declared backend id). Held by value. Exposes the pure config-read operations: `dry_run`, `compile_data`, `backend_id`, plus `validate` (editor-facing: returns every schema diagnostic, including the non-fatal `validation::must_fill` warning raised for each `!must_fill` marker) and the `seed_document` / `seed_main` / `seed_card` starters that emit committed example documents and cards. Construct with `Quill::from_tree` or `quillmark::quill_from_path`
- **`Backend`** — Trait for output formats (`Send + Sync`): `id()`, `supported_formats()`, `open(&Quill, json)`. There is no universal template input: a backend reads whatever static inputs it needs (a Typst plate, a `form.pdf`) from the quill's own files. No canvas-capability method — capability is derived (`LiveSession::supports_canvas()` from the session seam; `formats_support_canvas()` as a pre-session hint)
- **`LiveSession`** — Opaque live session returned by `Backend::open()`: a persistent compiler whose reads (`render(opts)`, the canvas seam, `regions()`/`field_at()`) serve its current compile, and whose `apply(json)` recompiles in place, transactionally (on `Err` reads keep serving the last-good compile) — returning a `ChangeSet` of dirty pages. Exposes `page_count()` and `warnings()` for consumers that don't go through `render()`. The canvas-preview seam lives on `SessionHandle` itself (`page_size_pt`/`render_rgba`, default `None`); a canvas backend overrides both, and the WASM painter dispatches generically through them — no per-backend downcast (Typst and pdfform both ride this seam — see [PREVIEW.md](PREVIEW.md)). A backend with a different richer typed surface can still downcast via `LiveSession::handle()` + `SessionHandle::as_any`.
- **`Document`** — Typed in-memory representation of a Quillmark Markdown file (root block, body, cards). Serializes via `serde` to a versioned JSON envelope (`StoredDocument`) for database persistence, decoupled from the evolving Markdown syntax — see [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)
- **`Diagnostic`** — Structured error with severity, code, message, location, hint, source chain
- **`RenderResult`** — Output artifacts + accumulated warnings

## Data Injection

`Backend::open()` receives:
- `source` — `&Quill` with static assets/packages, config, metadata. A backend reads its own inputs from here: the Typst backend reads the template named by `typst.plate_file` from `source.files()`; pdfform reads `form.pdf` / `form.json`
- `json_data` — JSON object after coercion, defaults, normalization

See [PLATE_DATA.md](PLATE_DATA.md) for the Typst helper package.

## Backend Implementation

Implement `id()`, `supported_formats()`, and `open()` of the `Backend` trait. To paint to a canvas, override the `SessionHandle` seam (`page_size_pt` / `render_rgba`) on the returned session — capability is derived from that seam, so there is no separate flag to set. Return a `LiveSession` wrapping a `SessionHandle` that handles format-specific rendering.

See `backends/quillmark-typst` for the reference implementation.
