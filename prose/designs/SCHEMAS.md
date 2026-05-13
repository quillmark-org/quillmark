# Schema Model (`QuillConfig`)

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `main.fields`
- `leaf_kinds.<leaf_kind>.fields`
- optional `ui` and `body` blocks on `main` and each leaf kind

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

`QuillConfig::coerce_frontmatter` and `coerce_leaf` run before validation.

- Returns `Result<IndexMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and per-leaf fields to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced
- Coercion rules per type: array wrapping, boolean from string/int/float, number/integer from string, string/markdown pass-through, date/datetime format validation, object property recursion

## Native validation

Validation is implemented by a native walker over `QuillConfig` in `quill/validation.rs`.

- Entry point: `QuillConfig::validate_document(&Document)` (dispatches to `validate_typed_document`)
- Returns `Result<(), Vec<ValidationError>>`
- Collects all errors (does not short-circuit)
- Emits path-aware errors for top-level fields and leaf fields
- Validates each leaf has a `KIND` discriminator matching a known leaf kind
- Enforces `body.enabled: false` on the main leaf and on each leaf kind — body content for a body-disabled leaf emits `ValidationError::BodyDisabled` (whitespace-only bodies are treated as empty)

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields and leaf kinds (`group`, `order`, `compact`, `multiline`, `title`)
- `body` blocks on leaves (`enabled`, `description`)
- A required `QUILL` sentinel prepended to `main.fields` (`const = "<name>@<version>"`)
- A required `KIND` sentinel prepended to each `leaf_kinds.<name>.fields` (`const = "<name>"`)

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `LeafSchema`, `UiFieldSchema`, `UiLeafSchema`, and `BodyLeafSchema` — there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema keys: `main`, optional `leaf_kinds` (map keyed by leaf name). `main` and each entry in `leaf_kinds` share the same `LeafSchema` shape: `fields` (map keyed by field name), optional `description`, optional `ui`, optional `body`. Each `FieldSchema` includes `type`, optional `description`/`default`/`example`/`enum`/`properties`/`ui`, and optional `required` (omitted when false).

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata`; Python: `Quill.metadata` plus dedicated getters). The bundled example markdown is exposed separately (Wasm: `Quill.example`; Python: `Quill.example`) so consumers choose whether to include it in a prompt.

### Bindings surface

| Binding | Schema accessor |
|---|---|
| Rust | `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML) |
| Wasm | `Quill.schema` getter (JSON) |
| Python | `Quill.schema` getter (YAML) |
| CLI | `quillmark schema <path>` |

### `main.fields` and `leaf_kinds.<name>.fields` sentinels

`schema()` prepends a synthetic field to each leaf's `fields` map so consumers know exactly which sentinel string to write:

- `main.fields.QUILL` — `{ type: string, const: "<name>@<version>", required: true, description: ... }`
- `leaf_kinds.<name>.fields.KIND` — `{ type: string, const: "<name>", required: true, description: ... }`

These appear ahead of the author's declared fields. They are not present in `Quill.yaml`; the projection injects them.
