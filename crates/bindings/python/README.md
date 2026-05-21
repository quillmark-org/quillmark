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

markdown = """~~~card-yaml
$quill: my_quill
$kind: main
title: Hello World
~~~

# Hello
"""

parsed = Document.from_markdown(markdown)
result = quill.render(parsed, OutputFormat.PDF)
result.artifacts[0].save("output.pdf")

# Round-trip: mutate, emit, re-parse
parsed.set_field("title", "Updated")
emitted = parsed.to_markdown()
reparsed = Document.from_markdown(emitted)
assert reparsed.payload["title"] == "Updated"
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

quill.backend            # "typst"
quill.supports_canvas    # True when the backend supports canvas preview
quill.schema             # structured dict of the quill's document schema
quill.schema_yaml        # same schema rendered as a YAML string
quill.blueprint          # auto-generated annotated Markdown blueprint
```

### `RenderSession`

Created via `quill.open(doc)`; reuses the compiled snapshot across renders.

```python
session = quill.open(parsed)
session.page_count
session.backend_id            # backend that produced this session
session.supports_canvas
session.warnings              # session-level warnings (e.g. version shims)
session.render(OutputFormat.SVG, [0])  # render a page subset
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

# Detect "is this content a stored DTO or raw Markdown?" without exceptions.
doc = Document.try_from_json(blob) or Document.from_markdown(blob)

# Schema-version probing for cross-version migrations.
v = Document.schema_version_of(blob)          # raw tag, even for future versions
current = Document.current_schema_version()   # version this build writes

# Cheap structural equality and cloning.
copy = doc.clone()                            # independent handle
assert copy == doc                            # structural compare; warnings ignored

# O(1) card count.
doc.card_count
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
