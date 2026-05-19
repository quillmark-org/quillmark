# Quillmark — Python bindings

Python bindings for Quillmark's format-first Markdown rendering engine.

Maintained by [TTQ](https://tonguetoquill.com).

## Installation

```bash
pip install quillmark
```

## Quick Start

```python
from quillmark import Quillmark, Document, OutputFormat

engine = Quillmark()
quill = engine.quill_from_path("path/to/quill")

markdown = """---
QUILL: my_quill
title: Hello World
---

# Hello
"""

parsed = Document.from_markdown(markdown)
result = quill.render(parsed, OutputFormat.PDF)
result.artifacts[0].save("output.pdf")

# Round-trip: mutate, emit, re-parse
parsed.set_field("title", "Updated")
emitted = parsed.to_markdown()
reparsed = Document.from_markdown(emitted)
assert reparsed.frontmatter["title"] == "Updated"
```

## API Overview

### `Quillmark`

```python
engine = Quillmark()
engine.registered_backends()      # ['typst']
quill = engine.quill_from_path("path/to/quill")
```

### `Quill`

```python
quill = engine.quill_from_path("path")
result = quill.render(parsed, OutputFormat.PDF)
session = quill.open(parsed)
quill.dry_run(parsed)
```

### `Document`

```python
doc = Document.from_markdown(markdown)
emitted = doc.to_markdown()          # canonical Quillmark Markdown

# Versioned storage DTO — use this to persist a document across a
# process restart or crate upgrade. The wire format is frozen per
# schema version, whereas Markdown syntax evolves.
stored = doc.to_json()               # JSON string carrying a schema version
restored = Document.from_json(stored)
assert restored.to_markdown() == doc.to_markdown()
```

`from_json` raises `ParseError` on malformed JSON or an unknown schema
tag. A DTO-reconstructed document has no parse-time `warnings`.

## Development

```bash
uv venv
uv pip install -e ".[dev]"
uv run pytest
```

## License

Apache-2.0
