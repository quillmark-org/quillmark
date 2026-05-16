# Composable Cards Architecture

> **Status**: Implemented — describes the `card` vocabulary.
> **Design basis**: [CARD_MODEL.md](../proposals/CARD_MODEL.md) defines the unified "card" model this design reflects (this document was formerly `LEAVES.md`).
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md), [MARKDOWN.md](MARKDOWN.md)

## Overview

A document is composed of **cards**. It has exactly one **main card** —
the top-of-document frontmatter — and zero or more **inline cards**.
Inline cards are structured metadata records inline within document
content, encoded as CommonMark fenced code blocks whose info string is
`card <kind>`. All inline cards are stored in a single `CARDS` array,
discriminated by `KIND` — an output-only field the parser populates from
the info-string kind token. See [MARKDOWN.md](MARKDOWN.md) §3.2 for the
syntax-level specification.

## Data Model

```rust
pub struct CardSchema {
    pub name: String,
    pub description: Option<String>,
    pub fields: HashMap<String, FieldSchema>,
    pub ui: Option<UiCardSchema>,
    pub body: Option<BodyCardSchema>,
}
```

The static display label for a card kind lives on `UiCardSchema::title`,
not on `CardSchema` directly — see `ui.title` below. Body behavior (whether
body content is permitted and optional guide text) lives under `body` —
see `body.enabled` and `body.description` below.

`QuillConfig` exposes every card schema through a single `cards` map. The
reserved key `cards.main` is the entry-point document schema (`CardSchema`,
no `KIND`); every other key under `cards` is a named inline card kind
(`CardSchema`) whose key is its `KIND` discriminator. Look up a named kind
by name, or iterate the `cards` map directly for the full list.

## Quill.yaml Configuration

```yaml
cards:
  main:
    fields:
      # ... main-document fields ...

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

`cards` is a single flat namespace of card schemas. The reserved key `main`
is the entry-point card (no `KIND`); every other key is an inline card kind
whose key is its `KIND` discriminator.

`ui.title` is the display label for UI consumers (section headers, chips,
picker entries, per-instance list titles). It may be a literal string or a
template containing `{field_name}` tokens that consumers interpolate with
live field values (e.g. `"{from} → {for}"`). It's decoupled from the
snake_case map key (`indorsement`), which is the on-the-wire `KIND`
discriminator — so authors can rename the label without breaking stored
documents.

## Public Schema YAML Output

```yaml
cards:
  main:
    fields:
      # ... main-document fields ...

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
same `cards.<name>.fields` shape as `Quill.yaml` and injects a
required `KIND` discriminator field whose `const` value is the card kind
name (the kind token authors write in the `` ```card <kind> `` info
string).
See `SCHEMAS.md` for the full surface.

## Markdown Syntax

````markdown
```card indorsement
from: ORG1/SYMBOL
for: ORG2/SYMBOL
signature_block:
  - "JOHN DOE, Lt Col, USAF"
  - "Commander"
```

Indorsement body content.
````

## Backend Consumption

- **All backends**: inline cards are delivered as `data.CARDS`, an array of
  objects each containing a `KIND` discriminator field, the card's
  metadata fields, and a `BODY` field with the card's body Markdown.
- **`Quill::compile_data()`** returns the fully coerced and validated
  JSON, including `CARDS`.
