# Quillmark

[![Crates.io](https://img.shields.io/crates/v/quillmark.svg)](https://crates.io/crates/quillmark)
[![PyPI](https://img.shields.io/pypi/v/quillmark.svg?color=3776AB)](https://pypi.org/project/quillmark/)
[![npm](https://img.shields.io/npm/v/@quillmark/wasm.svg?color=CB3837)](https://www.npmjs.com/package/@quillmark/wasm)
[![CI](https://github.com/quillmark-org/quillmark/workflows/CI/badge.svg)](https://github.com/quillmark-org/quillmark/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-lightgray.svg)](LICENSE)
 
In a nutshell: quills define document presentation and schema. Use quills to generate fully formatted documents from markdown or code.

Maintained by [quillmark-org](https://github.com/quillmark-org).

**UNDER DEVELOPMENT** — APIs may change.

## Documentation

- **[User Guide](https://quillmark-org.github.io/quillmark/)** - Tutorials, concepts, and bindings
- **[Rust API Reference](https://docs.rs/quillmark)** - Rust crate docs
- **[Changelog](CHANGELOG.md)** - Release notes and version history ([GitHub Releases](https://github.com/quillmark-org/quillmark/releases))

## Installation

```bash
cargo add quillmark
```

## Quick Start (Rust)

```rust
use quillmark::{quill_from_path, Document, OutputFormat, Quillmark, RenderOptions};

// A `Quill` is portable, declarative data.
let quill = quill_from_path("path/to/quill")?;
let engine = Quillmark::new();

let markdown = r#"~~~
$quill: my_quill
$kind: main
title: Example
~~~

# Hello World
"#;

let doc = Document::from_markdown(markdown)?;
let result = engine.render(
    &quill,
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
- **prose/canon** - Design documentation

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
