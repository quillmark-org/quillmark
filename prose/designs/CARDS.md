# Composable Cards Architecture

> **Status**: Implemented
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md)

## Overview

Cards are structured metadata blocks inline within document content. All cards are stored in a single `CARDS` array, discriminated by the `CARD` field.

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

The static display label for a card kind lives on `UiCardSchema::title`, not on `CardSchema` directly — see `ui.title` below. Body behavior (whether body content is permitted and optional guide text) lives under `body` — see `body.enabled` and `body.description` below.

`QuillConfig` exposes the entry-point card as `main: CardSchema` and the additional named card-kinds as `card_kinds: Vec<CardSchema>`. Look up a named card-kind by name via `card_kind(name)` or get a name-keyed map via `card_kinds_map()`.

## Quill.yaml Configuration

```yaml
main:
  fields:
    # ... main-card fields ...

card_kinds:
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

`ui.title` is the display label for UI consumers (section headers, chips, picker entries, per-instance list titles). It may be a literal string or a template containing `{field_name}` tokens that consumers interpolate with live field values (e.g. `"{from} → {for}"`). It's decoupled from the snake_case map key (`indorsement`), which is the on-the-wire `CARD` discriminator — so authors can rename the label without breaking stored documents.

## Public Schema YAML Output

```yaml
card_kinds:
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

`QuillConfig::schema()` emits the schema (with `ui` and `body` hints retained) and `schema_yaml()` is the YAML wrapper. The output keeps the same `card_kinds.<name>.fields` shape as `Quill.yaml` and injects a required `CARD` sentinel field whose `const` value is the card name. The `card_kinds` key is omitted entirely when no named card-kinds are defined. See `SCHEMAS.md` for the full surface.

## Markdown Syntax

The canonical syntax for a composable card is a fenced code block whose info
string is `card <kind>`. The kind is the on-the-wire `CARD` discriminator;
the fenced block's content is the card's YAML data, and the markdown after
the closing fence is the card's body.

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

The legacy `---` metadata-fence syntax (a `CARD:` sentinel inside a
`---`/`---` pair) is still accepted on input:

```markdown
---
CARD: indorsement
from: ORG1/SYMBOL
---

Indorsement body content.
```

Both syntaxes parse to the same card. `Document::to_markdown` only ever
emits the canonical fenced form, so a document authored with legacy fences
round-trips to fenced cards. See [`MARKDOWN.md`](./MARKDOWN.md) §3 for the
full syntax specification.

## Backend Consumption

- **All backends**: cards are delivered as `data.CARDS`, an array of objects each containing a `CARD` discriminator field, the card's metadata fields, and a `BODY` field with the card's body Markdown.
- **`Quill::compile_data()`** returns the fully coerced and validated JSON, including `CARDS`.
