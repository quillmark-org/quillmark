# Quill Resource File Structure and API

> **Status**: Final design — opinionated, no backward compatibility
> **Implementation**: `crates/core/src/quill/` (`QuillSource`),
> `crates/quillmark/src/orchestration/` (`Quill`)

## Type split: `QuillSource` vs `Quill`

Two types model a loaded quill:

- **`QuillSource`** (in `quillmark-core`) is the authored input — file bundle,
  parsed config, and metadata. It does not render.
- **`Quill`** (in `quillmark`) is the renderable shape — an `Arc<QuillSource>`
  paired with a resolved backend. Constructed only by the engine.

Bindings expose `Quill` only; `QuillSource` is a Rust-internal type.

A **quiver** is a collection of quills. The bundled fixtures under
`crates/fixtures/resources/quills/` are one quiver; the
[quill authoring contract](BLUEPRINT.md#guarantees) is verified across the
whole quiver by `every_quill_in_quiver_renders`
(`crates/quillmark/tests/quiver_test.rs`).

## Internal File Structure

```rust
pub enum FileTreeNode {
    File { contents: Vec<u8> },
    Directory { files: HashMap<String, FileTreeNode> },
}

pub struct QuillSource {
    pub(crate) metadata: HashMap<String, QuillValue>,
    pub(crate) plate: Option<String>,
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
}

pub struct Quill {
    source: Arc<QuillSource>,
    backend: Arc<dyn Backend>,
}
```

`metadata` is populated from `Quill.yaml` fields plus computed entries: `backend`, `description`, `version`, `author`, and any `typst_*` keys from the `[typst]` section.

## In-memory Tree Contract (`engine.quill(tree)`)

In-memory construction routes through the engine as `engine.quill(tree)`. The
core `QuillSource::from_tree` constructor is the single authoritative in-memory
entry point; filesystem walking (`engine.quill_from_path`) lives in
`quillmark` rather than in core. Input is a `FileTreeNode` directory tree
with UTF-8 and binary file contents represented as bytes.

For JS/WASM consumers this is exposed as `engine.quill(...)` accepting a
`Map<string, Uint8Array>` (path→bytes). Plain objects (`Record<string, Uint8Array>`)
are also accepted and walked via `Object.entries` at the boundary.

Validation rules:
1. Root MUST be a directory node
2. `Quill.yaml` MUST exist and be valid YAML
3. The `plate_file` referenced in `Quill.yaml`, if specified, MUST exist
4. File paths use `/` separators and are resolved relative to root

## `Quill.yaml` Structure

Required top-level sections: `quill` (bundle metadata). Optional: `main` (document fields), `card_kinds` (card kind definitions), `typst` (backend config).

```yaml
quill:
  name: my_quill          # required; snake_case
  backend: typst          # required
  version: "1.0.0"        # required; semver (MAJOR.MINOR.PATCH or MAJOR.MINOR)
  description: A beautiful format  # required; non-empty
  author: Jane Doe        # optional; defaults to "Unknown"
  plate_file: plate.typ   # optional; path to Typst template

main:
  fields:
    title:
      type: string
      description: Document title
    count:
      type: integer
      description: Whole-number count

card_kinds:
  quote:
    description: A single pull quote
    ui:
      title: Quote block      # optional UI display label
    body:
      example: The quote text  # optional editor placeholder
    fields:
      author:
        type: string
        description: Quote author

typst:
  packages:
    - "@preview/some-package:1.0.0"
```

Field names must be `snake_case` (match `[a-z_][a-z0-9_]*`). Capitalized or `$`-prefixed keys are rejected by the editor surface — document-level metadata sits on dedicated `$`-prefixed keys in the plate JSON (`$quill`, `$body`, `$cards`, `$kind`), and user fields stay lowercase so they cannot shadow it. Standalone `object` fields require a `properties` map. Every `array` field requires an `items:` element schema: use `items: { type: string }` (or `integer`, `markdown`, …) for a list of scalars, and `items: { type: object, properties: … }` for a list of objects.

Metadata resolution:
- `name`, `description`, `backend`, `version`, `author` are direct struct fields on `QuillConfig`. `description` (required, non-empty in the `quill:` section) describes the quill itself; it is independent of `QuillConfig.main.description`, which is the optional schema description authored under `main:` like any other card kind.
- `metadata` on `Quill` stores `backend`, `description`, `version`, `author`, and `typst_*` keys from the `[typst]` section. The `quill:` section accepts only the documented keys; unknown keys produce a `quill::unknown_key` error rather than landing in `metadata`.

## Strict Parsing

`Quill.yaml` is parsed strictly: every problem the parser can detect is collected and reported in one pass as a `Vec<Diagnostic>`, rather than failing on the first error or silently dropping unsupported shapes. Specifically:

- Unknown keys in the `quill:` section error with `quill::unknown_key` (typos like `platefile` are not silently captured).
- Unknown top-level sections error with `quill::unknown_section` (typos like `card_kind:` are not silently ignored). Root-level `fields:` gets a targeted hint pointing to `main.fields:`.
- Field schemas that fail to parse (e.g. a bare `title:`, missing `type:`) error with `quill::field_parse_error` and an actionable hint where applicable, rather than being dropped from the schema.
- `object` fields without a `properties` map error with `quill::object_missing_properties`; an empty `properties` map errors with `quill::object_empty_properties`; an object nested inside another object errors with `quill::nested_object_not_supported`.
- Malformed `quill.ui` / `main.ui` blocks error with `quill::invalid_ui` rather than being silently discarded.
- Malformed `main.body` / `card_kinds.<name>.body` blocks error with `quill::invalid_body`.
- A `body.example` set together with `body.enabled: false` warns with `quill::body_example_unused` (the example has no effect).

Errors flow through `RenderError::QuillConfig { diags: Vec<Diagnostic> }` and surface to bindings as a structured array (`err.diagnostics` in WASM, `.diagnostics` attribute in Python).

## File Ignore Rules

When loading from disk, `Quillmark::quill_from_path` respects a `.quillignore` file at the bundle root. If absent, default patterns apply: `.git/`, `.gitignore`, `.quillignore`, `target/`, `node_modules/`.

## API

Construction:
- `Quillmark::quill_from_path(path)` — load render-ready quill from filesystem directory
- `Quillmark::quill(tree)` — load render-ready quill from in-memory file tree

Note: `Quill::from_json` is not part of the public API.

File access on `FileTreeNode`:
- `file_exists(path)` / `get_file(path)` — check/read file
- `dir_exists(path)` / `list_files(path)` / `list_subdirectories(path)` — directory navigation

Path rules:
- Use forward slashes (`/`); absolute paths and `..` traversal are rejected
- Root: use `""` (empty string)
- `get_file()` returns `&[u8]` for all files
