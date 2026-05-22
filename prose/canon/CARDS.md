# Composable Cards Architecture

> **Implementation**: `crates/core/src/quill/`
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md)

## Overview

Cards are structured metadata blocks inline within document content. All cards are stored in a single `$cards` array on the plate JSON, discriminated by each card's `$kind` value.

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
        ui:
          group: Addressing
        description: Name, grade, and duty title.
```

`ui.title` is the display label for UI consumers (section headers, chips, picker entries, per-instance list titles). It may be a literal string or a template containing `{field_name}` tokens that consumers interpolate with live field values (e.g. `"{from} → {for}"`). It's decoupled from the snake_case map key (`indorsement`), which is the on-the-wire `$kind` discriminator — so authors can rename the label without breaking stored documents.

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
        ui:
          group: Addressing
```

`QuillConfig::schema()` emits the schema (with `ui` and `body` hints retained) and `schema_yaml()` is the YAML wrapper. The output keeps the same `card_kinds.<name>.fields` shape as `Quill.yaml` — only the user-fillable fields, no sentinel discriminator. The `card_kinds` map key (e.g. `indorsement`) is itself the `$kind` discriminator value. The `card_kinds` key is omitted entirely when no named card-kinds are defined. See `SCHEMAS.md` for the full surface.

## Markdown Syntax

A composable card is a `~~~card-yaml` block, optionally led by a
`$kind: <kind>` system-metadata line. The kind surfaces on the plate
JSON as the card's `$kind` discriminator; the block's payload is the
card's YAML data, and the markdown after the closing `~~~` fence is the
card's body.

````markdown
~~~card-yaml
$kind: indorsement
from: ORG1/SYMBOL
for: ORG2/SYMBOL
signature_block:
  - "JOHN DOE, Lt Col, USAF"
  - "Commander"
~~~

Indorsement body content.
````

See [`markdown-spec.md`](../references/markdown-spec.md) §3 for the full syntax specification.

## Backend Consumption

- **All backends**: cards are delivered as `data.$cards`, an array of objects each containing a `$kind` discriminator, the card's metadata fields, and a `$body` key with the card's body Markdown.
- **`Quill::compile_data()`** returns the fully coerced and validated JSON, including `$cards`.

## Out-of-band Metadata (`$ext`)

Per-card editor state — display renames, collapse flags, agent
annotations, anything bespoke to a UI consumer — belongs in the card's
`$ext` system-metadata key, **not** in user fields. `$ext` is an opaque
mapping that round-trips through Markdown and the storage DTO but is
stripped from `Document::to_plate_json()` before backends see it, so
template renders are not affected by editor state. Consumers
namespace inside the map (`$ext.presentation`, `$ext.agent`, …) to avoid
collisions when more than one tool carries state on the same card. See
[markdown-spec.md §3.3](../references/markdown-spec.md) for the full specification.
