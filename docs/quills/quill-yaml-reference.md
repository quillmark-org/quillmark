# Quill.yaml Reference

Complete reference for authoring `Quill.yaml` configuration files. For a hands-on introduction, see [Creating Quills](creating-quills.md).

## File Structure

A `Quill.yaml` has these top-level sections:

```yaml
quill:        # Required — format metadata
  ...

main:         # Optional — main entry-point card: field schemas and optional ui/body
  fields:
    ...
  ui:         # optional UI hints (e.g. title)
  body:       # optional body-region config (e.g. enabled, example)

card_kinds:   # Optional — additional composable card kinds
  ...

typst:        # Optional — backend-specific configuration
  ...
```

Root-level `fields:` is not supported; define the main document's field schemas under `main.fields`.

`Quill.yaml` is parsed strictly. Unknown keys in the `quill:` section, unknown top-level sections, malformed `ui:` blocks, and field schemas that can't be parsed all produce errors — they are never silently dropped. Every error is collected in a single pass, so authors see all problems at once. Run `quillmark validate <quill_dir>` to surface them.

---

## `quill` Section

Every Quill.yaml must have a `quill` section with format metadata.

`quill.name` must be `snake_case` (`^[a-z][a-z0-9_]*$`).

| Key              | Type   | Required | Description |
|------------------|--------|----------|-------------|
| `name`           | string | yes      | Unique identifier for the Quill |
| `backend`        | string | yes      | Rendering backend (e.g. `typst`) |
| `description`    | string | yes      | Human-readable description of the quill itself (non-empty). Independent of `main.description`, which is the optional schema description authored under `main:`. |
| `version`        | string | yes      | Semantic version (`MAJOR.MINOR` or `MAJOR.MINOR.PATCH`) |
| `author`         | string | no       | Creator of the Quill (defaults to `"Unknown"`) |
| `ui`             | object | no       | Document-level UI metadata |

A backend's own settings live under its backend-named section, not in `quill:`.
The Typst template, for example, is declared as `typst.plate_file` (see the
[`typst` Section](#typst-section) below).

```yaml
quill:
  name: usaf_memo
  version: "0.1"
  backend: typst
  description: Typesetted USAF Official Memorandum
  author: TongueToQuill

typst:
  plate_file: plate.typ
```

---

## `main` Section

The main document card holds **root-block field schemas** under `main.fields`. Optional `main.description` describes the schema itself (independent of `quill.description`, which describes the quill package). Optional `main.ui` sets container-level UI for that card. `quill.ui` is merged with `main.ui` when building the main card.

Field order under `main.fields` determines display order in UIs — the first field gets `order: 0`, the second gets `order: 1`, and so on.

Field keys must be `snake_case` (`^[a-z][a-z0-9_]*$`). Capitalized field keys are reserved.

```yaml
main:
  fields:
    subject:          # Field name (used as the card-yaml payload key)
      type: string
      description: Be brief and clear.
```

### Field Properties

| Property      | Type              | Required | Description |
|---------------|-------------------|----------|-------------|
| `type`        | string            | yes      | Data type (see [Field Types](#field-types)) |
| `description` | string            | no       | Detailed help text |
| `default`     | matches `type`    | no       | The value the **majority of authors want**. When the field is omitted, the default is filled in. **Declaring `default` makes the field Endorsed**: the blueprint renders the concrete default value with a type-only annotation (no marker), shippable as-is. Omitting `default` makes the field **Unendorsed**: the blueprint stamps the `!must_fill` marker (carrying the field's `example` as a suggested value when present, else bare). A surviving marker raises the non-fatal `validation::must_fill` warning — it never gates render, since an absent or present-null field zero-fills. |
| `example`     | matches `type`    | no       | A value matching the **type and shape** of what the author wants, but **not** the value desired most of the time. Documents shape only — surfaced in the [blueprint](https://github.com/quillmark-org/quillmark/blob/main/prose/canon/BLUEPRINT.md)'s `# e.g.` line for documentation and LLM authoring, never rendered as the value. |
| `enum`        | array of strings  | no       | Restrict to specific values |
| `ui`          | object            | no       | UI rendering hints (see [UI Properties](#ui-properties)) |
| `items`       | object            | for `array` | Element schema for an `array` field (a nested field schema). Required on every array. |
| `properties`  | object            | for `object` | Nested field schemas for an `object` typed dictionary (or an array's `object`-typed `items`). Required on every `object` field. |

### Field Types

| Type       | Notes |
|------------|-------|
| `string`   | UTF-8 text |
| `number`   | Numeric scalar (integers and decimals) |
| `integer`  | Integer-only numeric scalar |
| `boolean`  | `true` or `false` |
| `array`    | Ordered list; requires an `items:` element schema |
| `datetime` | Bare `YYYY-MM-DD` through full RFC 3339 with offset |
| `markdown` | Rich text; backends convert to target format |
| `object`   | Structured map; requires a `properties:` map |

### Enum Constraints

Restrict a string field to specific values:

```yaml
main:
  fields:
    format:
      type: string
      enum:
        - standard
        - informal
        - separate_page
      default: standard
      description: "Format style for the endorsement."
```

### Primitive Arrays, Typed Tables, and Typed Dictionaries

Every array declares its element type under `items:`. For a **primitive list**, give `items` a scalar type — coercion and validation then apply element-wise (e.g. each element of an `integer[]` is coerced to an integer, and a bad element fails at its indexed path like `counts[1]`):

```yaml
main:
  fields:
    tags:
      type: array
      items:
        type: string
    counts:
      type: array
      items:
        type: integer
    sections:
      type: array
      items:
        type: markdown   # each element is converted to backend markup
```

For a **typed table** — a list of structured rows — give `items` an `object` type with its own `properties:`. Coercion recurses into each element and converts property values to their declared types:

```yaml
main:
  fields:
    cells:
      type: array
      items:
        type: object
        properties:
          category:
            type: string
          score:
            type: number
```

Use `type: object` with `properties:` for a single structured mapping:

```yaml
main:
  fields:
    address:
      type: object
      properties:
        street:
          type: string
        city:
          type: string
```

Nesting beyond one level is not supported — an array element may not itself be an array.

---

## UI Properties

The `ui` property on fields controls how form builders and wizards render the field. These are UI hints, not validation constraints.

### `title`

Overrides the display label shown next to the input. Form builders derive a label automatically from the snake_case field key (`memo_for` → "Memo For"), so `ui.title` is only needed when that automatic label is wrong or misleading:

```yaml
main:
  fields:
    memo_for:
      type: array
      items:
        type: string
      ui:
        title: To       # "Memo For" would confuse users unfamiliar with memo conventions
```

### `group`

Organizes fields into visual sections:

```yaml
main:
  fields:
    memo_for:
      type: array
      items:
        type: string
      ui:
        group: Addressing

    memo_from:
      type: array
      items:
        type: string
      ui:
        group: Addressing

    letterhead_title:
      type: string
      ui:
        group: Letterhead
```

Fields with the same `group` value are rendered together. The group name becomes the section heading.

### `order`

Auto-assigned from field position. To override:

```yaml
main:
  fields:
    # Will get order: 0 from position, but we force it to 5
    special_field:
      type: string
      ui:
        order: 5
```

### `compact`

When `true`, the UI renders this field in a compact style (smaller vertical footprint).

```yaml
main:
  fields:
    tag:
      type: string
      ui:
        compact: true
```

### `multiline`

Controls the initial size of the text input for `string` and `markdown` fields. When `true`, the UI starts with a larger text box instead of a single-line input:

```yaml
main:
  fields:
    summary:
      type: markdown
      description: Executive summary
      ui:
        multiline: true   # start as a larger text box

    notes:
      type: string
      description: Free-form notes
      ui:
        multiline: true

    tagline:
      type: markdown
      description: One-sentence tagline
      # no multiline — single-line input that expands on demand
```

Meaningful on `string` and `markdown` fields; ignored on other types.

---

## `card_kinds` Section

`card_kinds` define composable, repeatable content blocks (the *kinds* — a document can then carry zero or more *instances* of each kind, interleaved with body content). Each entry is shaped exactly like `main:` (`fields`, optional `description`, `ui`, `body`); think of `main:` as the single mandatory card-kind for the document body, and `card_kinds:` as the library of additional kinds that may attach to it.

Card-kind names (the keys under `card_kinds`) must match `[a-z_][a-z0-9_]*` (leading underscore is allowed).

```yaml
card_kinds:
  indorsement:                    # Card-kind name
    description: Chain of routing endorsements.
    fields:
      from:
        type: string
        ui:
          group: Addressing
      format:
        type: string
        enum: [standard, informal, separate_page]
        default: standard
```

Invalid card-kind names include:

- `BadCard` (uppercase letters)
- `my-card` (hyphen)
- `2nd_card` (starts with a digit)

### Card Properties

| Property      | Type   | Required | Description |
|---------------|--------|----------|-------------|
| `description` | string | no       | Help text describing the card's purpose |
| `fields`      | object | no       | Field schemas (same structure as top-level fields) |
| `ui`          | object | no       | Container-level UI hints (see [Card-level `ui`](#card-level-ui)) |
| `body`        | object | no       | Body-region config (see [Card-level `body`](#card-level-body)) |

### Card-level `ui`

| Property | Type   | Description |
|----------|--------|-------------|
| `title`  | string | Display label for the card kind. Literal string or `{field}` template |

### Card-level `body`

| Property  | Type   | Description |
|-----------|--------|-------------|
| `enabled`     | bool   | Whether the body editor is enabled (default: true). When false, consumers must not accept or store body content for this card kind. |
| `example`     | string | Default body text used when seeding a card of this kind and shown in the blueprint body region; falls back to `Write <kind> body here.` when absent. |

#### `title`

A human-readable display label for the card kind. UI consumers should prefer it over the snake_case map key when rendering section headers, chips, picker entries, or per-instance titles in a list.

The label is decoupled from the map key (e.g. `indorsement`), which is the on-the-wire `$kind` discriminator. Authors can rename the label freely without invalidating stored documents.

**Two flavors:**

A literal string serves as a static type label:

```yaml
card_kinds:
  indorsement:
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
```

A template containing `{field_name}` tokens lets UI consumers produce a per-instance title by interpolating live field values:

```yaml
card_kinds:
  endorsement:
    ui:
      title: "{from} → {for}"
    fields:
      from:
        type: string
      for:
        type: string
```

With the template form, a UI rendering a list of cards can title each instance (e.g. `"ORG1/SYM → ORG2/SYM"`) instead of falling back to a generic `"Card (2)"`.

**Interpolation rules (for UI consumers):**
- `{field_name}` is replaced with the current value of that field.
- A title with no `{}` tokens is rendered verbatim — it's just a literal label.
- If a referenced field is absent or empty, the token resolves to an empty string.
- UI consumers are responsible for trimming degenerate separators (e.g. `" — "` with one empty side).

When omitted, UI consumers fall back to the prettified map key.

#### `body.enabled`

When `false`, the card kind has no body/content area. Consumers must not accept or store body content for instances of this card kind. The validator enforces this: a document instance that provides body content for a `body.enabled: false` card kind is rejected with a `BodyDisabled` error.

```yaml
card_kinds:
  metadata_block:
    body:
      enabled: false    # Card has fields only, no body/content area
    fields:
      category:
        type: string
```

#### `body.example`

Default body text seeded into a card of this kind and shown verbatim in the blueprint body region (it falls back to `Write <kind> body here.` when absent). Has no effect when `body.enabled` is false.

```yaml
card_kinds:
  experience:
    body:
      example: Describe your role, responsibilities, and key achievements.
    fields:
      company:
        type: string
```

### Using Cards in Markdown

Card kinds defined here are authored as `~~~` blocks (with a `$kind: <kind>` line) in the document body. See [card-yaml Blocks](../authoring/card-yaml.md#card-blocks) for the markdown syntax.

---

## `typst` Section

Backend-specific configuration for the Typst renderer.

| Key          | Type   | Required | Description |
|--------------|--------|----------|-------------|
| `plate_file` | string | no       | Path (relative to the quill root) to the Typst template the backend compiles |
| `packages`   | array  | no       | Typst packages the template depends on |

```yaml
typst:
  plate_file: plate.typ
  packages:
    - "@preview/appreciated-letter:0.1.0"
```

See the [Typst Backend Guide](typst-backend.md) for details.

---

## Reading the schema programmatically

Quillmark emits a public schema contract derived from `Quill.yaml`. Accessors:

- Rust: `QuillConfig::schema()` (JSON) / `QuillConfig::schema_yaml()` (YAML)
- Python: `quill.schema` (structured dict)
- WASM: `quill.schema` (JSON)
- CLI: `quillmark schema <path>`

`ui:` hints are preserved verbatim in the output. See [SCHEMAS.md](https://github.com/quillmark-org/quillmark/blob/main/prose/canon/SCHEMAS.md) for the emitted shape.

---

## Complete Example

```yaml
quill:
  name: project_report
  version: "1.0"
  backend: typst
  description: Monthly project status report
  author: Engineering Team

typst:
  plate_file: plate.typ

main:
  fields:
    project_name:
      type: string
      ui:
        group: Header

    status:
      type: string
      enum: [on_track, at_risk, blocked]
      ui:
        group: Header

    risk_description:
      type: string
      default: ""
      ui:
        group: Header
      description: Describe the risk or blocker. Only needed when status is not on_track.

    date:
      type: datetime
      ui:
        group: Header

    team_members:
      type: array
      items:
        type: string
      default: []
      ui:
        group: Team

    budget:
      type: number
      default: 0
      ui:
        group: Financials

card_kinds:
  milestone:
    description: A project milestone with target date and status.
    fields:
      name:
        type: string
      target_date:
        type: datetime
      completed:
        type: boolean
        default: false
```

---

## Next Steps

- [Creating Quills](creating-quills.md) — hands-on tutorial
- [Markdown Syntax](../authoring/markdown-syntax.md) — document authoring syntax
- [CLI Reference](../cli/reference.md) — validating quills with the `validate` command
