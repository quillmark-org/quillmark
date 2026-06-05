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

markdown = """~~~
$quill: my_quill
$kind: main
title: Hello World
~~~

# Hello
"""

parsed = Document.from_markdown(markdown)
result = quill.render(parsed, OutputFormat.PDF)
result.artifacts[0].save("output.pdf")
```

## API surface

The Python surface mirrors the [`@quillmark/wasm`](../wasm) package. Names
follow `snake_case` conventions; the underlying concepts (and shapes of
return values) are the same.

### `Quillmark`

```python
engine = Quillmark()
engine.registered_backends()      # ['typst']
quill = engine.quill_from_path("path/to/quill")
```

### `Quill`

```python
quill.backend_id            # "typst"
quill.supports_canvas       # True iff the backend supports canvas preview
quill.blueprint             # auto-generated annotated Markdown blueprint
quill.schema                # structured dict of the quill's document schema
quill.metadata              # identity snapshot of the quill: section
quill.supported_formats     # [OutputFormat.PDF, ...]
quill.quill_ref             # "name@version"

result  = quill.render(parsed, OutputFormat.PDF)          # ppi=, pages= optional
session = quill.open(parsed)
diags   = quill.validate(parsed)                         # list of validation::* diagnostic dicts ([] = valid)
seed    = quill.seed_document()                          # starter Document seeded from `example:` values
main    = quill.seed_main()                              # just the $kind: main card (dict, like doc.main)
card    = quill.seed_card("note")                        # one starter composable card (dict), None if kind undeclared
```

### `RenderSession`

```python
session = quill.open(parsed)
session.page_count
session.backend_id
session.supports_canvas
session.warnings
session.render(OutputFormat.SVG, pages=[0])
```

### `RenderResult` / `Artifact`

```python
result.artifacts            # [Artifact, ...]
result.warnings             # [Diagnostic, ...]
result.format               # OutputFormat
result.render_time_ms       # float

artifact.format             # OutputFormat
artifact.bytes              # bytes
artifact.mime_type          # 'application/pdf', 'image/svg+xml', ...
artifact.save("out.pdf")
```

### `Document`

```python
doc = Document.from_markdown(markdown)
emitted = doc.to_markdown()

stored   = doc.to_json()
restored = Document.from_json(stored)
maybe    = Document.try_from_json(blob)          # None when not a DTO

Document.schema_version_of(blob)                 # raw tag (incl. unknown futures)
Document.current_schema_version()                # what this build writes

doc.clone()
doc.equals(other)
doc.card_count
doc.main; doc.cards; doc.body; doc.warnings

doc.set_field("title", "New")
doc.push_card(Document.make_card("note", {"x": 1}, "..."))  # or pass a Card from cards/remove_card/seed_card
# insert_card, remove_card, move_card, set_card_kind,
# update_card_field, remove_card_field, update_card_body, ...
```

## Schema model

A field's *cell* is inferred from whether the schema declares a `default:`:

- **Unendorsed** (no `default:`) — the blueprint renders `<must-fill>`.
  An absent Unendorsed field is a non-fatal signal
  (`validation::field_absent`) — the render path zero-fills it
  silently. A surviving `<must-fill>` sentinel is fatal
  (`validation::must_fill_sentinel`). Partial documents are
  first-class; `quill.render()` only raises for malformed input.
- **Endorsed** (with `default:`) — the blueprint renders the default
  value with a `; delete-ok` annotation, and the default is used when
  the document omits the field.

There is no `required:` axis on `FieldSchema`.

## Error contract

A single exception type — `QuillmarkError` — is raised for every failure
mode. Every raised exception carries a non-empty `.diagnostics` list of
`Diagnostic` objects. This matches the WASM binding's contract.

```python
try:
    Document.from_markdown(bad_md)
except QuillmarkError as exc:
    for d in exc.diagnostics:
        print(d.severity, d.code, d.message, d.path)
```

`EditError`-shaped failures (invalid field names, kind names, out-of-range
indices) prefix the message with `[EditError::<Variant>]` — the same format
WASM uses — so callers can pattern-match on the message when they need to.

## Changelog

See the [changelog](https://github.com/quillmark-org/quillmark/blob/main/CHANGELOG.md)
and the [GitHub Releases](https://github.com/quillmark-org/quillmark/releases) page for
release notes and version history.

## Development

```bash
uv venv
uv pip install -e ".[dev]"
uv run pytest
```

## License

Apache-2.0
