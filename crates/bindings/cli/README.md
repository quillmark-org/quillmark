# Quillmark CLI

Command-line interface for the Quillmark Markdown rendering system.

Maintained by [TTQ](https://tonguetoquill.com).

## Overview

`quillmark-cli` is a standalone executable that renders Markdown files with YAML frontmatter into PDF, SVG, and other formats using Quillmark templates.

## Installation

### From crates.io (Recommended)

```bash
cargo install quillmark-cli
```

The binary will be installed to `~/.cargo/bin/quillmark` (ensure `~/.cargo/bin` is in your PATH).

### From Git Repository

```bash
# Install latest from main branch
cargo install --git https://github.com/nibsbin/quillmark quillmark-cli

# Install from specific branch or tag
cargo install --git https://github.com/nibsbin/quillmark --branch main quillmark-cli
```

### From Local Source

```bash
# From workspace root
cargo install --path bindings/quillmark-cli

# Or build without installing
cargo build --release -p quillmark-cli
# Binary will be at: target/release/quillmark
```

## Quick Start

Render a markdown file using a quill template:

```bash
quillmark render path/to/quill document.md
```

The output will be saved as `document.pdf` by default.

## Usage

### Basic Rendering

```bash
# Render to PDF (default format)
quillmark render ./quills/usaf_memo memo.md

# Specify output file
quillmark render ./quills/usaf_memo memo.md -o output/final.pdf

# Render to different format
quillmark render ./quills/usaf_memo memo.md --format svg
```

### Rendering the generated blueprint

If you omit `MARKDOWN_FILE`, the quill's generated blueprint is rendered:

```bash
quillmark render ./quills/usaf_memo
```

### Advanced Options

```bash
# Output to stdout (useful for piping)
quillmark render ./quills/usaf_memo memo.md --stdout > output.pdf

# Verbose output
quillmark render ./quills/usaf_memo memo.md --verbose

# Quiet mode (suppress all non-error output)
quillmark render ./quills/usaf_memo memo.md --quiet
```

## Command Reference

### `quillmark render`

Render a markdown file to the specified output format.

**Usage:**
```
quillmark render [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]
```

**Arguments:**
- `<QUILL_PATH>` - Path to quill directory
- `[MARKDOWN_FILE]` - Path to markdown file with YAML frontmatter (optional; when omitted, the quill's generated blueprint is rendered)

**Options:**
- `-o, --output <FILE>` - Output file path (default: derived from input filename)
- `-f, --format <FORMAT>` - Output format: pdf, svg, txt (default: pdf)
- `--stdout` - Write output to stdout instead of file
- `-v, --verbose` - Show detailed processing information
- `--quiet` - Suppress all non-error output

## Examples

### Example: Render USAF Memo

```bash
quillmark render \
  crates/fixtures/resources/quills/usaf_memo/0.1.0 \
  crates/fixtures/resources/quills/usaf_memo/0.1.0/example.md \
  -o usaf_memo_output.pdf
```

### Example: Generate SVG

```bash
quillmark render ./quills/my_template \
  document.md \
  --format svg \
  -o output.svg
```

### Example: Pipeline Usage

```bash
# Render and immediately view with a PDF viewer
quillmark render ./quills/usaf_memo memo.md --stdout | evince -

# Render to stdout and pipe to another tool
quillmark render ./quills/usaf_memo memo.md --stdout > final.pdf
```

## Error Handling

The CLI provides clear error messages for common issues:

- **Missing markdown file**: `Markdown file not found: path/to/file.md`
- **Missing quill**: `Quill directory not found: path/to/quill`
- **Parse errors**: Line numbers and context for YAML or markdown issues
- **Template errors**: Compilation diagnostics from the rendering backend

## Exit Codes

- `0` - Success
- `1` - Error occurred (see stderr for details)

## Development

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running Locally

```bash
cargo run -- render path/to/quill example.md
```

## Design Documentation

For architectural details and design decisions, see:
- [CLI Design Document](../../prose/designs/CLI.md)
- [Implementation Plan](../../prose/plans/cli-basic-render.md)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE) for details.
