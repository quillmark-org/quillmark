# Composable Leaves Architecture

> **Status**: Implemented
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md), [MARKDOWN.md](MARKDOWN.md), [LEAF_REWORK.md](LEAF_REWORK.md)

## Overview

Leaves are structured metadata records inline within document content,
encoded as CommonMark fenced code blocks with the info string `leaf`. All
leaves are stored in a single `LEAVES` array, discriminated by the `KIND`
field. See [MARKDOWN.md](MARKDOWN.md) §3.2 for the syntax-level
specification.

## Data Model

```rust
pub struct LeafSchema {
    pub name: String,
    pub description: Option<String>,
    pub fields: HashMap<String, FieldSchema>,
    pub ui: Option<UiLeafSchema>,
    pub body: Option<BodyLeafSchema>,
}
```

The static display label for a leaf kind lives on `UiLeafSchema::title`,
not on `LeafSchema` directly — see `ui.title` below. Body behavior (whether
body content is permitted and optional guide text) lives under `body` —
see `body.enabled` and `body.description` below.

`QuillConfig` exposes the entry-point document schema as `main: LeafSchema`
and the additional named leaf kinds as `leaf_kinds: Vec<LeafSchema>`. Look
up a named kind by name via `leaf_kind(name)`, or iterate `leaf_kinds`
directly for the full list.

## Quill.yaml Configuration

```yaml
main:
  fields:
    # ... main-document fields ...

leaf_kinds:
  indorsement:
    description: Chain of routing endorsements for multi-level correspondence.
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
        description: Office symbol of the endorsing official.
      for:
        type: string
        description: Office symbol receiving the endorsed memo.
      signature_block:
        type: array
        required: true
        ui:
          group: Addressing
        description: Name, grade, and duty title.
```

`ui.title` is the display label for UI consumers (section headers, chips,
picker entries, per-instance list titles). It may be a literal string or a
template containing `{field_name}` tokens that consumers interpolate with
live field values (e.g. `"{from} → {for}"`). It's decoupled from the
snake_case map key (`indorsement`), which is the on-the-wire `KIND`
discriminator — so authors can rename the label without breaking stored
documents.

## Public Schema YAML Output

```yaml
leaf_kinds:
  indorsement:
    description: Chain of routing endorsements for multi-level correspondence.
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
      for:
        type: string
      signature_block:
        type: array
        required: true
        ui:
          group: Addressing
```

`QuillConfig::schema()` emits the schema (with `ui` and `body` hints
retained) and `schema_yaml()` is the YAML wrapper. The output keeps the
same `leaf_kinds.<name>.fields` shape as `Quill.yaml` and injects a
required `KIND` sentinel field whose `const` value is the leaf kind name.
The `leaf_kinds` key is omitted entirely when no named kinds are defined.
See `SCHEMAS.md` for the full surface.

## Markdown Syntax

````markdown
```leaf
KIND: indorsement
from: ORG1/SYMBOL
for: ORG2/SYMBOL
signature_block:
  - "JOHN DOE, Lt Col, USAF"
  - "Commander"
```

Indorsement body content.
````

## Backend Consumption

- **All backends**: leaves are delivered as `data.LEAVES`, an array of
  objects each containing a `KIND` discriminator field, the leaf's
  metadata fields, and a `BODY` field with the leaf's body Markdown.
- **`Quill::compile_data()`** returns the fully coerced and validated
  JSON, including `LEAVES`.
