# Schema Model (`QuillConfig`)

> **Implementation**: `crates/core/src/quill/`

## TL;DR

`QuillConfig` is the only schema model in quillmark. Validation, coercion, defaults extraction, and public schema emission all read directly from it.

## Quill.yaml DSL

Schema authoring lives in `Quill.yaml` under:

- `main.fields`
- `card_kinds.<card_name>.fields`
- optional `ui` and `body` blocks on `main` and each card kind

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

`QuillConfig::coerce_payload` and `coerce_card` run before validation.

- Returns `Result<IndexMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and per-card fields to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced
- Coercion rules per type: array wrapping, boolean from string/int/float, number/integer from string, string/markdown pass-through, date/datetime format validation, object property recursion
- The Must-Fill sentinel string `<must-fill>` passes through coercion
  unchanged so the validation layer can surface a placeholder diagnostic
  rather than a type-coercion error

## Native validation

Validation is implemented by a native walker over `QuillConfig` in `quill/validation.rs`.

- Entry point: `QuillConfig::validate_document(&Document)` (dispatches to `validate_typed_document`)
- Returns `Result<(), Vec<ValidationError>>`
- Collects all errors (does not short-circuit)
- Emits path-aware errors for top-level fields and card fields
- Validates each card's `$kind` matches a known card kind
- Enforces `body.enabled: false` on the main card and on each card kind — body content for a body-disabled card emits `ValidationError::BodyDisabled` (whitespace-only bodies are treated as empty)
- **Sentinel detection runs first.** Before per-type checks, any value
  equal to the literal string `<must-fill>` (for markdown, the trimmed
  block-scalar content) fires `validation::must_fill_sentinel` and
  skips the type check for that field.
- **Required-field semantics**: a missing field with a `default:` accepts
  the default (no error). A missing field without a `default:` fires
  `validation::must_fill_absent`.

Field-level type and presence errors render under a uniform shape —
field path, verbatim source token, schema declaration, and both exits
when applicable. See `ERROR.md` § "Validation message contract".

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields and card kinds (`group`, `order`, `compact`, `multiline`, `title`)
- `body` blocks on cards (`enabled`, `description`)

The schema describes only the user-fillable fields. The quill reference
(`name@version`, available from quill metadata) and card-kind
discriminators (the `card_kinds` map keys themselves) are document-level
metadata, not schema fields, and do not appear in `fields`.

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `CardSchema`, `UiFieldSchema`, `UiCardSchema`, and `BodyCardSchema` — there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema keys: `main`, optional `card_kinds` (map keyed by card name). `main` and each entry in `card_kinds` share the same `CardSchema` shape: `fields` (map keyed by field name), optional `description`, optional `ui`, optional `body`. Each `FieldSchema` includes `type`, optional `description`/`default`/`example`/`enum`/`properties`/`ui`.

### Must-Fill vs. Endorsed fields

A field is **Must Fill** when no `default:` is declared; the schema author
expects the LLM or user to supply a value before shipping. A missing
Must Fill field at validate time fires `validation::must_fill_absent`.

A field is **Endorsed** when `default:` is declared; the rendered default
is shippable as-is (the author can keep or override it). Endorsed
defaults include type-empty values — `default: ""`, `default: []`,
`default: false`, `default: 0` — which standardize the "skippable" cell.

There is no separate `required:` axis; the presence or absence of
`default:` is the sole author choice per field. See
[BLUEPRINT.md](BLUEPRINT.md) for how the two cells render.

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata`; Python: `Quill.metadata` plus dedicated getters).

### Bindings surface

| Binding | Schema accessor |
|---|---|
| Rust | `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML) |
| Wasm | `Quill.schema` getter (JSON) |
| Python | `Quill.schema` getter (YAML) |
| CLI | `quillmark schema <path>` |

### Where the discriminators come from

The schema response omits discriminator fields. Consumers that need to
construct a document derive the discriminators from elsewhere:

- The root block's `$quill` value is `<name>@<version>`, built from
  `quill.metadata.name` and `quill.metadata.version`.
- Each composable card's `$kind` is the key under which it is declared
  in `card_kinds` (e.g. a card listed under `card_kinds.indorsement` is
  written as `$kind: indorsement`).
