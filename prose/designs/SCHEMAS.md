# Schema Model (`QuillConfig`)

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `main.fields`
- `card_types.<card_name>.fields`
- optional `ui` and `body` blocks on `main` and each card type

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
- Validates each card has a `CARD` discriminator matching a known card type
- Enforces `body.enabled: false` on the main card and on each card type — body content for a body-disabled card emits `ValidationError::BodyDisabled` (whitespace-only bodies are treated as empty)

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields and card types (`group`, `order`, `compact`, `multiline`, `title`)
- `body` blocks on cards (`enabled`, `description`)
- A required `QUILL` sentinel prepended to `main.fields` (`const = "<name>@<version>"`)
- A required `CARD` sentinel prepended to each `card_types.<name>.fields` (`const = "<name>"`)

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `CardSchema`, `UiFieldSchema`, `UiCardSchema`, and `BodyCardSchema` — there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema keys: `main`, optional `card_types` (map keyed by card name). `main` and each entry in `card_types` share the same `CardSchema` shape: `fields` (map keyed by field name), optional `description`, optional `ui`, optional `body`. Each `FieldSchema` includes `type`, optional `description`/`default`/`example`/`enum`/`properties`/`ui`, and optional `required` (omitted when false).

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata`; Python: `Quill.metadata` plus dedicated getters). The bundled example markdown is exposed separately (Wasm: `Quill.example`; Python: `Quill.example`) so consumers choose whether to include it in a prompt.

### Bindings surface

| Binding | Schema accessor |
|---|---|
| Rust | `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML) |
| Wasm | `Quill.schema` getter (JSON) |
| Python | `Quill.schema` getter (YAML) |
| CLI | `quillmark schema <path>` |

### `main.fields` and `card_types.<name>.fields` sentinels

`schema()` prepends a synthetic discriminator field to each card's `fields` map so consumers know exactly which discriminator value to use — the `QUILL` reference for the main card, and the card kind (the ```` ```card <kind> ```` info-string token) for each card type:

- `main.fields.QUILL` — `{ type: string, const: "<name>@<version>", required: true, description: ... }`
- `card_types.<name>.fields.CARD` — `{ type: string, const: "<name>", required: true, description: ... }`

These appear ahead of the author's declared fields. They are not present in `Quill.yaml`; the projection injects them.
