# Schema Model (`QuillConfig`)

> **Design basis**: [CARD_MODEL.md](../proposals/CARD_MODEL.md) defines the
> unified `cards:` map and the `card` vocabulary this document describes.

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `cards.main.fields`
- `cards.<card_kind>.fields`
- optional `ui` and `body` blocks on `cards.main` and each inline card kind

Supported field types:

| Quill.yaml Type | Meaning |
|---|---|
| `string` | UTF-8 text |
| `number` | Numeric value (integers and decimals) |
| `integer` | Integer-only numeric value |
| `boolean` | `true` / `false` |
| `array` | Ordered list; add `properties:` for typed rows |
| `object` | Structured map; requires `properties:` |
| `date` | `YYYY-MM-DD` |
| `datetime` | ISO 8601 |
| `markdown` | Rich text; backends handle conversion |

## Type coercion

`QuillConfig::coerce_frontmatter` and `coerce_card` run before validation.

- Returns `Result<IndexMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and per-card fields to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced
- Coercion rules per type: array wrapping, boolean from string/int/float, number/integer from string, string/markdown pass-through, date/datetime format validation, object property recursion

## Native validation

Validation is implemented by a native walker over `QuillConfig` in `quill/validation.rs`.

- Entry point: `QuillConfig::validate_document(&Document)` (dispatches to `validate_typed_document`)
- Returns `Result<(), Vec<ValidationError>>`
- Collects all errors (does not short-circuit)
- Emits path-aware errors for top-level fields and card fields
- Validates each inline card has a `KIND` discriminator matching a known card kind
- Enforces `body.enabled: false` on the main card and on each card kind — body content for a body-disabled card emits `ValidationError::BodyDisabled` (whitespace-only bodies are treated as empty)

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields and card kinds (`group`, `order`, `compact`, `multiline`, `title`)
- `body` blocks on cards (`enabled`, `description`)
- A required `QUILL` sentinel prepended to `cards.main.fields` (`const = "<name>@<version>"`)
- A required `KIND` discriminator field prepended to each `cards.<name>.fields` (`const = "<name>"`)

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `CardSchema`, `UiFieldSchema`, `UiCardSchema`, and `BodyCardSchema` — there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema key: a single `cards` map keyed by card name. The reserved key `cards.main` is the entry-point card; every other entry is an inline card kind whose key is its `KIND` discriminator. `cards.main` and each inline card kind share the same `CardSchema` shape: `fields` (map keyed by field name), optional `description`, optional `ui`, optional `body`. Each `FieldSchema` includes `type`, optional `description`/`default`/`example`/`enum`/`properties`/`ui`, and optional `required` (omitted when false).

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata`; Python: `Quill.metadata` plus dedicated getters). For a document-shaped reference, consumers use the generated blueprint (Wasm: `Quill.blueprint`; Python: `Quill.blueprint`).

### Bindings surface

| Binding | Schema accessor |
|---|---|
| Rust | `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML) |
| Wasm | `Quill.schema` getter (JSON) |
| Python | `Quill.schema` getter (YAML) |
| CLI | `quillmark schema <path>` |

### `cards.main.fields` and `cards.<name>.fields` sentinels

`schema()` prepends a synthetic field to each card's `fields` map so consumers know exactly which fixed value to write (`QUILL` as the frontmatter sentinel, `KIND` as the card fence's kind token):

- `cards.main.fields.QUILL` — `{ type: string, const: "<name>@<version>", required: true, description: ... }`
- `cards.<name>.fields.KIND` — `{ type: string, const: "<name>", required: true, description: ... }`

These appear ahead of the author's declared fields. They are not present in `Quill.yaml`; the projection injects them.
