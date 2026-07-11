# Programmatic Document Construction

> **Implementation**: `crates/core/src/document/` (the `edit` module), mirrored by every surface in `crates/bindings/`

## TL;DR

A `Document` is built and mutated in memory — no Markdown text involved —
through validated constructors and mutators: `Document::new` (blank canvas),
`Card::new`, `set_field` / `set_fields`, `push_card`. Every mutator enforces
the same field-name, depth, and kind invariants the Markdown parser does, so a
constructed document cannot be invalid. This is the authoring surface for
programs (database row → rendered PDF); Markdown serves human authoring and
the blueprint serves LLM/MCP consumers.

## Three authoring surfaces, one model

| Surface | Consumer | Entry |
|---|---|---|
| card-yaml Markdown | humans | `Document::from_markdown` |
| annotated blueprint | LLMs / MCP | `blueprint()` → fill → `from_markdown` |
| structured mutators | programs | `Document::new` → `set_fields` / `push_card` |

All three produce the same `Document`; render, validation, storage, and
emission do not distinguish how it was built. `to_markdown()` emits canonical
Markdown from any of them, so a programmatically built document round-trips
the same emitter parsed ones do.

## Blank canvas vs seeded starter

`Document::new(quill_ref)` is the blank canvas: a main card with no user
fields, an empty body, and no composable cards. Absent fields resolve at
render time (schema `default`, else type-empty zero — see
[SCHEMAS.md](SCHEMAS.md)), so nothing the program did not set reaches the
output.

`Quill::seed_document()` is the illustration-first starter: `example` values
committed, one card per declared kind — the structured twin of the blueprint
(see [BLUEPRINT.md](BLUEPRINT.md)). Hand it to a human or an editor as
something to edit; start from the blank canvas when the data is authoritative
and example values would pollute it.

## The flow

Python shown; Rust and WASM mirror it method-for-method:

```python
doc = Document("invoice")
doc.set_fields({"customer": row.name, "total": row.total})
for item in row.items:
    doc.push_card(Document.make_card("line_item", {"desc": item.desc, "qty": item.qty}))
result = engine.render(quill, doc, OutputFormat.PDF)
```

Values convert in place at each boundary (Python objects, JS values, Rust
scalars via `Into<QuillValue>`); no surface asks the caller to serialize
YAML or Markdown.

## Validation: batched, atomic, at the boundary

Structural invariants (field-name grammar, value depth, card kind) are
enforced per mutator call. `set_fields` validates its whole batch before
applying any of it: on violation nothing is applied and the single error
carries one diagnostic per offending field with `path` set to the field name —
externally sourced names (database columns, form keys) surface every violation
in one pass. Schema validation (types, enums, constraints) is a separate,
also-batched pass at `Quill::validate` / render.

## Addressing cards for re-render

Card mutators address by index. For patch-and-re-render automation (a source
row changed, re-render the document), stamp `$id` at build time and resolve
the index when patching:

```python
idx = next(i for i, c in enumerate(doc.cards) if c["id"] == row_id)
doc.set_card_fields(idx, {"qty": new_qty})
```

`$id` is optional and opaque; the model imposes no uniqueness on it.

## Links

- [SCHEMAS.md](SCHEMAS.md) — schema model and the zero-filled render projection
- [BLUEPRINT.md](BLUEPRINT.md) — the LLM/MCP authoring surface
- [CARDS.md](CARDS.md) — `$seed` overlays for editor-spawned cards
- [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) — persisting built documents
- [BINDINGS.md](BINDINGS.md) — the language surfaces that mirror this API
