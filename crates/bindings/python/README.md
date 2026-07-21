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

Python is a **Tier-1 binding**: field I/O flows through `quill.writer(doc)` and
`quill.view(doc)`, the schema-bound write/read front doors. `Document` carries
the quill-free surface — parse, storage, structure, `$ext` / `$seed`, and
`remove_field`. There is no opaque field store and no anchor-preserving content
lane (`install` / `revise` / `apply_change` + the `import_markdown` /
`export_markdown` / `rebase` / `map_pos` codec); those are WASM-only by scope,
serving live editors that Python does not target. See
[`prose/canon/BINDINGS.md`](../../../prose/canon/BINDINGS.md).

Names follow `snake_case`; the shared model (the `Document` / `Card` shapes,
`Diagnostic`s, the storage DTO) is identical to the
[`@quillmark/wasm`](../wasm) package's. Python renders in one shot via
`engine.render`; the iterative render-session and canvas-preview surface is
WASM-only (see `prose/canon/PREVIEW.md`).

**Capability principle:** a `Quill` is portable, declarative config data —
`quill.metadata` is a pure, infallible snapshot of the `quill:` section.
The format probe (`supported_formats`) and rendering (`render`) are resolved
by the engine, against a quill; they raise `QuillmarkError`
(code `engine::backend_not_found`) only if the declared backend isn't
registered.

### `Quillmark`

```python
engine = Quillmark()
engine.registered_backends()              # ['typst', 'pdfform'] (order not guaranteed)
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

writer  = quill.writer(doc)               # schema-bound typed write front door
reader  = quill.view(doc)                 # schema-bound interpreted read front door
```

### `Writer` — `quill.writer(doc)`

The typed write front door. Resolves each field's type from the bound quill, so a
name the schema does not declare is a typo (`UnknownField`), not a fallback. Holds
both handles by reference and owns neither — ephemeral by convention: bind, write,
discard.

```python
w = quill.writer(doc)
w.set("title", "On Taro")                 # typed-commit one field (mismatch raises now)
w.set_all({"title": "T", "author": "A"})  # atomic batch; one diagnostic per bad field
w.set_body("A **taro** essay.")           # typed body write (edit semantics)
w.revise_field("bio", "make it **bold**") # typed *and* anchor-preserving richtext write
w.add_card("quotes", {"author": "Basho"}, "…", at=None)  # make + typed commit + insert (at appends/inserts)
w.remove_card(0)
w.card(0).set("author", "Issa")           # a CardWriter: .index, .kind, .set, .set_all, .set_body, .revise_field
```

### `View` — `quill.view(doc)`

The interpreted read front door and the read twin of `Writer`. One `get` reads
each field by its declared type: a richtext field to its markdown projection,
every other type its canonical value verbatim.

```python
v = quill.view(doc)
v.get("bio")                              # richtext → markdown str; scalar → its value; absent → None
                                          # undeclared name raises UnknownField; undecodable content raises FieldRichtextDecode
v.get_body()                              # the main body markdown (quill-free body read)
v.card(0).kind                            # the composable card's $kind
v.card(0).get("author")                   # a card field, interpreted by its $kind schema
v.card(0).get_body()
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
doc.main; doc.cards; doc.body; doc.warnings      # total-read snapshots (dicts); body is a content dict
doc.set_quill_ref("other@1.0")

# Structure (quill-free — a card kind is a name, not a schema fact):
doc.insert_card(Document.make_card("note", {"x": 1}, "..."), at=None)  # at appends/inserts
doc.remove_card(0)                               # returns the Card dict, or None
doc.move_card(2, 0); doc.set_card_kind(0, "summary")
doc.remove_field("title")                        # remove has no lane; card=i targets a composable card

# Out-of-band consumer state (never rendered):
doc.store_ext({"agent": {"pinned": True}})       # whole $ext map; card=i for a composable card
doc.store_ext_namespace("agent", {"n": 1})       # one slot, siblings preserved; card=i too
doc.remove_ext_namespace("agent"); doc.remove_ext()
doc.store_seed_namespace("note", {"tag": "T"})   # per-kind $seed overlay; new cards spawn with it
doc.remove_seed_namespace("note")
```

Setting a field's value is the writer's job (`quill.writer(doc).set(...)`) — a
field write needs the schema, and `Document` is quill-free. Reading a field's
interpreted value is the view's (`quill.view(doc).get(...)`).

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

Mutator failures (invalid field names, kind names, out-of-range indices) carry
a namespaced `edit::*` `code` on `diagnostics[0]` — `edit::invalid_field_name`,
`edit::unknown_field`, `edit::index_out_of_range`, `edit::field_conform`, … —
the same taxonomy WASM uses. Route on `diagnostics[0].code`, never on message
text.

## Changelog

See the [changelog](https://github.com/borb-sh/quillmark/blob/main/CHANGELOG.md)
and the [GitHub Releases](https://github.com/borb-sh/quillmark/releases) page for
release notes and version history.

## Development

```bash
uv venv
uv pip install -e ".[dev]"
uv run pytest
```

## License

Apache-2.0
