# Blueprint Emission (`QuillConfig::blueprint`)

> **Implementation**: `crates/core/src/quill/`

## TL;DR

`blueprint()` produces an annotated Markdown document — the same shape an
author would write — pre-filled with placeholders, examples, and
constraint hints. It is the **authoring surface** for LLM and MCP
consumers; [SCHEMAS.md](SCHEMAS.md) covers the validation/form surface.

A blueprint is the document, not a description of the document. Fill in
the placeholders; the structure, `$` metadata, and body markers come for
free.

## Output shape

````
~~~card-yaml
$quill: <name>@<version>
$kind: main
# <description>

# <field description>
# e.g. <example value>
field: value  # <type>[; skip-ok]
~~~

Write main body here.

~~~card-yaml
$kind: <card_kind>
# composable (0..N)
# <card description>
...fields...
~~~

Write <card_kind> body here.
````

Every block is a `~~~card-yaml` block (see [MARKDOWN.md](MARKDOWN.md) §3):
the root block carries the `$quill` system-metadata line; each composable
card carries a `$kind: <card_kind>` metadata line.

When `body.example` is set, its text replaces the body marker entirely.
When `body.enabled` is false the marker is omitted entirely.

## Annotation grammar

| Slot | Form | Carries |
|---|---|---|
| **Leading `# …` lines** above a field | `# <prose>` or `# e.g. <value>` | description (single-line prose) and an illustrative example |
| **Inline `# …`** at end of the value line | `# <type>[<format>][; skip-ok]` | structural metadata: type, optional format refinement, optional skip-ok tag |

The two slots have disjoint purposes: leading is prose, inline is
structural. No colon-separated `key: value` annotation syntax appears in
either slot, so neither pattern collides with YAML key/value parsing.

### Leading lines — order

Per field, in order:

1. `# <description>` — `description:` from `Quill.yaml`,
   whitespace-collapsed. **Single line only**; multi-line descriptions are
   rejected at `Quill.yaml` parse time.
2. `# e.g. <value>` — emitted whenever `example:` is configured on a
   field. Independent of cell and type. The example never becomes the
   rendered value.

That's it. There is no leading `# required`, `# enum:`, `# default:`, or
`# type:` — those collapse into the inline.

### Inline annotation

Form: **`# <type>[<format>][; skip-ok]`**

- **Type slot** (mandatory, first): one of
  `string`, `integer`, `number`, `boolean`, `array`, `object`,
  `markdown`, `date`, `datetime`, `enum`.
  Every field is labeled — there is no "self-evident" exemption.
  (`object` requires a `properties` map; freeform untyped objects are not
  supported. `object` also appears in the format slot of typed-table fields
  as `array<object>`.)
- **Format slot** (optional, in `<…>` angle brackets): refines the type
  when the refinement carries information beyond the type name itself.
  - `date<YYYY-MM-DD>`
  - `datetime<ISO 8601>`
  - `array<string>`, `array<integer>`, `array<object>`, …
  - `enum<a | b | c>`
  - omitted for `string`, `integer`, `number`, `boolean`, `object`,
    `markdown` (nothing meaningful to refine).
- **Skip-ok tag** (optional, after `;`): the single tag `skip-ok`. Present
  on Endorsed fields (fields with a `default:` in the schema), signalling
  "the rendered value is shippable as-is — keep or override". Absent on
  Must Fill fields (fields without a `default:`), which carry the
  `<must-fill>` sentinel in the value cell instead.

The `$`-prefixed system-metadata keys (`$quill`, `$kind`, …) have no
inline-annotation slot — they are not user-defined data fields. (The YAML
parser accepts a trailing ` # comment` on a `$` line, but the blueprint
emitter does not attach one, and the canonical form drops every comment
attached to a `$` line.) The root block's `$quill` line is emitted
verbatim; its value is fixed and must not be modified. A composable
card's kind is carried in its `$kind: <card_kind>` metadata line. Its
`composable (0..N)` role is emitted as an own-line `# composable (0..N)`
comment directly under the `$kind` line, ahead of the card description.

Examples:

| Line | Reading |
|---|---|
| `name: <must-fill>  # string` | Must Fill string — replace `<must-fill>` before shipping |
| `title: "Curriculum Vitae"  # string; skip-ok` | Endorsed string — keep or override |
| `count: 0  # integer; skip-ok` | Endorsed integer (type-empty default, explicitly shippable) |
| `active: false  # boolean; skip-ok` | Endorsed boolean (type-empty default, explicitly shippable) |
| `notes: ""  # string; skip-ok` | Endorsed empty string (the "skippable" cell, now Endorsed) |
| `bio: |-` followed by indented `<must-fill>`, then `# markdown` | Must Fill markdown — see "Markdown fields render as block scalars" |
| `recipient: <must-fill>  # array<string>` | Must Fill array of strings |
| `date: <must-fill>  # date<YYYY-MM-DD>` | Must Fill date in `YYYY-MM-DD` format |
| `severity: <must-fill>  # enum<low \| medium \| high>` | Must Fill enum |
| `$quill: cmu_letter@0.1.0` | quill binding metadata, emitted verbatim, do not modify |
| `$kind: skill` followed by `# composable (0..N)` | repeat the entire `~~~card-yaml` … `~~~` block per instance |

## Placeholder value precedence

The rendered value follows a single cascade keyed on the cell:

| Field state | Value rendered | Cell |
|---|---|---|
| Has `default` | default | Endorsed (carries `; skip-ok`) |
| No `default` | `<must-fill>` sentinel | Must Fill (no `; skip-ok`) |

Examples never become the rendered value, regardless of cell or type —
this holds uniformly for scalars, arrays, typed tables, and typed
dictionaries. Examples are inherently illustrative and unsafe to ship;
they always surface in the `# e.g.` leading line while the value follows
the cascade above.

All fields render as **live YAML** — no commented-out fields. The
sentinel in the value cell is the sole "must fill" signal: a reader's
mental model is one rule — **`<must-fill>` in the value cell → replace
before shipping; otherwise the value cell is shippable as-is**.

The sentinel lives where the LLM types the value:

| Type | Sentinel position | Example |
|---|---|---|
| `string`, `integer`, `number`, `boolean`, `date`, `datetime`, `enum` | Value cell | `name: <must-fill>  # string` |
| `array<scalar>` | Value cell | `recipient: <must-fill>  # array<string>` |
| `markdown` | Inside the block scalar | `bio: |-` then `<must-fill>` |
| `object` (typed dict) | Per-property recursion | leaves carry sentinels |
| `array<object>` (typed table) | Per-property recursion in one synthetic row | leaves carry sentinels |

### Markdown fields render as block scalars

A `markdown` field renders as a YAML literal block scalar (`|-`). The
block-scalar shape is type-driven — it's the only YAML form that
cleanly accommodates multi-line markdown content. By rendering it from
the start, the LLM consumer writes into the indented block without
needing to switch shapes mid-fill.

For a Must Fill markdown field, the block's content is exactly one line
containing the sentinel:

```
bio: |-  # markdown
  <must-fill>
```

The LLM replaces that single line with multi-line markdown; the block
scalar's shape is unchanged.

When a `default:` is configured, the field is Endorsed and the default's
content fills the block:

```
bio: |-  # markdown; skip-ok
  ## About me
  
  <body>
```

If the default is empty (`default: ""`), the block scalar still carries
the `; skip-ok` tag and renders with one indented blank line — the
"skippable" markdown cell.

### Multi-element example arrays

Examples on array fields render as a YAML flow sequence so
multi-element shape information is preserved:

```
# e.g. [Mr. John Doe, 123 Main St, "Anytown, USA"]
recipient: <must-fill>  # array<string>
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

A field of `type: array` with a `properties` map renders with full
per-property annotations:

- A non-empty `default:` renders as actual rows (no per-property
  annotations on each row). The outer key carries `# array<object>; skip-ok`.
- Otherwise one synthetic row is emitted with each property carrying its
  own description, inline annotation, and cell signal (sentinel or
  `; skip-ok`). The outer key carries `# array<object>` (no `; skip-ok`).

An `example:` never renders as rows. Like every other field type, it
surfaces only in the `# e.g.` leading line — as a one-line flow
sequence, e.g. `# e.g. [{org: ACME, year: 2020}]`.

## Typed dictionaries

A field of `type: object` with a `properties` map renders as an indented
block mapping with per-property annotations — no outer value on the key
line:

- A non-empty `default:` renders as a concrete block mapping (property
  values only, no annotations). The outer key carries
  `# object; skip-ok`.
- Otherwise each property is emitted with its own description, inline
  annotation, and cell signal — the same rules as any other field. The
  outer key carries `# object` (no `; skip-ok`); state is a leaf concern.

An `example:` never renders as a concrete mapping. Like every other
field type, it surfaces only in the `# e.g.` leading line — as a
one-line flow mapping, e.g. `# e.g. {street: 1 Infinite Loop, city:
Cupertino}`.

```
# The sender's mailing address.
address:  # object
  # Street address line.
  street: <must-fill>  # string
  # City name.
  city: <must-fill>  # string
  # ZIP or postal code.
  zip: ""  # string; skip-ok
```

With a default:

```
address:  # object; skip-ok
  street: 5000 Forbes Avenue
  city: Pittsburgh
  zip: "15213"
```

Properties of a typed dictionary may not themselves be objects (nesting
beyond one level is not supported). The same rule applies to typed table
properties. Freeform `type: object` fields without a `properties` map are
rejected at `Quill.yaml` parse time (`quill::object_missing_properties`).

## UI metadata honored

`ui.order` controls field ordering within the document. Most other `ui:`
keys (`ui.group`, `ui.compact`, `ui.multiline`, `ui.title`) are
presentation-only and do not affect blueprint output. In particular,
`ui.group` no longer emits `# ==== GROUPNAME ====` banner lines — the
banners were visually confusable with field-description comments. Fields
within the same `ui.group` still cluster together via `ui.order`.

## Body markers

- `Write main body here.` after the root block's closing `~~~`
- `Write <card_kind> body here.` after each card block's closing `~~~`
- When `body.example` is set, its text replaces the marker verbatim.

`body.enabled: false` suppresses the marker entirely for body-less cards
(e.g., a `skills` card whose data is purely structured).

A `body.example` whose text contains a line that would parse as a
`~~~card-yaml` opener is rejected at `Quill.yaml` parse time
(`quill::body_example_contains_fence`) to prevent corrupting the
blueprint's document structure.

## Worked example

```
~~~card-yaml
$quill: cmu_letter@0.1.0
$kind: main
# Typeset letters that comply with Carnegie Mellon University letterhead standards.

# The recipient's name and full mailing address.
# e.g. [Mr. John Doe, 123 Main St, "Anytown, USA"]
recipient: <must-fill>  # array<string>
# The signer's information. Line 1: Name. Line 2: Title.
# e.g. [First M. Last, Title]
signature_block: <must-fill>  # array<string>
# The department or organizational unit name for the letterhead.
# e.g. Department of Electrical and Computer Engineering
department: ""  # string; skip-ok
# The sender's institutional mailing address.
# e.g. [5000 Forbes Avenue, "Pittsburgh, PA 15213-3890"]
address: <must-fill>  # array<string>
# The department or university website URL.
# e.g. www.ece.cmu.edu
url: ""  # string; skip-ok
# The date to appear on the letter.
date: <must-fill>  # date<YYYY-MM-DD>
~~~

Write main body here.
```

## Guarantees

`blueprint()` guarantees the emitted document is **parseable**: every
field key is present, every value is YAML-valid, the document round-trips
through `Document::from_markdown` and back. Endorsed cells coerce and
validate successfully; Must Fill cells carry the `<must-fill>` sentinel
in the value cell (or inside a markdown block scalar), which validation
reports as `validation::unfilled_placeholder` until the LLM replaces it
with a typed value.

`blueprint()` does **not**, on its own, guarantee the document
*renders*. Rendering depends on the quill's `plate.typ` and its
packages, which `blueprint()` does not control. That is a separate
**quill authoring contract**:

> A quill's `plate.typ` MUST render the quill's own blueprint — with all
> `<must-fill>` cells replaced by type-empty values of the declared
> type — to a successful (non-error) output.

The blueprint, after sentinel substitution, is by construction the
*type-minimal valid input* — the worst-case-but-valid document. A plate
that renders it has shown it degrades gracefully on every type-valid
input shape. The contract requires:

- Templates treat type-empty values (`""`, `0`, `false`, `[]`, empty
  markdown body) as valid *present* input — read via `data.field`,
  `card.at("field", default: …)`, or guarded with `if "field" in data`.
- No template asserts that a Must Fill field is *non-empty*. The schema
  guarantees *presence*, not non-emptiness; the `<must-fill>` sentinel
  is an authoring signal, not a render-time precondition.
- "Renders successfully" means "compiles without error," not "produces
  meaningful output." An empty-string title is a blank title — that is
  acceptable.

The contract is enforced by a fixture test that fills sentinels with
type-empty values, then renders every bundled quill's blueprint and
asserts success
(`crates/quillmark/tests/quiver_test.rs::every_quill_in_quiver_renders`).

## Bindings surface

| Binding | Accessor |
|---|---|
| Rust | `QuillConfig::blueprint() -> String` |
| Wasm | `Quill.blueprint` getter |
| Python | `Quill.blueprint` property |
| CLI | `quillmark blueprint <QUILL_PATH> [-o <FILE>]` |

The Rust example `cargo run -p quillmark-core --example print_blueprint
-- <quill_name> [<version>]` prints the blueprint for any bundled
fixture.

## Relationship to schema

| Concern | Use |
|---|---|
| Validators, form builders, machine consumers | [SCHEMAS.md](SCHEMAS.md) — `schema()` |
| LLM/MCP authoring, prompt-time reference document | this doc — `blueprint()` |

## Authoring guidance

When designing a `Quill.yaml` schema, choose between Must Fill and
Endorsed per field:

- **Declare `default:`** when a value (including a type-empty value like
  `""`, `[]`, `false`, or `0`) is acceptable to ship as-is. The field
  becomes Endorsed and the blueprint carries `; skip-ok`.
- **Omit `default:`** when the author, LLM, or user must supply a value
  before shipping. The field becomes Must Fill and the blueprint carries
  the `<must-fill>` sentinel.
- **Use `example:`** for illustrative reference values. `example:` is
  orthogonal to the cell decision — it appears in the leading `# e.g.`
  line for any cell, never rendering as the value.

This is documentation, not enforcement.

### Writing the literal string `<must-fill>` as content

The blueprint emitter detects the unquoted form `<must-fill>` as the
sentinel. To write the literal string as a field value, quote it:
`"<must-fill>"`. Exact-string-equality detection treats the unquoted
form as the sentinel and the quoted form as content.
