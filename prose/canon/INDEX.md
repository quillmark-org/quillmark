# Quillmark Canon Index

## Core

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Crate structure and system overview
- **[ERROR.md](ERROR.md)** - Structured diagnostics and cross-language serialization

## Components

- **[../references/markdown-spec.md](../references/markdown-spec.md)** - Quillmark Markdown specification (superset of CommonMark)
- **[DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md)** - Versioned JSON serialization of `Document` for database persistence
- **[QUILL.md](QUILL.md)** - Quill resource file structure and the portable, declarative `Quill` data type
- **[QUILL_VALUE.md](QUILL_VALUE.md)** - Unified value type for YAML/JSON conversions
- **[VERSIONING.md](VERSIONING.md)** - Quill version format and `$quill` reference syntax (selector parsed, not runtime-resolved)
- **[SCHEMAS.md](SCHEMAS.md)** - `QuillConfig` schema model, native validation, and emission overview
- **[BLUEPRINT.md](BLUEPRINT.md)** - Annotated Markdown blueprint for LLM/MCP authoring
- **[PROGRAMMATIC.md](PROGRAMMATIC.md)** - Building documents in memory (blank canvas, batched mutators) for automation
- **[CARDS.md](CARDS.md)** - Composable cards delivered on the `$cards` plate-JSON array
- **[PLATE_DATA.md](PLATE_DATA.md)** - Plate data injection

## Backends

- **[CONVERT.md](CONVERT.md)** - How the Typst backend lowers richtext body content (the content) to Typst markup
- Typst backend internals: see `crates/backends/typst/` rustdoc
- **[../../docs/quills/pdfform-backend.md](../../docs/quills/pdfform-backend.md)** - The `pdfform` backend: fill an existing AcroForm PDF (real interactive fields, Technique A), built on the `quillmark-pdf` stamp spine

## Bindings

- **[BINDINGS.md](BINDINGS.md)** - Language surfaces (Python, WASM, CLI) over the one core engine
- **[CLI.md](CLI.md)** - Command-line interface
- **[PREVIEW.md](PREVIEW.md)** - WASM live preview: LiveSession (apply/ChangeSet) + multi-backend canvas paint (Typst, pdfform)

## Infrastructure

- **[CI_CD.md](CI_CD.md)** - CI/CD workflows
