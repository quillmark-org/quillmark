# Composable Cards Architecture

> **Implementation**: `crates/core/src/quill/`
> **Related**: [SCHEMAS.md](SCHEMAS.md), [QUILL.md](QUILL.md)

## TL;DR

Cards are structured-data blocks inline within document content. All cards are stored in a single `$cards` array on the plate JSON, discriminated by each card's `$kind` value.

## Data Model

```rust
pub struct CardSchema {
    pub name: String,
    pub description: Option<String>,
    pub fields: BTreeMap<String, FieldSchema>,
    pub ui: Option<UiCardSchema>,
    pub body: Option<BodyCardSchema>,
}
```

The static display label for a card kind lives on `UiCardSchema::title`, not on `CardSchema` directly — see `ui.title` below. Body behavior (whether body content is permitted and optional guide text) lives under `body` — see `body.enabled` and `body.example` below.

`QuillConfig` exposes the entry-point card as `main: CardSchema` and the additional named card-kinds as `card_kinds: Vec<CardSchema>`. Look up a named card-kind by name via `card_kind(name)`.

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
        items:
          type: string
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
        items:
          type: string
        ui:
          group: Addressing
```

`QuillConfig::schema()` emits the schema (with `ui` and `body` hints retained) and `schema_yaml()` is the YAML wrapper. The output keeps the same `card_kinds.<name>.fields` shape as `Quill.yaml` — only the user-fillable fields, no sentinel discriminator. The `card_kinds` map key (e.g. `indorsement`) is itself the `$kind` discriminator value. The `card_kinds` key is omitted entirely when no named card-kinds are defined. See `SCHEMAS.md` for the full surface.

## Markdown Syntax

A composable card is a `~~~` block (`~~~card-yaml` is also accepted as a
non-canonical alias), optionally led by a
`$kind: <kind>` system-metadata line. The kind surfaces on the plate
JSON as the card's `$kind` discriminator; the block's payload is the
card's YAML data, and the markdown after the closing `~~~` fence is the
card's body.

````markdown
~~~
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

- **All backends**: cards are delivered as `data.$cards`, an array of objects each containing a `$kind` discriminator, the card's payload fields, and a `$body` key with the card's body Markdown.
- **`Quill::compile_data()`** returns the fully coerced and validated JSON, including `$cards`.

The sigiled `data.$cards` here is plate JSON — glue delivered to the backend. It is a **different namespace** from the unsigiled `cards` in a `Diagnostic.path` (`cards.<kind>[<index>]`, the document-model anchor — see [ERROR.md](ERROR.md) § "Document-model paths"). The plate key is not renamed off `$cards`; the two namespaces are documented apart.

## Out-of-band Metadata (`$ext`)

Per-card editor state — display renames, collapse flags, agent
annotations, anything bespoke to a UI consumer — belongs in the card's
`$ext` system-metadata key, **not** in user fields. `$ext` is an opaque
mapping that round-trips through Markdown and the storage DTO but is
stripped from `Document::to_plate_json()` before backends see it, so
template renders are not affected by editor state. Consumers
namespace inside the map (`$ext.editor`, `$ext.agent`, …) to avoid
collisions when more than one tool carries state on the same card. See
[markdown-spec.md §3.3](../references/markdown-spec.md) for the full specification.

`$ext.editor.title` is the canonical slot for a per-card display name —
the label an editing surface shows when a user renames one card
instance. It overrides the per-*kind* `ui.title` and, being editor
state, never reaches the backend.

## Per-kind Seed Overlays (`$seed`)

`$seed` is the structural twin of `$ext` — a system-metadata mapping carried on
the **main card only**, round-tripping through Markdown and the storage DTO and
stripped before backends — but the seeding layer *interprets* it. It is
**root-only** like `$quill`: a composable card carrying `$seed` is rejected at
parse and on storage load. It answers
"what does a *new* card of kind K start with in this document": each entry,
keyed by composable card-kind, is a **sparse overlay** of the user fields (plus
an optional reserved `$body` string) a freshly-added card of that kind inherits.

````markdown
~~~
$quill: usaf_memo@0.2.0
$kind: main
$seed:
  indorsement:                 # keyed by card-kind; never "main"
    from: 49 FW/CC
    signature_block:
      - "JANE A. DOE, Col, USAF"
      - "Commander"
~~~
````

`Quill::seed_card(kind, overlay)` layers the overlay over the quill's
schema-`example:` seed, per field `overlay › example › absent`, ordered by
field declaration order (see [SCHEMAS.md](SCHEMAS.md) "Document seeding"). The overlay is
*sparse*: fields it omits keep flowing from the live quill seed, so it tracks
the quill rather than freezing a snapshot. The overlay is read off the main
card's `$seed` map (`Card::seed`, exposed as `card.seed` in the bindings) and
parsed by `SeedOverlay::from_json`; the consumer passes it to `seed_card`
(`quill.seedCard(kind, doc.main.seed?.[kind])`) — a read of the document, never
a mutation of it. There is no dedicated `Document::seed` accessor: `$seed` is
read through the card, exactly like `$ext`.

The main card **carries** `$seed` but is never a *subject* of it: its keys range
over `card_kinds`, and `main ∉ card_kinds` (a `$seed.main` entry is an advisory
unknown-kind). Overlays are validated only on the editor surface
(`Quill::validate`, warning-severity, rooted at `$seed.<kind>[.<field>]`) and
**never gate render** — `compile_data` / `dry_run` ignore `$seed` entirely. A
malformed overlay surfaces enforcement only when a card is actually spawned from
it, as an ordinary card diagnostic on that card.
