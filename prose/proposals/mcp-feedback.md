# MCP Authoring-Surface Cleanup

> **Motivation**: friction observed in an MCP consumer's eval, where LLM
> authors of Quillmark Markdown made systematic mistakes the consumer had
> to bandage with prompt-side rules. This proposal addresses the subset
> of that friction whose fixes do not break the CommonMark-superset
> guarantee or introduce silent coercion.

## TL;DR

Three changes: drop the redundant `$kind: main` requirement on the root
block; give validation errors a uniform field-path / source-token /
both-exits format; and collapse the field-schema axes (`required`,
`optional`, `default`) into a two-cell model where a field either has a
`default:` (Endorsed) or does not (Must Fill). Pre-1.0; breaking changes
acceptable.

## Background

A consumer building an MCP server over Quillmark surfaced friction
points where LLM authors made systematic mistakes. Three of them
(asymmetric fences, tildes-only, `---` frontmatter as sugar) defend
invariants worth more than the friction they cost and are not addressed
here. The remaining ones — root-block redundancy, type-mismatch error
quality, and the muddled `required`/`optional`/`default` schema axes —
are addressed below.

The field-schema collapse below resolves an overpromise: the
`required` / `optional` axis advertised a validation gate the system did
not implement, because the blueprint pre-fills every field and absence
never reaches validation through the blueprint pathway. The new model
replaces the overpromise with a single author choice (`default:` or not)
and a visible blueprint signal (sentinel value or real value) per cell.

## Change 1: Drop `$kind: main` requirement on the root block

The root block is identified by position (MARKDOWN.md §2). Requiring
`$kind: main` on the root is double bookkeeping that LLMs forget to
satisfy and that adds no validation power.

**Parsing.** Root block with `$kind: main` continues to validate. Root
block without `$kind` validates as well; `main` is the implicit kind by
virtue of position.

**Canonical emission.** Canonical emission auto-injects `$kind: main`
on the root, so the canonical form is unchanged and existing canonical
documents round-trip byte-equal. Non-canonical input (omitted `$kind`
on root) converges to the canonical form on first emit.

**Rejected:** a composable block declaring `$kind: main` is still a
parse error. The reservation of `main` for the root holds.

**Spec touches:** MARKDOWN.md §3.3 (rules block), §9 (emission
contract), §10 (errors).

**Implementation:** `crates/core/src/document/`.

## Change 2: Uniform validation-error format

Validation errors that fire when a YAML scalar's parsed type does not
match the schema's declared type — or when a sentinel survives to
validation — emit a single canonical shape:

- Names the field by its path (`recipient`, `cards[2].author`).
- Shows the source token verbatim (`42`, `null`, `true`, `""`,
  `<must-fill>`).
- Offers both exits when applicable: quote the value, change the
  schema's declared type, or replace the sentinel.

No silent coercion. The error message is the lever; the parser stays
strict.

Example messages:

```
Field `build_number` got integer `42`, schema declares `string`.
Either quote the value (`build_number: "42"`) or change the schema's
`type:` to `integer`.
```

```
Field `subtitle` got `null`, schema declares `string` with default
`"My Subtitle"`. Either omit the line (the default will fill in) or
set the value to a string.
```

```
Field `recipient` still contains the placeholder `<must-fill>`.
Replace it with a value of type `array<string>`.
```

**Spec touches:** SCHEMAS.md (Native validation section), ERROR.md
(new code `validation::unfilled_placeholder`).

**Implementation:** `crates/core/src/quill/` (validation walker; error
construction).

## Change 3: Two-cell field model

### 3.1 Schema cells

The `required:` key is removed from `Quill.yaml`. The `optional:` key is
removed. The `default:` key alone determines a field's cell:

| Schema | Author intent | Omission semantic |
|---|---|---|
| `default: <value>` | Endorsed — the rendered value is shippable; LLM may keep or override | pass `<value>` |
| (no `default:`) | Must Fill — LLM must provide content before shipping | `validation::required_field_absent` error |

There is no third cell. "Skippable" use cases (the field may be left
empty in the document) are expressed as Endorsed with a type-empty
default — `default: ""`, `default: []`, `default: false`, etc. The plate
contract (BLUEPRINT.md) already requires plates to accept type-empty
values; this collapse standardizes the encoding rather than introducing
new burden.

### 3.2 Blueprint rendering

Two render shapes:

| Cell | Render |
|---|---|
| Endorsed | `field: <value>  # <type>[<format>]; skip-ok` |
| Must Fill | `field: <must-fill>  # <type>[<format>]` |

The sentinel `<must-fill>` in the value cell signals "you must replace
this." The tag `; skip-ok` in the annotation signals "this default is
shippable; keep or override." The two are orthogonal — every field is
exactly one cell, and the two signals never co-occur on the same field.

A reader's mental model is one rule: **sentinel in value cell → must
replace; otherwise the value cell is shippable.** Endorsed values
include type-empty values (`""`, `[]`, `false`, `0`) and the `; skip-ok`
tag confirms they are deliberate, not artifacts.

### 3.3 Sentinel placement by type

The sentinel lives where the LLM types the value:

| Type | Sentinel position | Example |
|---|---|---|
| `string`, `integer`, `number`, `boolean`, `date`, `datetime`, `enum` | Value cell | `name: <must-fill>  # string` |
| `array<scalar>` | Value cell | `recipient: <must-fill>  # array<string>` |
| `markdown` | Inside the block scalar | `bio: \|-\n  <must-fill>` |
| `object` (typed dict) | Per-property recursion | leaves carry sentinels |
| `array<object>` (typed table) | Per-property recursion in one synthetic row | leaves carry sentinels |

**Markdown.** The block-scalar wrapper (`|-`) is preserved. The scalar's
content is exactly `<must-fill>`, one line. Coercion reads the scalar's
content as a string, compares to the sentinel literal, fires the
placeholder error. When the LLM fills, it replaces the single line with
multi-line markdown; the block-scalar shape is unchanged.

**Typed object.** The container key has no value cell. Each property is
rendered with its own annotation and its own cell signal:

```
address:  # object
  street: <must-fill>  # string
  city: <must-fill>  # string
  zip: ""  # string; skip-ok
```

The container annotation has no `; skip-ok` tag because state is a leaf
concern. If the container itself has a `default:` (concrete block
mapping), the container is Endorsed and carries the tag; property
annotations are dropped per the existing spec:

```
address:  # object; skip-ok
  street: 5000 Forbes Avenue
  city: Pittsburgh
  zip: "15213"
```

**Typed table.** Same recursion. Without a container `default:`, one
synthetic row is emitted with leaf-level sentinels:

```
entries:  # array<object>
  - org: <must-fill>  # string
    year: <must-fill>  # integer
```

With a container `default:`, actual rows render with property values
only (no annotations), and the container carries `; skip-ok`.

### 3.4 Inline annotation grammar

The annotation collapses to:

```
# <type>[<format>][; skip-ok]
```

The role slot (`; required` / `; optional`) is removed. No replacement
token. State is conveyed by the value cell (sentinel or real value) and
by the presence or absence of `; skip-ok`. The `<format>` slot is
unchanged from the current spec.

Examples:

| Line | Reading |
|---|---|
| `name: <must-fill>  # string` | Must Fill string |
| `title: "Curriculum Vitae"  # string; skip-ok` | Endorsed string |
| `is_published: false  # boolean; skip-ok` | Endorsed boolean (type-empty default, explicitly shippable) |
| `notes: ""  # string; skip-ok` | Endorsed empty string (the "skippable" cell, now Endorsed) |
| `date: <must-fill>  # date<YYYY-MM-DD>` | Must Fill date |
| `severity: <must-fill>  # enum<low \| medium \| high>` | Must Fill enum |
| `tags: <must-fill>  # array<string>` | Must Fill array of strings |

### 3.5 Validation

`QuillConfig::coerce_payload` and `validate_document`:

- **Sentinel detection first.** Before per-type coercion runs, the raw
  YAML value is compared against the literal `<must-fill>`. For block
  scalars (markdown), the trimmed content is compared. On match: emit
  `validation::unfilled_placeholder` with the field path; skip per-type
  coercion for this field.
- **Type-only coercion otherwise.** Every present value that is not the
  sentinel is coerced to its declared type. Type mismatch fires
  `validation::type_mismatch` per Change 2.
- **Type-empty values are accepted.** No asymmetric rule for strings or
  markdown. `""`, `[]`, `0`, `false`, empty block scalars all coerce
  successfully for their declared types. The `default: ""` cell is fully
  valid.
- **Absence falls back.** A missing field with a `default:` accepts the
  default. A missing field without a `default:` fires
  `validation::required_field_absent`. (Blueprint flow never produces
  absence; this branch matters for non-blueprint authoring paths.)
- **Errors accumulate.** The walker collects all errors per pass; it
  does not short-circuit on the first placeholder.

### 3.6 Plate contract

Unchanged. Plates must handle type-empty values (`""`, `[]`, `0`,
`false`, empty markdown bodies) per BLUEPRINT.md's existing rendering
contract. Plates never see `<must-fill>` — validation rejects it before
render. The "every quill renders its own blueprint" fixture
(`every_quill_in_quiver_renders`) tightens to "the blueprint after
all `<must-fill>` cells are replaced (or after migration converts them
to Endorsed defaults)" — the bundled quills' blueprints are valid input
once migrated.

**Spec touches:** SCHEMAS.md (field schema definition, validation
section), BLUEPRINT.md (annotation grammar, placeholder cascade,
sentinel rules, worked examples).

**Implementation:** `crates/core/src/quill/` (schema model, validator,
blueprint emitter).

## Change 4: Authoring guidance

BLUEPRINT.md gains a short authoring-guidance section recommending that
schema authors:

- Declare `default:` on a field when a value (including a type-empty
  value) is acceptable to ship as-is.
- Omit `default:` when the author / LLM / user must supply a value.
- Use `example:` for illustrative reference values. `example:` is
  orthogonal to the cell decision — it appears in the leading `# e.g.`
  line for any cell, never rendering as the value.

This is documentation, not enforcement.

**Spec touches:** BLUEPRINT.md (new authoring-guidance section).

## Migration

The transformation is largely mechanical:

| Current schema | New schema | Notes |
|---|---|---|
| `required: true`, no `default:` | drop `required:`; no `default:` | Was Must Fill, stays Must Fill |
| `required: false` (or omitted), no `default:` | drop `required:`; add `default: <type-empty>` | Old "implicit optional" becomes Endorsed empty |
| `required: true`, `default: <value>` | drop `required:`; keep `default:` OR drop both and add `example:` | One judgment call: was the default shippable (keep) or a must-customize placeholder (move to `example`)? |
| `required: false` (or omitted), `default: <value>` | drop `required:`; keep `default:` | Unchanged behavior |

A migration script handles rows 1, 2, and 4 deterministically. Row 3
requires per-field author judgment; the script flags these for review.

Bundled quills in `crates/fixtures/resources/quills/` are migrated as
part of the implementation.

## Worked example

`cmu_letter` after migration:

```yaml
main:
  fields:
    recipient:
      type: array
      example: [Mr. John Doe, 123 Main St, "Anytown, USA"]
      # no default → Must Fill
    signature_block:
      type: array
      example: [First M. Last, Title]
      # no default → Must Fill
    department:
      type: string
      default: ""
      example: Department of Electrical and Computer Engineering
    address:
      type: array
      example: [5000 Forbes Avenue, "Pittsburgh, PA 15213-3890"]
      # no default → Must Fill
    url:
      type: string
      default: ""
      example: www.ece.cmu.edu
    date:
      type: date
      # no default → Must Fill
```

Blueprint:

````
~~~card-yaml
$quill: cmu_letter@0.1.0
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
````

Two render shapes, one tag, no `required` token. The LLM reads
`<must-fill>` as "replace before shipping" and `; skip-ok` as "the
default here is fine; keep or override." Six fields, three signals
(sentinel, plain value with tag, leading `# e.g.` reference), no
ambiguity.

## What this proposal does not do

- Does not change fence syntax (`~~~card-yaml` stays asymmetric;
  tildes-only stays).
- Does not accept `---` frontmatter as sugar.
- Does not add type coercion at the parser boundary.
- Does not add a `field_name_type_mismatch` lint — naming-convention
  heuristics produce false positives (`is_a_test`, `count` as unit
  suffix) and Change 2's boundary error surfaces the real bug
  per-document.
- Does not add a `min_items:` or `non_empty:` constraint for arrays.
  "Required non-empty array" remains unrepresentable — the schema
  author who needs it documents the expectation in the field
  description and relies on plate-side handling. Deferred; revisit
  after migration if real cases accumulate.
- Does not change `example:` semantics. `example:` is illustrative
  reference flavor, orthogonal to cell choice. The `# e.g.` leading
  line is unchanged.
- Does not introduce a "must customize this default" cell. The closest
  expression is no-default + `example:` (Must Fill with reference).

## Implementation order

Each step is a separate commit; the order below is also a safe landing
order.

1. **Spec edits** to MARKDOWN.md, SCHEMAS.md, BLUEPRINT.md, and
   ERROR.md reflecting the changes above. Spec edits go first so the
   implementation has a target.
2. **`$kind: main` relaxation** in the document parser and emitter
   (Change 1). Smallest change; lands independently.
3. **Boundary-error format** in the validation walker (Change 2).
   Independent of Change 3.
4. **Two-cell schema model** in `QuillConfig` and downstream consumers
   (Change 3). Includes:
   - Removal of `required:` / `optional:` from `FieldSchema`.
   - Sentinel detection in the coercion path.
   - `validation::unfilled_placeholder` error code.
   - Blueprint-emitter rewrite: new annotation grammar, sentinel
     rendering, `; skip-ok` tag emission.
5. **Quill migration**: rewrite bundled quills in
   `crates/fixtures/resources/quills/` to the two-cell shape; update
   fixture tests; verify `every_quill_in_quiver_renders` still holds
   (now over migrated blueprints).
6. **Documentation guidance** added to BLUEPRINT.md (Change 4).

## Open questions

- **Migration script for row 3 of the migration table.** The judgment
  call between "keep default as Endorsed" and "move to example as Must
  Fill" cannot be automated. Decision: the script flags these fields,
  emits a comment, and a human reviewer makes the call per field.
  Tooling, not design.
- **Sentinel name collision.** `<must-fill>` is unlikely to appear as
  legitimate content. If a future use case needs to write the literal
  string `<must-fill>` as a value, the workaround is quoting
  (`"<must-fill>"`) — exact-string-equality detection treats the
  unquoted form as the sentinel and the quoted form as content. Not a
  design question; documented in BLUEPRINT.md and ERROR.md.
- **Required non-empty arrays.** Deferred (see "What this proposal does
  not do"). Revisit if migration surfaces material need.
