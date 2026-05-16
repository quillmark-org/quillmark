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
  body:       # optional body-region config (e.g. enabled, description)

card_types:   # Optional — additional composable card types
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
| `plate_file`     | string | no       | Path to the plate file |
| `example`        | string | no       | Path to an example Markdown document |
| `example_file`   | string | no       | Alias for `example` |
| `ui`             | object | no       | Document-level UI metadata |

```yaml
quill:
  name: usaf_memo
  version: "0.1"
  backend: typst
  description: Typesetted USAF Official Memorandum
  author: TongueToQuill
  plate_file: plate.typ
  example: example.md
```

---

## `main` Section

The main document card holds **frontmatter field schemas** under `main.fields`. Optional `main.description` describes the schema itself (independent of `quill.description`, which describes the quill package). Optional `main.ui` sets container-level UI for that card. `quill.ui` is merged with `main.ui` when building the main card.

Field order under `main.fields` determines display order in UIs — the first field gets `order: 0`, the second gets `order: 1`, and so on.

Field keys must be `snake_case` (`^[a-z][a-z0-9_]*$`). Capitalized field keys are reserved.

```yaml
main:
  fields:
    subject:          # Field name (used as the YAML frontmatter key)
      type: string
      required: true
      description: Be brief and clear.
```

### Field Properties

| Property      | Type              | Required | Description |
|---------------|-------------------|----------|-------------|
| `type`        | string            | yes      | Data type (see [Field Types](#field-types)) |
| `description` | string            | no       | Detailed help text |
| `default`     | any               | no       | Default value when not provided |
| `example`     | any               | no       | Illustrative value surfaced in the [blueprint](https://github.com/nibsbin/quillmark/blob/main/prose/designs/BLUEPRINT.md) for documentation and LLM authoring |
| `required`    | boolean           | no       | Whether the field must be present (default: `false`) |
| `enum`        | array of strings  | no       | Restrict to specific values |
| `ui`          | object            | no       | UI rendering hints (see [UI Properties](#ui-properties)) |
| `properties`  | object            | no       | Nested field schemas (for `array` typed-table rows or `object` typed dictionaries) |

### Field Types

| Type       | Notes |
|------------|-------|
| `string`   | UTF-8 text |
| `number`   | Numeric scalar (integers and decimals) |
| `integer`  | Integer-only numeric scalar |
| `boolean`  | `true` or `false` |
| `array`    | Ordered list; add `properties:` for typed rows |
| `date`     | YYYY-MM-DD |
| `datetime`  | ISO 8601 |
| `markdown` | Rich text; backends convert to target format |
| `object`   | Structured map; requires a `properties:` map |

Use `type: array` with `properties:` when you need a **list** of structured rows. Use `type: object` with `properties:` for a single structured mapping. Nesting beyond one level is not supported.

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

### Typed Arrays and Typed Dictionaries

Add `properties:` to a `type: array` field to define a typed table — each element is a structured object. Coercion recurses into each element and converts property values to their declared types:

```yaml
main:
  fields:
    cells:
      type: array
      properties:
        category:
          type: string
          required: true
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
          required: true
        city:
          type: string
```

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
      ui:
        title: To       # "Memo For" would confuse users unfamiliar with memo conventions
```

Most fields don't need `ui.title`. Prefer clear field names over fixing a bad key with a title override.

`title` is a UI hint only — no effect on validation, backend rendering, or blueprint output.

### `group`

Organizes fields into visual sections:

```yaml
main:
  fields:
    memo_for:
      type: array
      ui:
        group: Addressing

    memo_from:
      type: array
      ui:
        group: Addressing

    letterhead_title:
      type: string
      ui:
        group: Letterhead
```

Fields with the same `group` value are rendered together. The group name becomes the section heading.

### `order`

Auto-assigned based on field position in the YAML file. You rarely need to set this manually — just put fields in the order you want them displayed.

If you do need to override:

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

When `true`, the UI renders this field in a compact style (smaller vertical footprint). UI hint only — no effect on validation or rendering.

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

`multiline` is a UI hint only — it has no effect on validation or backend processing. It is meaningful on `string` and `markdown` fields; ignored on other types.

---

## `card_types` Section

`card_types` define composable, repeatable content blocks (the *types* — a document can then carry zero or more *instances* of each type, interleaved with body content). Each entry is shaped exactly like `main:` (`fields`, optional `description`, `ui`, `body`); think of `main:` as the single mandatory card-type for the document body, and `card_types:` as the library of additional types that may attach to it.

Card-type names (the keys under `card_types`) must match `[a-z_][a-z0-9_]*` (leading underscore is allowed).

```yaml
card_types:
  indorsement:                    # Card-type name
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

Invalid card-type names include:

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
| `title`  | string | Display label for the card type. Literal string or `{field}` template |

### Card-level `body`

| Property  | Type   | Description |
|-----------|--------|-------------|
| `enabled`     | bool   | Whether the body editor is enabled (default: true). When false, consumers must not accept or store body content for this card type. |
| `description` | string | Description shown in the body editor placeholder when the body is empty. |

#### `title`

A human-readable display label for the card type. UI consumers should prefer it over the snake_case map key when rendering section headers, chips, picker entries, or per-instance titles in a list.

The label is decoupled from the map key (e.g. `indorsement`), which is the on-the-wire `CARD` discriminator. Authors can rename the label freely without invalidating stored documents.

**Two flavors:**

A literal string serves as a static type label:

```yaml
card_types:
  indorsement:
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
```

A template containing `{field_name}` tokens lets UI consumers produce a per-instance title by interpolating live field values:

```yaml
card_types:
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

`title` is a UI hint only — it has no effect on validation or rendering. When omitted, UI consumers fall back to the prettified map key.

#### `body.enabled`

When `false`, the card type has no body/content area. Consumers must not accept or store body content for instances of this card type. The validator enforces this: a document instance that provides body content for a `body.enabled: false` card type is rejected with a `BodyDisabled` error.

```yaml
card_types:
  metadata_block:
    body:
      enabled: false    # Card has fields only, no body/content area
    fields:
      category:
        type: string
```

#### `body.description`

Optional description displayed in the body editor placeholder area when the body is empty. Has no effect when `body.enabled` is false.

```yaml
card_types:
  experience:
    body:
      description: Describe your role, responsibilities, and key achievements.
    fields:
      company:
        type: string
```

### Using Cards in Markdown

Cards appear as fenced code blocks with the info string `card <kind>` in the document body:

````markdown
---
QUILL: usaf_memo
subject: Example
# ... other fields ...
---

Main memo body text here.

```card indorsement
from: ORG/SYMBOL
for: RECIPIENT/SYMBOL
signature_block:
  - JANE A. DOE, Colonel, USAF
  - Commander
```

Body of the first endorsement.

```card indorsement
from: ANOTHER/ORG
for: FINAL/RECIPIENT
format: informal
signature_block:
  - JOHN B. SMITH, Lt Col, USAF
  - Deputy Commander
```

Body of the second endorsement.
````

---

## `typst` Section

Backend-specific configuration for the Typst renderer.

```yaml
typst:
  packages:
    - "@preview/appreciated-letter:0.1.0"
```

See the [Typst Backend Guide](typst-backend.md) for details.

---

## Reading the schema programmatically

Quillmark emits a public schema contract derived from `Quill.yaml`. Accessors:

- Rust: `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML)
- Python: `quill.schema` (YAML)
- WASM: `quill.schema` (JSON)
- CLI: `quillmark schema <path>`

`ui:` hints are preserved verbatim in the output. See [SCHEMAS.md](https://github.com/nibsbin/quillmark/blob/main/prose/designs/SCHEMAS.md) for the emitted shape.

---

## Complete Example

```yaml
quill:
  name: project_report
  version: "1.0"
  backend: typst
  description: Monthly project status report
  author: Engineering Team
  plate_file: plate.typ
  example: example.md

main:
  fields:
    project_name:
      type: string
      required: true
      ui:
        group: Header

    status:
      type: string
      required: true
      enum: [on_track, at_risk, blocked]
      ui:
        group: Header

    risk_description:
      type: string
      ui:
        group: Header
      description: Describe the risk or blocker. Only needed when status is not on_track.

    date:
      type: date
      ui:
        group: Header

    team_members:
      type: array
      ui:
        group: Team

    budget:
      type: number
      default: 0
      ui:
        group: Financials

card_types:
  milestone:
    description: A project milestone with target date and status.
    fields:
      name:
        type: string
        required: true
      target_date:
        type: date
      completed:
        type: boolean
        default: false
```

---

## Next Steps

- [Creating Quills](creating-quills.md) — hands-on tutorial
- [Markdown Syntax](../authoring/markdown-syntax.md) — document authoring syntax
- [CLI Reference](../cli/reference.md) — validating quills with the `validate` command
