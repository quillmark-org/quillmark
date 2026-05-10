# Quillmark

[![Crates.io](https://img.shields.io/crates/v/quillmark.svg)](https://crates.io/crates/quillmark)
[![PyPI](https://img.shields.io/pypi/v/quillmark.svg?color=3776AB)](https://pypi.org/project/quillmark/)
[![npm](https://img.shields.io/npm/v/@quillmark/wasm.svg?color=CB3837)](https://www.npmjs.com/package/@quillmark/wasm)
[![CI](https://github.com/nibsbin/quillmark/workflows/CI/badge.svg)](https://github.com/nibsbin/quillmark/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-lightgray.svg)](LICENSE)

A format-first Markdown rendering system that converts Markdown with YAML frontmatter into PDF, SVG, PNG, and other output formats.

Maintained by [TongueToQuill](https://tonguetoquill.com).

**UNDER DEVELOPMENT** — APIs may change.

## Features

- **Format-driven design**: Quills define structure and styling; Markdown provides content
- **Schema-backed validation**: Strong field coercion and validation via `QuillConfig`
- **Multiple backends**: Typst backend supports PDF/SVG/PNG output
- **Structured diagnostics**: Path-aware errors and warnings

## Documentation

- **[User Guide](https://quillmark.readthedocs.io)** - Tutorials, concepts, and bindings
- **[Rust API Reference](https://docs.rs/quillmark)** - Rust crate docs

## Installation

```bash
cargo add quillmark
```

## Quick Start (Rust)

```rust
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

let engine = Quillmark::new();
let quill = engine.quill_from_path("path/to/quill")?;

let markdown = r#"---
QUILL: my_quill
title: Example
---

# Hello World
"#;

let doc = Document::from_markdown(markdown)?;
let result = quill.render(
    &doc,
    &RenderOptions {
        output_format: Some(OutputFormat::Pdf),
        ..Default::default()
    },
)?;

let pdf_bytes = &result.artifacts[0].bytes;
```

## Examples

```bash
cargo run --example appreciated_letter
cargo run --example usaf_memo
cargo run --example taro
```

## Project Structure

- **crates/core** - Core parsing, schema, and backend traits
- **crates/quillmark** - Rust orchestration API
- **crates/backends/typst** - Typst backend
- **crates/bindings/python** - Python bindings
- **crates/bindings/wasm** - WebAssembly bindings
- **crates/bindings/cli** - Command-line interface
- **crates/fixtures** - Test fixtures and sample Quill templates
- **crates/fuzz** - Property-based fuzzing tests

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
