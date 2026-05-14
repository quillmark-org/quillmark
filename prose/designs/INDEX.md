# Quillmark Design Index

## Core

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Crate structure and system overview
- **[ERROR.md](ERROR.md)** - Structured diagnostics and cross-language serialization

## Components

- **[MARKDOWN.md](MARKDOWN.md)** - Quillmark Markdown specification (superset of CommonMark)
- **[QUILL.md](QUILL.md)** - Quill bundle structure and file tree API
- **[QUILL_VALUE.md](QUILL_VALUE.md)** - Unified value type for YAML/JSON conversions
- **[VERSIONING.md](VERSIONING.md)** - Quill version resolution
- **[SCHEMAS.md](SCHEMAS.md)** - `QuillConfig` schema model, native validation, and emission overview
- **[BLUEPRINT.md](BLUEPRINT.md)** - Annotated Markdown blueprint for LLM/MCP authoring
- **[LEAVES.md](LEAVES.md)** - Composable leaves with unified LEAVES array
- **[LEAF_REWORK.md](LEAF_REWORK.md)** - Design rationale for the inline-leaf syntax
- **[GLUE_METADATA.md](GLUE_METADATA.md)** - Plate data injection

## Backends

- Typst backend: see `crates/backends/typst/` rustdoc

## Bindings

- **[CLI.md](CLI.md)** - Command-line interface
- **[PREVIEW.md](PREVIEW.md)** - WASM-only Typst canvas preview path
- Python bindings (PyO3): see `crates/bindings/python/` rustdoc
- WebAssembly bindings: see `crates/bindings/wasm/` rustdoc

## Infrastructure

- **[CI_CD.md](CI_CD.md)** - CI/CD workflows
