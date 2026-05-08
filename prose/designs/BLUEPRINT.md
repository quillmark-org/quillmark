# Blueprint Emission (`QuillConfig::blueprint`)

## TL;DR

`blueprint()` produces an annotated Markdown document — the same shape an
author would write — pre-filled with placeholders, examples, and
constraint hints. It is the **authoring surface** for LLM and MCP
consumers; [SCHEMAS.md](SCHEMAS.md) covers the validation/form surface.

A blueprint is the document, not a description of the document. Fill in
the placeholders; the structure, sentinels, group banners, and body
markers come for free.

## Output shape

```
---
# <description>
QUILL: <name>@<version>  # sentinel; required

# ==== <GROUP> ====
# <field description>
# required
field: value

---

Write main body here.

---
# <card description>
CARD: <card_name>  # sentinel, composable (0..N)
...fields...
---

Write <card_name> body here.
```

When `body.example` is set, its text replaces the body marker entirely.
When `body.enabled` is false the marker is omitted entirely.

## Annotation grammar

| Slot | Carries |
|---|---|
| **Leading `# …` lines** above a field | Human prose: description, `required`, `enum: a \| b \| c`, `example: <value>` |
| **Inline `# …`** at end of a value line | Structural type/constraint info: `# integer`, `# YYYY-MM-DD`, `# markdown`, `# sentinel`, `# sentinel, composable (0..N)` |

### Leading comments — order

Per field, in order:

1. `# <description>` — `description:` from `Quill.yaml`, whitespace-collapsed
2. `# required` — only when `required: true`
3. `# enum: a | b | c` — when `enum:` is present
4. `# example: <value>` — only for optional, non-enum fields with an example

Required fields skip the `# example:` line because the example is rendered
*as the value*. Enum fields skip it because the first enum value is the
canonical placeholder.

### Inline annotations

- `# number`, `# integer`, `# boolean`, `# markdown`, `# object`,
  `# YYYY-MM-DD`, `# ISO 8601` — emitted only for non-obvious types.
  `string` and `array` are self-evident from the YAML value.
- `# sentinel` on the `QUILL:` line — copy verbatim; the value binds the
  document to a specific quill@version.
- `# sentinel, composable (0..N)` on each `CARD:` line — copy the sentinel
  value verbatim; repeat the entire `--- … --- card body...` block per
  instance.

## Placeholder value precedence

| Field state | Value rendered |
|---|---|
| Required, has `example` | example |
| Required, has `default` only | default |
| Required, neither | type-based placeholder (`"<name>"`, `0`, `false`, `[]`, `""`) |
| Optional, has `default` | default |
| Optional, has `enum` only | first enum value |
| Optional, neither | **commented-out** type-based empty (`# field: ""`, `# field: 0`, …) |

Optional fields' examples surface in the `# example:` comment, never as
the value.

`date` and `datetime` required fields with no example or default always
render `""` (not `"<name>"`); the inline type annotation (`# YYYY-MM-DD` or
`# ISO 8601`) carries the format hint.

### Commented-out optional fields

An optional field with no `default` and no `enum` is commented out so the
author can uncomment what they need:

```
# field_name: ""
```

The leading description and `# example:` comments are still emitted above it.

### Multi-element example arrays

Examples on optional array fields render as a YAML flow sequence so
multi-element shape information is preserved:

```
# example: [Mr. John Doe, 123 Main St, "Anytown, USA"]
recipient: []
```

Items containing flow indicators (`,`, `[`, `]`, `{`, `}`) get quoted so
the flow form round-trips.

## Typed tables

A field of `type: array` whose `items` is a typed object (`type: object`
+ `properties`) renders with full per-property annotations:

- An `example:` or non-empty `default:` renders as actual rows.
- Otherwise one synthetic row is emitted, with each property carrying its
  own description / `# required` / `# enum:` / type annotation.

## UI metadata honored

Most `ui:` keys are stripped, but two structural hints survive:

- `ui.group` — produces `# ==== GROUPNAME ====` banners between sections.
  Group names are uppercased. Ungrouped fields lead (no banner); named
  groups follow in first-appearance order.
- `ui.order` — controls field ordering within a group.

`ui.compact`, `ui.multiline`, `ui.title` are presentation-only and dropped.

## Body markers

- `Write main body here.` after the main fence
- `Write <card_name> body here.` after each card fence
- When `body.example` is set, its text replaces the marker verbatim.

`body.enabled: false` suppresses the marker entirely for body-less cards
(e.g., a `skills` card whose data is purely structured).

A `body.example` whose text contains a line that would parse as a metadata
fence (`---`, with up to three leading spaces) is rejected at `Quill.yaml`
parse time (`quill::body_example_contains_fence`) to prevent corrupting
the blueprint's document structure.

## Bindings surface

| Binding | Accessor |
|---|---|
| Rust | `QuillConfig::blueprint() -> String` |
| Wasm | `Quill.blueprint` getter |
| Python | `Quill.blueprint` property |
| CLI | not yet exposed |

The Rust example `cargo run -p quillmark-core --example print_blueprint
-- <quill_name> [<version>]` prints the blueprint for any bundled
fixture.

## Relationship to schema

| Concern | Use |
|---|---|
| Validators, form builders, machine consumers | [SCHEMAS.md](SCHEMAS.md) — `schema()` |
| LLM/MCP authoring, prompt-time reference document | this doc — `blueprint()` |
