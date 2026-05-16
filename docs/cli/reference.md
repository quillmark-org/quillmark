# CLI Reference

Command-line interface for Quillmark rendering.

## Installation

```bash
cargo install quillmark-cli
```

## Commands

### render

Render markdown documents to PDF, SVG, PNG, or text. Optionally emit compiled JSON data.

```bash
quillmark render [OPTIONS] <QUILL_PATH> [MARKDOWN_FILE]
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory
- `[MARKDOWN_FILE]`: Path to markdown file with YAML frontmatter (optional — when omitted, the quill's example content is used)

`<QUILL_PATH>` selects the local quill bundle used for rendering. `MARKDOWN_FILE` frontmatter still requires top-level `QUILL` during parsing.

**Options:**

- `-o <PATH>` / `--output <PATH>`: Output file path (default: derived from input filename, e.g. `input.pdf`)
- `-f <FORMAT>` / `--format <FORMAT>`: Output format: `pdf`, `svg`, `png`, `txt` (default: `pdf`)
- `--output-data <DATA_FILE>`: Write compiled JSON data to a file
- `-v` / `--verbose`: Show detailed processing information
- `--quiet`: Suppress all non-error output
- `--stdout`: Write output to stdout instead of file

**Examples:**

```bash
# Render to PDF
quillmark render ./invoice-quill input.md -o output.pdf

# Render to SVG
quillmark render ./my-quill input.md -f svg -o output.svg

# Emit compiled data for inspection
quillmark render ./my-quill input.md --output-data data.json

# Output to stdout
quillmark render ./my-quill input.md --stdout > output.pdf

# Render the quill's built-in example
quillmark render ./my-quill
```

### schema

Extract the public schema YAML contract from a quill's field definitions.

```bash
quillmark schema [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-o <FILE>` / `--output <FILE>`: Output file (default: stdout)

**Examples:**

```bash
# Print schema to stdout
quillmark schema ./my-quill

# Save schema to file
quillmark schema ./my-quill -o schema.yaml

# Use with other tools
quillmark schema ./my-quill | grep '^  description:'
```

### specs

Print a quill's Markdown blueprint — an annotated document showing the quill's fields, constraints, and examples. The blueprint is dense enough to replace the schema for LLM consumers and is itself a valid document an author can fill in.

```bash
quillmark specs [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-o <FILE>` / `--output <FILE>`: Output file (default: stdout)

**Examples:**

```bash
# Print blueprint to stdout
quillmark specs ./my-quill

# Save blueprint to file
quillmark specs ./my-quill -o blueprint.md
```

### validate

Validate quill configuration and structure.

```bash
quillmark validate [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `-v` / `--verbose`: Show verbose output with all validation details

**Examples:**

```bash
# Validate quill structure
quillmark validate ./my-quill

# Verbose validation
quillmark validate ./my-quill -v
```

### info

Display metadata and information about a quill.

```bash
quillmark info [OPTIONS] <QUILL_PATH>
```

**Arguments:**

- `<QUILL_PATH>`: Path to quill directory

**Options:**

- `--json`: Output as machine-readable JSON instead of human-readable format

**Examples:**

```bash
# Display quill info
quillmark info ./my-quill

# Output as JSON
quillmark info ./my-quill --json

# Use with other tools
quillmark info ./my-quill --json | jq '.name'
```

## Exit Codes

- `0` — success
- `1` — error (invalid arguments, file not found, parse error, compilation error, etc.)

## Environment Variables

- `RUST_LOG` — log level (e.g. `RUST_LOG=debug`)
- `NO_COLOR` — disable colored output
