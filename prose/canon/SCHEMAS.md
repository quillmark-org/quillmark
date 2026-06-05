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
| `array` | Ordered list; requires an `items:` element schema (e.g. `items: { type: string }` for `string[]`, `items: { type: object, properties: … }` for a typed table) |
| `object` | Structured map; requires `properties:` |
| `datetime` | YAML 1.1 timestamp: bare `YYYY-MM-DD` through full RFC 3339 with offset; seconds optional |
| `markdown` | Rich text; backends handle conversion |

## Type coercion

`QuillConfig::coerce_payload` and `coerce_card` run before validation.

- Returns `Result<IndexMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and per-card fields to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced
- Coercion rules per type: array wrapping plus element-wise coercion against the `items` schema (a bad element fails at its indexed path, e.g. `counts[1]`), boolean from string/int/float, number/integer from string, string/markdown pass-through, date/datetime format validation, object property recursion
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
  `validation::must_fill_absent` — a non-fatal signal at render, where the
  field is zero-filled (see [Zero-filled render](#zero-filled-render)).

Field-level type and presence errors render under a uniform shape —
field path, verbatim source token, schema declaration, and both exits
when applicable. See `ERROR.md` § "Validation message contract".

## Value sources and projections

Every field value comes from one of a small set of **sources**, ordered by
*commitment* — how strongly the value claims to be the real answer. This is the
**commitment ladder**:

| Rung | Source | Persisted? | Renders? |
|---|---|---|---|
| top | authored value | yes | yes |
| | `default:` | yes (interpolated when omitted) | yes — the fidelity value |
| | `example:` | only by [seeding](#document-seeding) | only on illustration surfaces |
| floor | type-empty `zero` (`zero_value`) | never | last resort |
| (signal) | `<must-fill>` sentinel | never (error if it survives) | never |

No surface owns a precedence *policy*; each **projection cuts the same ladder**
at a different rung, and the per-rung producers are shared (`zero_value` for the
floor, `FieldSchema::ui_order` for ordering):

| Projection | Per-field precedence | Floor | Output |
|---|---|---|---|
| render (fidelity) | authored › `default:` › zero | zero | plate JSON — [Zero-filled render](#zero-filled-render) |
| `example` document | `example:` › `default:` › zero | zero | annotated string — [BLUEPRINT.md](BLUEPRINT.md) |
| `blueprint` document | `default:` › `<must-fill>` | sentinel | annotated string — [BLUEPRINT.md](BLUEPRINT.md) |
| seeding | `example:` › absent | (deferred to render floor) | committed `Document` — [Document seeding](#document-seeding) |
| form view | authored / `default:` / missing (uncollapsed; carries `example:`) | — | read-only view |

Two seams are deliberate, not uniform: the floor is `zero` on every projection
except `blueprint`, which substitutes the `<must-fill>` *sentinel*; and `zero`
is honestly blank for every type except `enum`, whose zero is the first declared
variant (there is no empty enum member). Both are detailed below.

## Zero-filled render

**Partial documents are first-class citizens.** A document need not be
complete to render — render success is not a completeness signal.
Shippability is the author's judgment; the engine's only hard requirement
is that the document be *well-formed* (values coerce, no surviving
`<must-fill>` sentinel). Completeness is surfaced as a hint — the form
view's per-field `source: "missing"` — never enforced as a gate.

Rendering and the *completeness verdict* are orthogonal. The render path
(`compile_data` / `resolve_fields` in `quillmark::orchestration`) uses
**zero-filled render**: every absent schema field is resolved by precedence
— an authored value, else the `default:`, else the type-empty zero value
(`zero_value`, defined below) — in the plate-JSON projection that feeds the
backend **only, never in the persisted document**.

- **Incomplete is renderable.** A document that merely omits a Must Fill
  field renders fine: the field is zero-filled in the projection, so
  `validation::must_fill_absent` is demoted from a render error to a
  non-fatal signal. The `validate_document` layer still emits the code;
  consumers (e.g. the form view's per-field state) read it for doneness.
- **Malformed is fatal.** A value that cannot coerce to its declared type,
  or a surviving `<must-fill>` sentinel, errors on every path. The sentinel
  is the system's own "replace me" placeholder, so leaving it in is provably
  an authoring accident — rendering it literally is never intended.
- **Non-persist invariant.** The zero-fill lives only in the ephemeral
  projection and must never be written back. A type-empty value is
  indistinguishable from authored-empty, so persisting it would make
  `must_fill_absent` (which keys on absence) vacuous and blind a future
  schema migration to author intent.

The per-field zero value is honestly blank for every scalar type except
`enum`, whose zero is the first declared variant. An `object` with
`properties` is shape-valid only when every property is present, so its zero
is the object whose each property carries that property's zero (recursively). It is the one shared producer behind both this render floor
and the `example` document's fallback (see [BLUEPRINT.md](BLUEPRINT.md)).

## Document seeding

**Seeding** builds a starter `Document` from the schema for editor consumers
("new document"): each field that declares an `example:` is committed verbatim,
and **every other field is left absent**. The seeding cascade is therefore
`example: → absent` — absent fields are never written; they are interpolated at
the compilation layer by [zero-filled render](#zero-filled-render) (`default:`,
else type-empty zero), exactly as for any authored document.

Committing *only* `example` is the whole design. `resolve_fields` already
produces `default` and `zero` at compile time but **never `example`** (example
is excluded from the render path — see [BLUEPRINT.md](BLUEPRINT.md)), so
`example` is the one source the render floor cannot reproduce. Persisting a
`default` would be redundant — the floor interpolates it anyway — and would
*freeze* it against a later schema change; persisting a `zero` is outright
forbidden ([Non-persist invariant](#zero-filled-render)). So the seed writes
exactly the one source that wouldn't otherwise appear and leaves the rest to
the floor. This keeps a split-screen editor/preview consistent — the document
carries real content, the preview renders it, and absent fields resolve
identically in both panes.

The seed is **illustration-first**, exactly like the `example` string (below):
a field carrying *both* an `example` and a `default` commits — and therefore
renders — its **`example`**, not its default. So a seeded document is *not* the
fidelity render: BLUEPRINT.md's "a both-having field renders its default on the
render path" describes authored and blank documents, where no `example` is ever
present. In a seed, the `example` is present, so it wins.

- **Composable cards** are seeded one instance per declared kind; `body.example`
  fills the body when bodies are enabled.
- **The main card** carries `$quill` and `$kind: main`, so a seed round-trips
  through Markdown like an authored document.
- **Provenance is deferred.** A seeded `example` is committed as ordinary
  content, so the form view reports it as `source: "document"`, not `"missing"`
  — a Must-Fill field seeded with an `example` reads as done. Distinguishing an
  untouched seed from authored input is a future addition; correctness and
  renderability do not depend on it.

Seeding is the committed, structured counterpart of the `example` *string*
document ([BLUEPRINT.md](BLUEPRINT.md) § "Two reference documents"), but with a
different cascade: the string fills every field (`example: › default: › zero`)
for illustration, whereas the seed commits only `example` and defers the rest to
the render floor for fidelity. Implemented by `Quill::seed_document` (with
`seed_main` / `seed_card`) in `quillmark`.

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields and card kinds (`group`, `order`, `compact`, `multiline`, `title`)
- `body` blocks on cards (`enabled`, `example`)

The schema describes only the user-fillable fields. The quill reference
(`name@version`, available from quill metadata) and card-kind
discriminators (the `card_kinds` map keys themselves) are document-level
metadata, not schema fields, and do not appear in `fields`.

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `CardSchema`, `UiFieldSchema`, `UiCardSchema`, and `BodyCardSchema` — there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema keys: `main`, optional `card_kinds` (map keyed by card name). `main` and each entry in `card_kinds` share the same `CardSchema` shape: `fields` (map keyed by field name), optional `description`, optional `ui`, optional `body`. Each `FieldSchema` includes `type`, optional `description`/`default`/`example`/`enum`/`properties`/`items`/`ui`. `items` (the element schema, itself a `FieldSchema`) is required on `array` fields and rejected elsewhere; `properties` is used by `object` fields (and by an array's `object`-typed `items`).

### `default` and `example`

`default` and `example` are both type- and shape-valid values, but they
encode opposite author intents:

- **`default`** is the value the *majority* of authors want. Because most
  authors want it, the field can be omitted entirely: at render time the
  default fills any field the document leaves out (an
  authored value always wins — `resolve_fields` in
  `quillmark::orchestration`). A field with a `default:` is **Endorsed** — the
  rendered value is shippable as-is — and the blueprint tags it
  `; delete-ok`. Type-empty defaults (`default: ""`, `[]`, `false`, `0`)
  are the canonical way to mark a "skippable" cell.
- **`example`** matches the semantic and type *shape* of the desired
  value but is *not* the value most authors want. It documents shape, not
  the choice — so it never becomes the rendered value; it only surfaces in
  the blueprint's `# e.g.` line.

### Must-Fill vs. Endorsed fields

A field is **Must Fill** when no `default:` is declared — the quill author
has not endorsed any value, so the `<must-fill>` sentinel signals to LLMs
and authors that the field warrants attention. A missing Must Fill field at
render time zero-fills silently; the non-fatal `validation::must_fill_absent`
is the completeness hint, never a render gate.

A field is **Endorsed** when `default:` is declared; the rendered default
is shippable as-is (the author can keep or override it).

There is no separate `required:` axis; the presence or absence of
`default:` is the sole author choice per field. See
[BLUEPRINT.md](BLUEPRINT.md) for how the two cells render.

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata`; Python: `Quill.metadata` plus dedicated getters).

### Bindings surface

| Binding | Schema accessor |
|---|---|
| Rust | `QuillConfig::schema()` (JSON) / `schema_yaml()` (YAML) |
| Wasm | `Quill.schema` getter (JSON) |
| Python | `Quill.schema` getter (dict) |
| CLI | `quillmark schema <path>` |

### Where the discriminators come from

The schema response omits discriminator fields. Consumers that need to
construct a document derive the discriminators from elsewhere:

- The root block's `$quill` value is `<name>@<version>`, built from
  `quill.metadata.name` and `quill.metadata.version`.
- Each composable card's `$kind` is the key under which it is declared
  in `card_kinds` (e.g. a card listed under `card_kinds.indorsement` is
  written as `$kind: indorsement`).
