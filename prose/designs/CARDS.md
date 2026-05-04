# Composable Cards Architecture

> **Status**: Implemented
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md)

## Overview

Cards are structured metadata blocks inline within document content. All cards are stored in a single `CARDS` array, discriminated by the `CARD` field.

## Data Model

```rust
pub struct CardSchema {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub fields: HashMap<String, FieldSchema>,
    pub ui: Option<UiContainerSchema>,
}
```

`QuillConfig` exposes the entry-point card as `main: CardSchema` and the additional named card-types as `card_types: Vec<CardSchema>`. Look up a named card-type by name via `card_type(name)` or get a name-keyed map via `card_types_map()`.

## Quill.yaml Configuration

```yaml
main:
  fields:
    # ... main-card fields ...

card_types:
  indorsement:
    title: Routing Indorsement
    description: Chain of routing endorsements for multi-level correspondence.
    fields:
      from:
        title: From office/symbol
        type: string
        description: Office symbol of the endorsing official.
      for:
        title: To office/symbol
        type: string
        description: Office symbol receiving the endorsed memo.
      signature_block:
        title: Signature block lines
        type: array
        required: true
        ui:
          group: Addressing
        description: Name, grade, and duty title.
```

## Public Schema YAML Output

```yaml
card_types:
  indorsement:
    title: Routing Indorsement
    description: Chain of routing endorsements for multi-level correspondence.
    fields:
      from:
        type: string
      for:
        type: string
      signature_block:
        type: array
        required: true
```

The schema is emitted by `QuillConfig::schema()` (clean, no `ui` hints) and `QuillConfig::form_schema()` (with `ui` hints, for form builders), with YAML wrappers `schema_yaml()` and `form_schema_yaml()`. Both keep the same `card_types.<name>.fields` shape as `Quill.yaml` and inject a required `CARD` sentinel field whose `const` value is the card name. The `card_types` key is omitted entirely when no named card-types are defined. See `SCHEMAS.md` for the full surface.

## Markdown Syntax

```markdown
---
CARD: indorsement
from: ORG1/SYMBOL
for: ORG2/SYMBOL
signature_block:
  - "JOHN DOE, Lt Col, USAF"
  - "Commander"
---

Indorsement body content.
```

## Backend Consumption

- **All backends**: cards are delivered as `data.CARDS`, an array of objects each containing a `CARD` discriminator field, the card's metadata fields, and a `BODY` field with the card's body Markdown.
- **`Quill::compile_data()`** returns the fully coerced and validated JSON, including `CARDS`.
