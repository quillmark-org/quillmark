# Blueprint Emission (`QuillConfig::blueprint`)

## TL;DR

`blueprint()` produces an annotated Markdown document — the same shape an
author would write — pre-filled with placeholders, examples, and
constraint hints. It is the **authoring surface** for LLM and MCP
consumers; [SCHEMAS.md](SCHEMAS.md) covers the validation/form surface.

A blueprint is the document, not a description of the document. Fill in
the placeholders; the structure, sentinels, and body markers come for
free.

## Output shape

```
---
# <description>
QUILL: <name>@<version>  # sentinel; required, verbatim

# <field description>
# e.g. <example value>
field: value  # <type>; <role>

---

Write main body here.

---
# <card description>
CARD: <card_name>  # sentinel; composable (0..N)
...fields...
---

Write <card_name> body here.
```

When `body.example` is set, its text replaces the body marker entirely.
When `body.enabled` is false the marker is omitted entirely.

## Annotation grammar

| Slot | Form | Carries |
|---|---|---|
| **Leading `# …` lines** above a field | `# <prose>` or `# e.g. <value>` | description (single-line prose) and an illustrative example |
| **Inline `# …`** at end of the value line | `# <type>[<format>]; <role>[, <extra>...]` | structural metadata: type, optional format refinement, role, optional extras |

The two slots have disjoint purposes: leading is prose, inline is
structural. No colon-separated `key: value` annotation syntax appears in
either slot, so neither pattern collides with YAML key/value parsing.

### Leading lines — order

Per field, in order:

1. `# <description>` — `description:` from `Quill.yaml`,
   whitespace-collapsed. **Single line only**; multi-line descriptions are
   rejected at `Quill.yaml` parse time.
2. `# e.g. <value>` — emitted whenever `example:` is configured on a
   field. Independent of role and type. The example never becomes the
   rendered value (see precedence below).

That's it. There is no leading `# required`, `# enum:`, `# default:`, or
`# type:` — those collapse into the inline.

### Inline annotation

Form: **`# <type>[<format>]; <role>[, <extra>...]`**

- **Type slot** (mandatory, first): one of
  `string`, `integer`, `number`, `boolean`, `array`,
  `markdown`, `date`, `datetime`, `enum`, `sentinel`.
  Every field is labeled — there is no "self-evident" exemption.
  (`object` appears only in the format slot of typed-table fields as
  `array<object>`; standalone `object` fields are not supported.)
- **Format slot** (optional, in `<…>` angle brackets): refines the type
  when the refinement carries information beyond the type name itself.
  - `date<YYYY-MM-DD>`
  - `datetime<ISO 8601>`
  - `array<string>`, `array<integer>`, `array<object>`, …
  - `enum<a | b | c>`
  - omitted for `string`, `integer`, `number`, `boolean`, `object`,
    `markdown` (nothing meaningful to refine).
- **Role slot** (mandatory, after `;`): `required`, `optional`, or
  `composable (0..N)` (CARD-sentinel only).
- **Extras** (optional, comma-separated, after the role): additional
  qualifiers. Currently used for `verbatim` on the QUILL sentinel,
  signaling that the rendered value is fixed and must not be modified.

Examples:

| Line | Reading |
|---|---|
| `name: ""  # string; required` | required string field, no format refinement |
| `count: 0  # integer; required` | required integer field |
| `active: false  # boolean; optional` | optional boolean field |
| `bio: |-` followed by indented block, then `# markdown; optional` | optional markdown field — see "Markdown fields render as block scalars" |
| `recipient: []  # array<string>; optional` | optional array of strings |
| `entries: [...]  # array<object>; required` | required array of objects (typed table follows) |
| `date: ""  # date<YYYY-MM-DD>; required` | required date in `YYYY-MM-DD` format |
| `published: ""  # datetime<ISO 8601>; required` | required datetime in ISO 8601 |
| `level: low  # enum<low \| medium \| high>; optional` | optional enum, default is first value |
| `QUILL: cmu_letter@0.1.0  # sentinel; required, verbatim` | quill binding, do not modify |
| `CARD: skill  # sentinel; composable (0..N)` | repeat the entire `--- CARD ... ---` block per instance |

## Placeholder value precedence

The rendered value is independent of the role (required vs. optional).
Role drives "must fill"; the value rendering follows a single cascade:

| Field state | Value rendered |
|---|---|
| Has `default` | default |
| Has `enum` only | first enum value |
| Otherwise | type-empty (`""`, `0`, `false`, `[]`, or block scalar for markdown — see below) |

Examples never become the rendered value, regardless of role. Examples
are inherently illustrative and unsafe to ship; they always surface in
the `# e.g.` leading line while the value follows the cascade above.

All fields render as **live YAML** — no commented-out fields. The role
tag (`; required` / `; optional`) is the sole "must fill" signal. The
rendered value is the effective default: what the field will be if the
author leaves it untouched.

`date` and `datetime` fields render `""` when no example or default is
configured; the inline format slot (`<YYYY-MM-DD>` or `<ISO 8601>`)
carries the shape hint.

There is no string `"<name>"` placeholder — required strings render as
`""` like every other type.

### Markdown fields render as block scalars

A `markdown` field renders as a YAML literal block scalar (`|-`), even
when the type-empty case applies:

```
bio: |-
  
```

When a `default:` is configured, its content fills the block:

```
bio: |-
  ## About me
  
  <body>
```

The block-scalar shape is type-driven — it's the only YAML form that
cleanly accommodates multi-line markdown content. By rendering it from
the start, the LLM consumer writes into the indented block without
needing to switch shapes mid-fill.

### Multi-element example arrays

Examples on array fields render as a YAML flow sequence so
multi-element shape information is preserved:

```
# e.g. [Mr. John Doe, 123 Main St, "Anytown, USA"]
recipient: []  # array<string>; optional
```

Items containing flow indicators (`,`, `[`, `]`, `{`, `}`) get quoted so
the flow form round-trips.

### Reserved characters in format and enum literals

To keep the inline grammar unambiguous, format slot contents — including
enum values — may not contain `>`, `;`, or `|`. These are the closing
delimiter, the role separator, and the enum-value separator respectively.
`Quill.yaml` parsing rejects offending values with
`quill::format_literal_reserved_char`. There is no escape or quoting
fallback; authors needing these characters must reshape their values.

## Typed tables

A field of `type: array` whose `items` is a typed object (`type: object`
+ `properties`) renders with full per-property annotations:

- An `example:` or non-empty `default:` renders as actual rows.
- Otherwise one synthetic row is emitted, with each property carrying
  its own description, inline type/format, and role.

The outer field's inline annotation is `# array<object>; <role>`.

## UI metadata honored

`ui.order` controls field ordering within the document. Most other `ui:`
keys (`ui.group`, `ui.compact`, `ui.multiline`, `ui.title`) are
presentation-only and do not affect blueprint output. In particular,
`ui.group` no longer emits `# ==== GROUPNAME ====` banner lines — the
banners were visually confusable with field-description comments. Fields
within the same `ui.group` still cluster together via `ui.order`.

## Body markers

- `Write main body here.` after the main fence
- `Write <card_name> body here.` after each card fence
- When `body.example` is set, its text replaces the marker verbatim.

`body.enabled: false` suppresses the marker entirely for body-less cards
(e.g., a `skills` card whose data is purely structured).

A `body.example` whose text contains a line that would parse as a
metadata fence (`---`, with up to three leading spaces) is rejected at
`Quill.yaml` parse time (`quill::body_example_contains_fence`) to
prevent corrupting the blueprint's document structure.

## Worked example

```
---
# Typeset letters that comply with Carnegie Mellon University letterhead standards.
QUILL: cmu_letter@0.1.0  # sentinel; required, verbatim
# The recipient's name and full mailing address.
# e.g. [Mr. John Doe, 123 Main St, "Anytown, USA"]
recipient: []  # array<string>; optional
# The signer's information. Line 1: Name. Line 2: Title.
# e.g. [First M. Last, Title]
signature_block: []  # array<string>; optional
# The department or organizational unit name for the letterhead.
# e.g. Department of Electrical and Computer Engineering
department: ""  # string; optional
# The sender's institutional mailing address.
# e.g. [5000 Forbes Avenue, "Pittsburgh, PA 15213-3890"]
address: []  # array<string>; optional
# The department or university website URL.
# e.g. www.ece.cmu.edu
url: ""  # string; optional
# The date to appear on the letter.
date: ""  # date<YYYY-MM-DD>; optional
---

Write main body here.
```

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
