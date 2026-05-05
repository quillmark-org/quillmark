# Schema Model (`QuillConfig`)

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults/examples extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `main.fields`
- `cards.<card_name>.fields`
- optional `ui` hints on fields/cards/main

Supported field types:

| Quill.yaml Type | Meaning |
|---|---|
| `string` | UTF-8 text |
| `number` | Numeric value (integers and decimals) |
| `integer` | Integer-only numeric value |
| `boolean` | `true` / `false` |
| `array` | Ordered list; use `items:` |
| `object` | Structured map; use `properties:` |
| `date` | `YYYY-MM-DD` |
| `datetime` | ISO 8601 |
| `markdown` | Rich text; backends handle conversion |

## Type coercion

`QuillConfig::coerce(&HashMap<String, QuillValue>)` runs before validation.

- Returns `Result<HashMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and card fields in `CARDS` to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced
- Coercion rules per type: array wrapping, boolean from string/int/float, number/integer from string, string/markdown pass-through, date/datetime format validation, object property recursion

## Native validation

Validation is implemented by a native walker over `QuillConfig` in `quill/validation.rs`.

- Entry point: `QuillConfig::validate(&HashMap<String, QuillValue>)` (dispatches to `validate_document`)
- Returns `Result<(), Vec<ValidationError>>`
- Collects all errors (does not short-circuit)
- Emits path-aware errors for top-level fields and card fields
- Validates `CARDS` array: each element must have a `CARD` discriminator matching a known card type

## Schema emission

Two projections of the same `QuillConfig` source are exposed:

- `QuillConfig::schema()` — **structural schema**. Types, constraints,
  `QUILL`/`CARD` sentinels with `const` values. No `ui` keys. The surface
  for validators, machine consumers, and CLI inspection.
- `QuillConfig::form_schema()` — same shape **plus** field-level (`group`,
  `order`, `compact`, `multiline`) and card-level (`hide_body`,
  `default_title`) `ui` hints. The surface for form builders.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()`
emits a document-shaped, pre-filled Markdown reference that's denser
than schema for prompt-time use.

YAML wrappers `QuillConfig::schema_yaml()` and `QuillConfig::form_schema_yaml()`
encode the same values. Both projections are pinned by serde attributes on
`FieldSchema`, `CardSchema`, `UiFieldSchema`, and `UiContainerSchema` —
there is no parallel mirror struct. The clean variant is produced by
recursively stripping `ui` keys after serialisation.

Top-level keys: `main`, optional `card_types` (map keyed by card name).
`main` and each entry in `card_types` share the same `CardSchema` shape:
`fields` (map keyed by field name), optional `description`, and —
in `form_schema()` only — `ui`. Each `FieldSchema` includes `type`,
optional `description`/`default`/`example`/`enum`/`properties`/
`items`, optional `required` (omitted when false), and — in `form_schema()`
only — optional `ui`.

Identity fields (`name`, `version`, `backend`, `author`, `description`)
live on the parent metadata object (Wasm: `Quill.metadata`; Python:
`Quill.metadata` plus dedicated getters). The bundled example markdown is
exposed separately (Wasm: `Quill.example`; Python: `Quill.example`) so
consumers choose whether to include it in a prompt.

### Bindings surface

| Binding | Clean schema | Form schema |
|---|---|---|
| Rust | `QuillConfig::schema()` / `schema_yaml()` | `QuillConfig::form_schema()` / `form_schema_yaml()` |
| Wasm | `Quill.schema` getter | `Quill.formSchema` getter |
| Python | `Quill.schema` getter (YAML) | `Quill.form_schema` getter (YAML) |
| CLI | `quillmark schema <path>` | `quillmark schema <path> --with-ui` |

### `main.fields` and `card_types.<name>.fields` sentinels

Both `schema()` and `form_schema()` prepend a synthetic field to each card's
`fields` map so consumers know exactly which sentinel string to write:

- `main.fields.QUILL` — `{ type: string, const: "<name>@<version>", required: true, description: ... }`
- `card_types.<name>.fields.CARD` — `{ type: string, const: "<name>", required: true, description: ... }`

These appear ahead of the author's declared fields. They are not present in
`Quill.yaml`; the projection injects them.
