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
$quill: <name>@<version> # keep verbatim
$kind: main
# <description>

# <field description>
field: !must_fill # <type>
  - <example item>
endorsed: value # <type>[<format>]
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

Every block is a bare `~~~` block (the canonical card-yaml fence; `~~~card-yaml`
is also accepted as an alias — see
[markdown-spec.md](../references/markdown-spec.md) §3): the root block carries
the `$quill` system-metadata line; each composable card carries a
`$kind: <card_kind>` metadata line.

When `body.example` is set, its text replaces the body marker entirely.
When `body.enabled` is false the marker is omitted entirely.

## One emitter, by construction

`blueprint()` does not format YAML itself. It builds a `Document` — the same
typed model a parsed `.md` produces, with prose annotations as comments and
`!must_fill` as fill flags — and emits it through the **canonical
`Document::to_markdown`**. There is no second formatter. Two consequences
follow:

- The blueprint round-trips through `Document::from_markdown` and back **by
  construction** — the emitter that produced it is the same one round-trip uses.
- The blueprint inherits `to_markdown`'s representation choices: a **one-space**
  ` # ` inline-comment gap, **block-style** sequences at every level (no inline
  flow), and **inline double-quoted** multi-line strings (no `|`/`>` block
  scalars). The sections below reflect those choices.

## Annotation grammar

| Slot | Form | Carries |
|---|---|---|
| **Leading `# …` lines** above a field | `# <prose>` or `# e.g. <value>` | description (single-line prose) and an illustrative example |
| **Inline `# …`** at end of the value line | `# <type>[<format>]` | structural metadata: the field's type and an optional format refinement |

The two slots have disjoint purposes: leading is prose, inline is
structural. No colon-separated `key: value` annotation syntax appears in
either slot, so neither pattern collides with YAML key/value parsing.

### Leading lines — order

Per field, in order:

1. `# <description>` — `description:` from `Quill.yaml`,
   whitespace-collapsed. **Single line only**; multi-line descriptions are
   rejected at `Quill.yaml` parse time.
2. `# e.g. <value>` — emitted on an **Endorsed** field whenever `example:`
   is configured. Independent of type. On an Endorsed field the example
   never becomes the rendered value, so it surfaces as a hint. On an
   **Unendorsed** field there is normally no `# e.g.` line: the example inlines
   directly as the `!must_fill` marker's suggested value (see "Placeholder
   value precedence"), so a separate hint would be redundant. The one exception
   is `richtext`, which never inlines its example as the value — an Unendorsed
   richtext field with an `example:` therefore keeps the `# e.g.` line (see
   "Richtext fields").

That's it. There is no leading `# required`, `# enum:`, `# default:`, or
`# type:` — those collapse into the inline.

### Inline annotation

Form: **`# <type>[<format>]`**

- **Type slot** (mandatory, first): one of
  `string`, `integer`, `number`, `boolean`, `array`, `object`,
  `richtext`, `datetime`, `enum`.
  Every field is labeled — there is no "self-evident" exemption.
  (`object` requires a `properties` map; freeform untyped objects are not
  supported. `object` also appears in the format slot of typed-table fields
  as `array<object>`.)
- **Format slot** (optional, in `<…>` angle brackets): refines the type
  when the refinement carries information beyond the type name itself.
  - `datetime<YYYY-MM-DD[Thh:mm:ss]>`
  - `richtext<markdown>`, `richtext(inline)<markdown>` — the `<markdown>` slot
    names the surface encoding an author writes over the corpus model
  - `array<string>`, `array<integer>`, `array<object>`, `array<richtext<markdown>>`, …
  - `enum<a | b | c>`
  - omitted for `string`, `integer`, `number`, `boolean`, `object`
    (nothing meaningful to refine).

The inline annotation is **purely structural** — it carries the type (and
optional format), nothing else. Shippability is conveyed by the **value cell**,
not by the annotation: an Endorsed field (one with a `default:`) renders its
concrete default value, which is shippable as-is — keep or override; an
Unendorsed field (no `default:`) carries the `!must_fill` marker on its value
instead. The reader's single rule is: a `!must_fill` marker present → fill it;
a concrete value present → shippable as-is (delete or blank the line to fall
back to the default).

The `$`-prefixed system-metadata keys (`$quill`, `$kind`, …) carry no
inline type annotation — they are not user-defined data fields, so there
is no `# <type>` slot to fill. (A `$` line *can* carry an ordinary YAML
comment: both an inline trailing ` # comment` and an adjacent own-line
comment parse and round-trip faithfully, exactly like comments on data
fields — see [markdown-spec.md](../references/markdown-spec.md) §3.3.)

The root block's `$quill` line is emitted verbatim and carries an inline
**`# keep verbatim`** reminder — an in-band guard against the
`parse::missing_quill` failure, where an LLM author omits the `$quill` line
entirely and the document fails to bind to a quill. The reminder rides only
on `$quill`: it is the one line whose omission is a hard error. `$kind: main`
carries no reminder — an omitted root `$kind` is synthesised at parse time,
so dropping it is not an error, and a `# …` line in that slot would only
read as a leading annotation for the field below it. A composable card's kind is carried in its
`$kind: <card_kind>` metadata line. Its `composable (0..N)` role is
emitted as an own-line `# composable (0..N)` comment directly under the
`$kind` line, ahead of the card description — that comment carries the
card's cardinality, which is structural information rather than a
redundant instruction.

Examples:

| Line | Reading |
|---|---|
| `name: !must_fill # string` | Unendorsed string, no example — bare marker, replace before shipping |
| `name: !must_fill Jane Doe # string` | Unendorsed string with an `example` — the example is the suggested value, still marked |
| `title: "Curriculum Vitae" # string` | Endorsed string — concrete value, shippable as-is (keep or override) |
| `count: 0 # integer` | Endorsed integer (type-empty default, shippable as-is) |
| `active: false # boolean` | Endorsed boolean (type-empty default, shippable as-is) |
| `notes: "" # string` | Endorsed empty string (the "skippable" cell, now Endorsed) |
| `bio: !must_fill # richtext<markdown>` | Unendorsed richtext — bare marker (see "Richtext fields") |
| `recipient: !must_fill # array<string>` | Unendorsed array of strings |
| `date: !must_fill # datetime<YYYY-MM-DD[Thh:mm:ss]>` | Unendorsed datetime |
| `severity: !must_fill # enum<low \| medium \| high>` | Unendorsed enum |
| `$quill: cmu_letter@0.1.0 # keep verbatim` | quill binding metadata, emitted verbatim; the inline reminder guards against dropping the line |
| `$kind: skill` followed by `# composable (0..N)` | repeat the entire `~~~` … `~~~` block per instance |

## Placeholder value precedence

The blueprint emits along **two orthogonal axes**. The *value axis* decides
what data the cell carries; the *marker axis* decides whether the cell is
stamped `!must_fill`. They are independent — the marker never changes the
value, and the value never implies the marker.

| Field state | Value rendered | Marker |
|---|---|---|
| Has `default` (Endorsed) | the default | none |
| No `default`, no `example` (Unendorsed) | none (bare null/empty) | `!must_fill` |
| No `default`, has `example` (Unendorsed) | the `example` | `!must_fill` |

So an Unendorsed field is always stamped `!must_fill`; its *value* is the
field's `example` when one exists (a reviewable suggested value), else bare
(null for scalars, empty for the marked container). An Endorsed field renders
its default with **no marker** — the concrete value cell is the shippability
signal on its own.

An `example` on an **Endorsed** field never becomes the rendered value — it
surfaces in the `# e.g.` leading line instead. Only **Unendorsed** fields
inline the example as the marker's suggested value. This holds uniformly for
scalars, arrays, typed tables, and typed dictionaries — **except `richtext`**,
which never inlines its example as a value in either endorsement state; its
`example:` always surfaces as the `# e.g.` line (see "Richtext fields").

All fields render as **live YAML** — no commented-out fields. The `!must_fill`
marker is the sole "must fill" signal: a reader's mental model is one rule —
**`!must_fill` on a field → replace before shipping; otherwise the value cell
is shippable as-is**. A marked document still renders (the cell zero-fills, or
uses its suggested value); the marker only drives the non-fatal
`validation::must_fill` warning (see "Guarantees").

The marker is stamped where the LLM types the value:

| Type | Marker position | Example |
|---|---|---|
| `string`, `integer`, `number`, `boolean`, `datetime`, `enum` | On the field | `name: !must_fill # string` |
| `array<scalar>` | On the field | `recipient: !must_fill # array<string>` |
| `richtext` | On the field (bare; no block scalar) | `bio: !must_fill # richtext<markdown>` |
| `object` (typed dict) | Per-property recursion | leaves carry `!must_fill` |
| `array<object>` (typed table) | Per-property recursion in one synthetic row | leaves carry `!must_fill` |

### Richtext fields

A richtext field's value cell is markdown — the surface projection of the
corpus model, which `to_markdown` re-emits — carried under a `# richtext<markdown>`
annotation.

An **Unendorsed** `richtext` field renders as a bare marker on the field —
no block scalar:

```
bio: !must_fill # richtext<markdown>
```

The LLM replaces the marked field with its markdown content (a quoted scalar
or a block scalar, the consumer's choice); the marker signals "fill me."

Unlike other scalars, a richtext field never inlines its `example:` as the
marker's suggested value (a block-scalar placeholder would be indistinguishable
from real content). Instead the `example:` surfaces as a `# e.g.` leading hint:

```
# e.g. Hello world
bio: !must_fill # richtext<markdown>
```

When a `default:` is configured, the field is **Endorsed** and renders its
default as an **inline double-quoted scalar** with `\n` escapes — the canonical
`to_markdown` string form (no `|`/`>` block scalars):

```
bio: "## About me\n\n<body>" # richtext<markdown>
```

If the default is empty (`default: ""`), the cell is the inline empty string
`bio: "" # richtext<markdown>` — the "skippable" richtext cell.

### Multi-element example arrays

The `example` of an Unendorsed array field rides the `!must_fill` marker as a
**block-style sequence** — the canonical `to_markdown` form at every nesting
level:

```
recipient: !must_fill # array<string>
  - Mr. John Doe
  - 123 Main St
  - Anytown, USA
```

Items are quoted only when their plain form would re-parse differently
(`to_markdown`'s scalar rule); in block context a leading/embedded comma does
not force quoting.

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
shippable as-is; no `default:` is Unendorsed:

- A non-empty `default:` renders as actual rows (no per-property
  annotations on each row). The outer key carries `# array<object>`.
- `default: []` renders inline as `[]` with `# array<object>` —
  shippable empty. Inline row shape is not surfaced under an empty
  default; use `example:` to document row shape (tracked in
  [#736](https://github.com/quillmark-org/quillmark/issues/736)).
- No `default:` is Unendorsed: one synthetic row is emitted with each
  property carrying its own description, inline annotation, and the
  `!must_fill` marker on its leaf value. The container key itself is
  untagged — you tag the leaves, not the container (per
  [markdown-spec.md](../references/markdown-spec.md) §3.4). The outer key
  carries `# array<object>`.

An `example:` never renders as rows. Like every other field type, it
surfaces only in the `# e.g.` leading line — as a one-line flow
sequence, e.g. `# e.g. [{org: ACME, year: 2020}]`.

## Typed dictionaries

A field of `type: object` with a `properties` map follows the uniform
cell cascade — `default:` (any default, including `{}`) is Endorsed and
shippable as-is; no `default:` is Unendorsed:

- A non-empty `default:` renders as a concrete block mapping (property
  values only, no annotations). Only the keys present in the default are
  shown — a *partial* default is a deliberate "already handled, ignore the
  rest" signal and is rendered verbatim. The outer key carries `# object`.
- `default: {}` **expands** to the field's zero-filled shape: every property
  shown with its type-empty value (`""`, `0`, `false`, `[]`, …), all
  unmarked and unannotated (uniform with a concrete default, since the
  container is Endorsed). The bare `{}` is never emitted — an empty endorsed
  object shows its structure. The outer key carries `# object`.
- No `default:` is Unendorsed: each property is emitted with its own
  description, inline annotation, and the `!must_fill` marker on its leaf
  value. The container key itself is untagged — you tag the leaves, not the
  container (per [markdown-spec.md](../references/markdown-spec.md) §3.4).
  The outer key carries `# object`.

The `{}` expansion (and not partial defaults, and not arrays) makes the object
rule a single statement: **show every key, fill from default-over-zero, mark
per endorsement.** Arrays are unchanged — `default: []` stays inline `[]`.

An `example:` never renders as a concrete mapping. Like every other
field type, it surfaces only in the `# e.g.` leading line — as a
one-line flow mapping, e.g. `# e.g. {street: 1 Infinite Loop, city:
Cupertino}`.

```
# The sender's mailing address.
address: # object
  # Street address line.
  street: !must_fill # string
  # City name.
  city: !must_fill # string
  # ZIP or postal code.
  zip: "" # string
```

With a default:

```
address: # object
  street: 5000 Forbes Avenue
  city: Pittsburgh
  zip: "15213"
```

With `default: {}` (expanded to the zero-filled shape, all unmarked):

```
address: # object
  street: ""
  city: ""
  zip: ""
```

Properties of a typed dictionary may not themselves be objects (nesting
beyond one level is not supported). The same rule applies to typed table
properties. Freeform `type: object` fields without a `properties` map are
rejected at `Quill.yaml` parse time (`quill::object_missing_properties`).

## UI metadata honored

`ui.order` controls field ordering within the document. Most other `ui:`
keys (`ui.group`, `ui.compact`, `ui.multiline`, `ui.title`) are
presentation-only and do not affect blueprint output. In particular,
`ui.group` emits no banner lines; fields within the same `ui.group`
cluster together via `ui.order`.

## Body markers

- `Write main body here.` after the root block's closing `~~~`
- `Write <card_kind> body here.` after each card block's closing `~~~`
- When `body.example` is set, its text replaces the marker verbatim.

`body.enabled: false` suppresses the marker entirely for body-less cards
(e.g., a `skills` card whose data is purely structured).

A `body.example` whose text contains a line that would parse as a
card-yaml opener — a bare `~~~` (or the `~~~card-yaml` alias) — is
rejected at `Quill.yaml` parse time (`quill::body_example_contains_fence`)
to prevent corrupting the blueprint's document structure.

## Worked example

```
~~~
$quill: cmu_letter@0.1.0 # keep verbatim
$kind: main
# Typeset letters that comply with Carnegie Mellon University letterhead standards.
# The recipient's name and full mailing address.
recipient: !must_fill # array<string>
  - Mr. John Doe
  - 123 Main St
  - Anytown, USA
# The signer's information. Line 1: Name. Line 2: Title.
signature_block: !must_fill # array<string>
  - First M. Last
  - Title
# The department or organizational unit name for the letterhead.
# e.g. Department of Electrical and Computer Engineering
department: "" # string
# The sender's institutional mailing address.
address: !must_fill # array<string>
  - 5000 Forbes Avenue
  - Pittsburgh, PA 15213-3890
# The department or university website URL.
# e.g. www.ece.cmu.edu
url: "" # string
# The date to appear on the letter.
date: !must_fill # datetime<YYYY-MM-DD[Thh:mm:ss]>
~~~

Write main body here.
```

## Guarantees

`blueprint()` guarantees the emitted document is **parseable** *and*
**renders**: every field key is present, every value is YAML-valid, the
document round-trips through `Document::from_markdown` and back, and every
cell is type-valid. Endorsed cells coerce and validate against their default;
Unendorsed cells carry the `!must_fill` marker on a value that is either the
field's `example` (a real, type-valid suggested value) or bare null/empty —
and because **null ≡ absent** (a present-null cell zero-fills at render, just
like an omitted field), even a bare-marked cell renders cleanly. A surviving
marker is surfaced by `Quill::validate` as the **non-fatal**
`validation::must_fill` warning — never a render gate. A strict consumer
(e.g. an LLM authoring loop) treats any outstanding marker as "not done."

Rendering still depends on the quill's `plate.typ` and its packages, which
`blueprint()` does not control. That is a separate **quill authoring
contract**:

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
  richtext body) as valid *present* input — read via `data.field`,
  `card.at("field", default: …)`, or guarded with `if "field" in data`.
- No template asserts that an Unendorsed field is *non-empty*. The schema
  guarantees *presence*, not non-emptiness; the `!must_fill` marker
  is an authoring signal, not a render-time precondition.
- "Renders successfully" means "compiles without error," not "produces
  meaningful output." An empty-string title is a blank title — that is
  acceptable.

The contract is enforced by fixture tests that render each bundled quill's
empty document (`quiver_test.rs::every_quill_in_quiver_renders`) and, for the
`blueprint()` guarantee above, parse, round-trip, and render each quill's
generated blueprint (`quiver_test.rs::every_quill_blueprint_round_trips_and_renders`).

## The blueprint and its filled-out twin

The blueprint is the **one** annotated reference document — the authoring
surface. Its "show me a filled-out one" counterpart is **seeding**, which
materializes a real `Document` (committed, structured content for editor and
render consumers) rather than a second annotated string. There is no
annotated `example` *document*: nothing consumes a filled-out document for its
annotations, so the filled-out projection is committed `Document` content, not
prose.

| Projection | Intent | Value precedence | Output | Markers? |
|---|---|---|---|---|
| `blueprint` | *"give me the form to fill"* | Endorsed: `default:`; Unendorsed: `example:` else bare | annotated string | yes (`!must_fill`) |
| seeding | *"give me a filled-out one"* | `example:` › absent | committed `Document` | no |

- The **blueprint** is the canonical authoring surface: an Endorsed field
  (has a `default:`) renders its default with no marker; an Unendorsed field
  is stamped `!must_fill`, carrying its `example` as the suggested value when
  one exists (else bare null/empty). On an Endorsed field an `example:`
  surfaces only as a `# e.g.` hint, never as the rendered value.
- **Seeding** commits each field's `example:` and leaves every other field
  absent (`example: → absent`, *not* `example: › default: › zero`), so the
  compilation layer fills `default: → zero` underneath at render time. It is
  the committed, structured twin handed to editor consumers. See
  [SCHEMAS.md](SCHEMAS.md) § "Document seeding".
- A seeded document therefore *renders* each field's `example:` where present,
  else its `default:`, else its zero value — the same consolidation an eager
  fill would produce, but resolved at the render floor for fidelity. The
  per-field **zero value** (`zero_value`, defined in
  [SCHEMAS.md](SCHEMAS.md) § "Zero-filled render") is that shared render floor.

## Bindings surface

| Binding | Accessor |
|---|---|
| Rust | `QuillConfig::blueprint() -> String`; the filled-out twin is `Quill::seed_document() -> Document` |
| Wasm | `Quill.blueprint` getter; `Quill.seedDocument()` |
| Python | `Quill.blueprint` property; `Quill.seed_document()` |
| CLI | `quillmark blueprint <QUILL_PATH> [-o <FILE>]`; `render` with no input file renders the **seeded** document |

The Rust example `cargo run -p quillmark-core --example print_blueprint
-- <quill_name> [<version>]` prints the blueprint for any bundled
fixture.

## Relationship to schema

| Concern | Use |
|---|---|
| Validators, form builders, machine consumers | [SCHEMAS.md](SCHEMAS.md) — `schema()` |
| LLM/MCP authoring, prompt-time reference document | this doc — `blueprint()` |

## Authoring guidance

Choosing **Unendorsed vs. Endorsed** per field (declare a `default:` or not)
and **when to reach for `example:`** is schema-authoring guidance owned by
[SCHEMAS.md](SCHEMAS.md) § "`default` and `example`" and § "Unendorsed vs.
Endorsed fields". The blueprint is where that choice becomes visible: an
Endorsed field renders its concrete default (shippable as-is); an Unendorsed
field is stamped with the `!must_fill` marker.

### Writing the literal text `!must_fill` as content

The placeholder is a YAML **tag**, not a string sentinel, so there is no
collision and no quoting escape-hatch to learn. The literal text `!must_fill`
written as an ordinary *value* (`note: "!must_fill"`, or even an unquoted
scalar that merely contains those characters) is just content; a real marker
is the YAML tag attached to a field (`note: !must_fill`). The two are
structurally distinct, so nothing special is required to author the literal
text.
