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
~~~
$quill: <name>@<version>
$kind: main
# <description>

# <field description>
# e.g. <example value>
field: value  # <type>[; delete-ok]
~~~

Write main body here.

~~~
$kind: <card_kind>
# composable (0..N)
# <card description>
...fields...
~~~

Write <card_kind> body here.
````

Every block is a bare `~~~` block (the canonical card-yaml fence; the legacy
`~~~card-yaml` opener is still accepted as an alias — see
[markdown-spec.md](../references/markdown-spec.md) §3): the root block carries
the `$quill` system-metadata line; each composable card carries a
`$kind: <card_kind>` metadata line.

When `body.example` is set, its text replaces the body marker entirely.
When `body.enabled` is false the marker is omitted entirely.

## Annotation grammar

| Slot | Form | Carries |
|---|---|---|
| **Leading `# …` lines** above a field | `# <prose>` or `# e.g. <value>` | description (single-line prose) and an illustrative example |
| **Inline `# …`** at end of the value line | `# <type>[<format>][; delete-ok]` | structural metadata: type, optional format refinement, optional delete-ok tag |

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

Form: **`# <type>[<format>][; delete-ok]`**

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
- **Skip-ok tag** (optional, after `;`): the single tag `delete-ok`. Present
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
| `title: "Curriculum Vitae"  # string; delete-ok` | Endorsed string — keep or override |
| `count: 0  # integer; delete-ok` | Endorsed integer (type-empty default, explicitly shippable) |
| `active: false  # boolean; delete-ok` | Endorsed boolean (type-empty default, explicitly shippable) |
| `notes: ""  # string; delete-ok` | Endorsed empty string (the "skippable" cell, now Endorsed) |
| `bio: |-` followed by indented `<must-fill>`, then `# markdown` | Must Fill markdown — see "Markdown fields render as block scalars" |
| `recipient: <must-fill>  # array<string>` | Must Fill array of strings |
| `date: <must-fill>  # date<YYYY-MM-DD>` | Must Fill date in `YYYY-MM-DD` format |
| `severity: <must-fill>  # enum<low \| medium \| high>` | Must Fill enum |
| `$quill: cmu_letter@0.1.0` | quill binding metadata, emitted verbatim, do not modify |
| `$kind: skill` followed by `# composable (0..N)` | repeat the entire `~~~` … `~~~` block per instance |

## Placeholder value precedence

The rendered value follows a single cascade keyed on the cell:

| Field state | Value rendered | Cell |
|---|---|---|
| Has `default` | default | Endorsed (carries `; delete-ok`) |
| No `default` | `<must-fill>` sentinel | Must Fill (no `; delete-ok`) |

Examples never become the rendered value, regardless of cell or type —
this holds uniformly for scalars, arrays, typed tables, and typed
dictionaries. An example matches the *shape* of the desired value but is
not the value most authors want, so it always surfaces in the `# e.g.`
leading line while the value follows the cascade above.

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
bio: |-  # markdown; delete-ok
  ## About me
  
  <body>
```

If the default is empty (`default: ""`), the block scalar still carries
the `; delete-ok` tag and renders with one indented blank line — the
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

A field of `type: array` with a `properties` map follows the uniform
cell cascade — `default:` (any default, including `[]`) is Endorsed and
shippable as-is; no `default:` is Must Fill:

- A non-empty `default:` renders as actual rows (no per-property
  annotations on each row). The outer key carries `# array<object>; delete-ok`.
- `default: []` renders inline as `[]` with `# array<object>; delete-ok` —
  shippable empty. Inline row shape is not surfaced under an empty
  default; use `example:` to document row shape. See
  `prose/BOOKMARKS.md` "Typed container empty default loses inline
  shape documentation."
- No `default:` is Must Fill: one synthetic row is emitted with each
  property carrying its own description, inline annotation, and cell
  signal (sentinel or `; delete-ok`). The outer key carries
  `# array<object>` (no `; delete-ok`).

An `example:` never renders as rows. Like every other field type, it
surfaces only in the `# e.g.` leading line — as a one-line flow
sequence, e.g. `# e.g. [{org: ACME, year: 2020}]`.

## Typed dictionaries

A field of `type: object` with a `properties` map follows the uniform
cell cascade — `default:` (any default, including `{}`) is Endorsed and
shippable as-is; no `default:` is Must Fill:

- A non-empty `default:` renders as a concrete block mapping (property
  values only, no annotations). The outer key carries
  `# object; delete-ok`.
- `default: {}` renders inline as `{}` with `# object; delete-ok` —
  shippable empty. Inline property shape is not surfaced under an empty
  default; use `example:` to document property shape.
- No `default:` is Must Fill: each property is emitted with its own
  description, inline annotation, and cell signal. The outer key
  carries `# object` (no `; delete-ok`).

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
  zip: ""  # string; delete-ok
```

With a default:

```
address:  # object; delete-ok
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
card-yaml opener — a bare `~~~` (or the legacy `~~~card-yaml` alias) — is
rejected at `Quill.yaml` parse time (`quill::body_example_contains_fence`)
to prevent corrupting the blueprint's document structure.

## Worked example

```
~~~
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
department: ""  # string; delete-ok
# The sender's institutional mailing address.
# e.g. [5000 Forbes Avenue, "Pittsburgh, PA 15213-3890"]
address: <must-fill>  # array<string>
# The department or university website URL.
# e.g. www.ece.cmu.edu
url: ""  # string; delete-ok
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
reports as `validation::must_fill_sentinel` until the LLM replaces it
with a typed value.

`blueprint()` does **not**, on its own, guarantee the document
*renders*. Rendering depends on the quill's `plate.typ` and its
packages, which `blueprint()` does not control. That is a separate
**quill authoring contract**:

> A quill's `plate.typ` MUST render an **empty document** (just `$quill` /
> `$kind: main`, no fields) to a successful (non-error) output. Under
> zero-filled render, every absent field is filled with its type-empty
> (zero) value in the plate projection, so an empty document is by
> construction the *type-minimal valid input*.

The zero-filled empty document is the *type-minimal valid input* — the
worst-case-but-renderable document. A plate that renders it has shown it
degrades gracefully on every type-valid input shape. The contract
requires:

- Templates treat type-empty values (`""`, `0`, `false`, `[]`, empty
  markdown body) as valid *present* input — read via `data.field`,
  `card.at("field", default: …)`, or guarded with `if "field" in data`.
- No template asserts that a Must Fill field is *non-empty*. The schema
  guarantees *presence*, not non-emptiness; the `<must-fill>` sentinel
  is an authoring signal, not a render-time precondition.
- "Renders successfully" means "compiles without error," not "produces
  meaningful output." An empty-string title is a blank title — that is
  acceptable.

The contract is enforced by a fixture test that renders each bundled
quill's empty document (zero-filled) and asserts success
(`crates/quillmark/tests/quiver_test.rs::every_quill_in_quiver_renders`).

## Two reference documents

A quill projects into two intent-named reference documents, each ordering
its value sources for its own purpose (there is no cross-output "default
always wins" rule). Both are annotated, parseable, and schema-valid; the
fill strategy is an internal detail, not a public parameter.

| Output | Intent | Value precedence | Sentinels? |
|---|---|---|---|
| `blueprint` | *"give me the form to fill"* | `default:`, else `<must-fill>` | yes |
| `example` | *"show me a filled-out one"* | `example:` › `default:` › type-empty zero | no |

- The **blueprint** is the canonical authoring surface: an Endorsed field
  (has a `default:`) renders its default; a Must Fill field carries the
  `<must-fill>` sentinel.
- The **example** document is the illustrative consolidation — each
  field's `example:`, else its `default:`, else its zero value — with no
  sentinels. It is example-*first* but not guaranteed fully populated
  (a field with neither an `example:` nor a `default:` renders blank). A
  field with *both* a default and an example shows its example here but
  its default on the render path: the example optimizes for illustration,
  render for fidelity.
- The per-field **zero value** (`""`, `0`, `false`, `[]`, `{}`, first
  enum variant; `quillmark_core::quill::zero_value`) is one shared
  producer — the example fallback above *and* the render floor for
  zero-filled render (see
  [zero-filled-render.md](../proposals/zero-filled-render.md), pending
  graduation into [SCHEMAS.md](SCHEMAS.md)).

## Bindings surface

| Binding | Accessor |
|---|---|
| Rust | `QuillConfig::blueprint() -> String`, `QuillConfig::example() -> String` |
| Wasm | `Quill.blueprint` / `Quill.example` getters |
| Python | `Quill.blueprint` / `Quill.example` properties |
| CLI | `quillmark blueprint <QUILL_PATH> [-o <FILE>]`; `render` with no input file renders the `example` document |

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

- **Declare `default:`** when the value is what the *majority* of authors
  want — including a type-empty value like `""`, `[]`, `false`, or `0`.
  Most authors want it, so the field can be omitted and the default is
  interpolated for them. The field becomes Endorsed and the blueprint
  carries `; delete-ok`.
- **Omit `default:`** when there is no value most authors want — the
  author, LLM, or user must supply one before shipping. The field becomes
  Must Fill and the blueprint carries the `<must-fill>` sentinel.
- **Use `example:`** when a value matches the semantic and type shape of
  what the author wants but is *not* the value they'd want most of the
  time. It documents shape, not the choice — orthogonal to the cell
  decision, it appears in the leading `# e.g.` line for any cell and never
  renders as the value.

This is documentation, not enforcement.

### Writing the literal string `<must-fill>` as content

The blueprint emitter detects the unquoted form `<must-fill>` as the
sentinel. To write the literal string as a field value, quote it:
`"<must-fill>"`. Exact-string-equality detection treats the unquoted
form as the sentinel and the quoted form as content.
