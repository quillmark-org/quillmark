# Quillmark — Python bindings

Python bindings for Quillmark, a schema-driven document engine.

Maintained by [TTQ](https://tonguetoquill.com).

## Installation

```bash
pip install quillmark
```

## Quick Start

```python
from quillmark import Quillmark, Quill, Document, OutputFormat

engine = Quillmark()                       # backend registry + render dispatcher
quill = Quill.from_path("path/to/quill")   # portable, declarative config data

markdown = """~~~
$quill: my_quill
$kind: main
title: Hello World
~~~

# Hello
"""

parsed = Document.from_markdown(markdown)
result = engine.render(quill, parsed, OutputFormat.PDF)
result.artifacts[0].save("output.pdf")
```

## API surface

The Python surface mirrors the [`@quillmark/wasm`](../wasm) package for the
shared document model. Names follow `snake_case` conventions; the underlying
concepts (and shapes of return values) are the same. Python renders in one
shot via `engine.render`; the iterative render-session and canvas-preview
surface is WASM-only (see `prose/canon/PREVIEW.md`).

**Capability principle:** a `Quill` is portable, declarative config data —
`quill.metadata` is a pure, infallible snapshot of the `quill:` section.
The format probe (`supported_formats`) and rendering (`render`) are resolved
by the engine, against a quill; they raise `QuillmarkError`
(`UnsupportedBackend`) only if the declared backend isn't registered.

### `Quillmark`

```python
engine = Quillmark()
engine.registered_backends()              # ['typst']
engine.render(quill, parsed, OutputFormat.PDF)   # ppi=, pages=, producer= optional
engine.supported_formats(quill)           # [OutputFormat.PDF, ...] (raises if backend unregistered)
```

### `Quill`

```python
quill = Quill.from_path("path/to/quill")  # pure config load — no backend resolved here

quill.backend_id            # "typst" (declared backend)
quill.blueprint             # auto-generated annotated Markdown blueprint
quill.schema                # structured dict of the quill's document schema
quill.metadata              # pure config snapshot of the quill: section (never raises)
quill.quill_ref             # "name@version"

diags   = quill.validate(parsed)          # list of validation::* diagnostic dicts ([] = valid)
seed    = quill.seed_document()           # starter Document seeded from `example:` values
main    = quill.seed_main()               # just the $kind: main card (dict, like doc.main)
card    = quill.seed_card("note")         # one starter composable card (dict), None if kind undeclared
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
doc = Document("my_quill")                       # blank canvas: $quill only, no fields, no cards
doc = Document.from_markdown(markdown)
emitted = doc.to_markdown()

stored   = doc.to_json()
restored = Document.from_json(stored)
maybe    = Document.try_from_json(blob)          # None when not a DTO

Document.schema_version_of(blob)                 # raw tag (incl. unknown futures)
Document.current_schema_version()                # what this build writes

Document.format_rules()                          # card-yaml authoring rules (static text)
Document.quill_ref_hint()                        # $quill reference grammar (static text)
Document.blueprint_instruction("taro")           # LLM/MCP blueprint header for a quill

doc.clone()
doc.equals(other)
doc.card_count
doc.main; doc.cards; doc.body; doc.warnings

doc.set_field("title", "New")
doc.set_fields({"title": "New", "author": "A"})  # atomic batch; one diagnostic per bad field
doc.push_card(Document.make_card("note", {"x": 1}, "..."))  # or pass a Card from cards/remove_card/seed_card
# insert_card, remove_card, move_card, set_card_kind,
# update_card_field, update_card_fields, remove_card_field, update_card_body, ...
```

## Schema model

A field's *cell* is inferred from whether the schema declares a `default:`:

- **Unendorsed** (no `default:`) — the blueprint renders the `!must_fill`
  marker (carrying the field's `example` as a suggested value when one
  exists). An absent Unendorsed field zero-fills silently. A `!must_fill`
  marker left in the document is non-fatal: it emits the
  `validation::must_fill` warning and still renders. Partial documents are
  accepted; `engine.render(quill, doc)` only raises for malformed input.
- **Endorsed** (with `default:`) — the blueprint renders the default
  value with a type-only `# <type>` annotation (shippable as-is), and the
  default is used when the document omits the field.

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
        print(str(d))   # canonical pretty-printed text (matches CLI / WASM)
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
