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
#@quill: my_quill
#@kind: main
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
```

## Development

```bash
uv venv
uv pip install -e ".[dev]"
uv run pytest
```

## License

Apache-2.0
