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
| `string` | Open scalar UTF-8 text — a value the template computes with (URL, path, identifier, reference key), not prose it lays out |
| `enum` | Closed string domain; requires a `values:` list. Projects to JSON-Schema `{type: string, enum: […]}`. The `enum:` modifier on `string` is a deprecated one-release alias; `enum:`/`values:` on any other type is a load error |
| `number` | Numeric value (integers and decimals) |
| `integer` | Integer-only numeric value |
| `boolean` | `true` / `false` |
| `array` | Ordered list; requires an `items:` element schema (e.g. `items: { type: string }` for `string[]`, `items: { type: object, properties: … }` for a typed table) |
| `object` | Structured map; requires `properties:` |
| `date` | A strict calendar date `YYYY-MM-DD`. Rejects any time component (a time-bearing string is a `datetime`, not a truncated date). The common case in a document engine, so it is the unmarked date type. Stored verbatim; lowers to Typst `datetime(year:, month:, day:)` |
| `datetime` | A strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]`, seconds optional (zero-filled). Rejects timezone offsets (`Z`, `±HH:MM`), the space separator, fractional seconds, and a bare date (which is a `date`). An offset is **rejected, never dropped** — the engine does no zone math, keeping wall-clock semantics end to end. Stored verbatim; lowers to the six-component Typst `datetime(year:, .., second:)` |
| `plaintext` | Navigable **unformatted** prose over the same canonical content (`Content`) as `richtext` — same media type, nav, and regions — but a **literal** codec (`from_plaintext`/`to_plaintext`): delimiters stay literal, no markup, verbatim round-trip. Declare `inline: true` for the single-line variant. Constrained mark-/island-free (`Content::is_plain`); a formatted wire content is rejected (`plaintext::not_plain`), not stripped |
| `richtext` | Rich **formatted** prose over a canonical content (`Content`); markdown is a projection of it. Declare `inline: true` for the single-line variant (exactly one `Para` line, no container, no islands). The pre-richtext `markdown` spelling and the retired `type: richtext(inline)` token are schema load errors (`quill::field_parse_error`) |

The text-ish types form a **data vs content** × **open/plain vs closed/formatted**
2×2: `enum` (closed data), `string` (open data), `plaintext` (plain content),
`richtext` (formatted content). Navigation/regions are a property of the content
model, so `plaintext` and `richtext` share the entire nav/region/preview
stack and the same backend lowering (both carry `contentMediaType:
application/quillmark-content+json`); `plaintext` additionally carries
`quillmark:plain: true`, an editor-only annotation backends ignore.

## Type coercion

`QuillConfig::coerce_payload` and `coerce_card` run before validation.

- Returns `Result<IndexMap<String, QuillValue>, CoercionError>`
- Coerces top-level fields and per-card fields to their declared types
- Fails fast (`Err`) on the first value that cannot be coerced
- Coercion rules per type: array wrapping plus element-wise coercion against the `items` schema (a bad element fails at its indexed path, e.g. `counts[1]`); boolean from string/int/float; number/integer from string or from boolean (`true→1`, `false→0`); string unwraps a length-1 string array into the bare string (else identity); richtext commits the canonical content form (the model) — an authored markdown string imports (via `quillmark-content::import`), an editor-supplied content object revalidates and re-canonicalizes, and the length-1-array-unwrap / bare-scalar-stringify leniencies feed the import; `date`/`datetime` per-type strict-grammar validation, stored verbatim (a `date` rejects any time component, a `datetime` rejects offsets/space/fractional/bare-date; neither truncates); object property recursion
- **`inline` richtext enforcement.** A `richtext` field with `inline: true` requires its content to be exactly one `Para` line, in no container, with no islands (`Content::is_inline`). The empty content satisfies it, so a blank or zero-filled inline field passes. The constraint is checked in three places: coercion (`CoercionError` for a document value), validation (`richtext::not_inline`, the `TypeMismatch` fatality class, as a backstop for a content that bypassed coercion), and load-time example import (a schema literal that violates it is a load error). Blueprint still annotates inline fields as `richtext(inline)<markdown>`; `build_transform_schema` emits `quillmark:inline: true`
- **`plaintext` coercion and enforcement.** A `plaintext` value rides the same content as `richtext`, but a string is imported through the **literal** codec (`from_plaintext`, verbatim — no markdown parse, no escaping) and an editor-supplied content object is validated **plain** (`Content::is_plain`: no marks, no islands, all `Para` lines) rather than markdown-decoded. A formatted wire content is rejected, not stripped — mirroring the `inline` precedent, checked in the same three places (coercion `CoercionError`; validation `plaintext::not_plain`, `TypeMismatch` fatality class; load-time literal import). An `inline: true` plaintext field additionally requires a single line. The load-time content caches (`default_content`/`example_content`) and the render-floor zero (the empty content) cover `plaintext` exactly as `richtext` — both are content leaves (`field_contains_content`)
- **`enum` domain validation.** An `enum` field (or the deprecated `enum:` modifier on `string`) coerces as a string; domain membership is a *value* check (`validation::enum_violation`), not a type check, so an out-of-domain string is well-typed but invalid. `type: enum` requires a non-empty `values:` list; `enum:`/`values:` on any type other than `string`/`enum` is a load error (`quill::field_parse_error`), closing the pre-promotion silent no-op. Both spellings populate one carrier (`FieldSchema::enum_values`) and project identically to `{type: string, enum: […]}`
- **Null short-circuits coercion.** A null value (`field:`, `field: null`,
  `field: ~`) passes coercion unchanged for *every* type — null ≡ absent, so
  it carries no data to coerce. The value reaches the render floor and
  zero-fills (authored › `default:` › type-zero) exactly like an omitted
  field
- **Bare scalars stringify into `string`/`richtext` fields.** A bare boolean,
  integer, or number written where a `string` is expected adopts its canonical
  scalar token (`true`, `47`, `1.0`) instead of failing — it is unambiguously
  text (null and collections are excluded); a `richtext` field then imports that
  token as its markdown source. The leniency is scoped to
  *document* payloads via the shared `scalar_as_string` predicate; a quill
  author's own `default:`/`example:` literals stay strict, so the blueprint
  keeps quoting ambiguous string literals

## Native validation

Validation is implemented by a native walker over `QuillConfig` in `quill/validation.rs`.

- Entry point: `QuillConfig::validate_document(&Document)` (dispatches to `validate_typed_document`)
- Returns `Result<(), Vec<ValidationError>>`
- Collects all errors (does not short-circuit)
- Emits path-aware errors for top-level fields and card fields
- Validates each card's `$kind` matches a known card kind
- Enforces `body.enabled: false` on the main card and on each card kind — body content for a body-disabled card emits `ValidationError::BodyDisabled` (whitespace-only bodies are treated as empty)
- `body.enabled: false` also drops `$body` from `build_transform_schema`'s `properties` for that kind — absent, not present-and-empty. This cascades into the Typst helper's generated `_qm-meta` address tables, so `form-field(field:)` rejects a `$body` address on that kind at compile time (see `PLATE_DATA.md`)
- **Null ≡ absent.** A present-null value (`field:`, `field: null`,
  `field: ~`) carries no data: it is treated exactly like an omitted field.
  It validates clean (no `TypeMismatch`) and zero-fills at render
  (authored › `default:` › type-zero — see
  [Zero-filled render](#zero-filled-render)).
- **`!must_fill` marker → non-fatal warning.** For every `!must_fill` marker
  present — root or nested, main card or composable card —
  `Quill::validate` emits `validation::must_fill` at **`Severity::Warning`**,
  regardless of whether the marker carries a value. It **never gates render**:
  a marked document renders fine (the cell zero-fills, or uses its suggested
  value). A strict consumer (e.g. an LLM authoring loop) treats any
  outstanding marker as "not done."
- **Absence semantics**: a missing (or present-null) field with a `default:`
  accepts the default; without a `default:` it zero-fills. Either way it
  validates clean. Field absence is **not surfaced as a diagnostic** —
  `Quill::validate` raises no completeness/`field_absent` code — so a merely
  incomplete (or present-null) document validates clean. The only authoring
  signal it raises is the non-fatal `validation::must_fill` warning.

Field-level type and presence errors render under a uniform shape —
field path, verbatim source token, schema declaration, and both exits
when applicable. See `ERROR.md` § "Validation message contract".

## Value sources and projections

Every field value comes from one of a small set of **sources**, ordered by
*commitment* — how strongly the value claims to be the real answer. This is the
**commitment ladder**:

| Rung | Source | Persisted into a `Document`? | Renders? |
|---|---|---|---|
| top | authored value | yes — it *is* the document content | yes |
| | `default:` | **never** by the engine — lives in the schema, interpolated only into the ephemeral render projection | yes — the fidelity value |
| | `example:` | only by [seeding](#document-seeding) | yes — once committed by seeding |
| floor | type-empty `zero` (`zero_value`) | never ([Non-persist invariant](#zero-filled-render)) | last resort |
| (signal) | `!must_fill` marker | yes — rides on the value as a YAML tag | yes — the marked value (suggested value or zero-fill); raises the non-fatal `validation::must_fill` warning |

A `default` is never written back into a document: it lives in `Quill.yaml`,
the render path interpolates it into the plate-JSON projection only, and seeding
deliberately omits it (persisting it would be redundant and would freeze it
against a schema change). The lone way a default's *value* becomes document
content is indirect: `blueprint()` emits it as literal text in its reference
*string* (the concrete default value, shippable as-is), and if a consumer authors from it and saves
it, that value is now ordinary **authored** content — the consumer committed
it, not the engine.

No surface owns a precedence *policy*; each **projection cuts the same ladder**
at a different rung, and the per-rung producers are shared (`zero_value` for the
floor; field ordering is declaration order, carried by the schema's ordered
field maps rather than a sort key):

| Projection | Per-field precedence | Floor | Output |
|---|---|---|---|
| render (fidelity) | authored › `default:` › zero | zero | plate JSON — [Zero-filled render](#zero-filled-render) |
| `blueprint` document | Endorsed: `default:`; Unendorsed: `example:` else zero, stamped `!must_fill` | zero (under the marker) | annotated string — [BLUEPRINT.md](BLUEPRINT.md) |
| seeding | `example:` › absent | (deferred to render floor) | committed `Document` — [Document seeding](#document-seeding) |
| add-card (into a document) | `$seed` overlay › `example:` › absent | (deferred to render floor) | a new composable `Card` — [Document seeding](#document-seeding) |
| editor (consumer-side) | authored / `default:` / missing (uncollapsed; `example:` as guidance) | — | a `Document`-payload × schema join the UI consumer performs directly (no engine projection); completeness comes from `Quill::validate` |

Two seams are deliberate, not uniform: on `blueprint` the floor still
zero-fills like every other projection (an Unendorsed cell with no `example`
carries bare null/empty under its marker), but the projection additionally
**stamps the `!must_fill` marker** on every Unendorsed field — the marker
rides *alongside* the value rather than replacing it; and `zero` is honestly
blank for every type except `enum`, whose zero is the first declared variant
(there is no empty enum member). Both are detailed below.

## Zero-filled render

**A document need not be complete to render** — render success is not a
completeness signal.
Shippability is the author's judgment; the engine's only hard requirement
is that the document be *well-formed* (values coerce). A `!must_fill` marker
and a present-null cell are both renderable — neither gates render.
Completeness is not surfaced as a diagnostic — `Quill::validate` raises no
completeness/`field_absent` code (see [Native validation](#native-validation));
the only authoring signal it raises is the non-fatal `validation::must_fill`
warning for each outstanding marker.

Rendering and the *completeness verdict* are orthogonal. The render path
(`compile_data` / `resolve_fields` in `quillmark::orchestration`) uses
**zero-filled render**: every absent schema field is resolved by precedence
— an authored value, else the `default:`, else the type-empty zero value
(`zero_value`, defined below) — in the plate-JSON projection that feeds the
backend **only, never in the persisted document**.

- **Incomplete is renderable.** A document that merely omits an Unendorsed
  field — or leaves it present-null — renders fine: the field is zero-filled
  in the projection. Such a document validates clean; completeness is not
  surfaced as a diagnostic.
- **Malformed is fatal.** The only malformed case is a value that cannot
  coerce to (or validate against) its declared type. Placeholders and null
  are *not* malformed: a `!must_fill` marker renders (using its suggested
  value or zero-filling) and raises only the non-fatal `validation::must_fill`
  warning, and a present-null cell zero-fills like an absent field.
- **Non-persist invariant.** The zero-fill lives only in the ephemeral
  projection and must never be written back. A type-empty value is
  indistinguishable from authored-empty, so persisting it would erase the
  absence signal (which keys on a field being unwritten) and blind a future
  schema migration to author intent.

The per-field zero value is honestly blank for every scalar type except
`enum`, whose zero is the first declared variant. An `object` with
`properties` is shape-valid only when every property is present, so its zero
is the object whose each property carries that property's zero (recursively).
It is the shared producer behind the render floor — for authored, blank, and
seeded documents alike (see [BLUEPRINT.md](BLUEPRINT.md)).

## Document seeding

**Seeding** builds a starter `Document` from the schema for editor consumers
("new document"): each field that declares an `example:` is committed, and
**every other field is left absent**. The seeding cascade is therefore
`example: → absent` — absent fields are never written; they are interpolated at
the compilation layer by [zero-filled render](#zero-filled-render) (`default:`,
else type-empty zero), exactly as for any authored document.

**Seed-commits-content.** A seeded richtext field (and the body) commits the
canonical **content** form, not the authored markdown string, so a seeded
document is content from birth — matching what an editor round-trip produces and
what storage embeds. The content is imported once at quill load into a
`#[serde(skip)]` companion cache on the schema (`FieldSchema::default_content` /
`example_content`, `BodyCardSchema::example_content`), a pure function of the
`Quill.yaml` bytes; seeding and the render floor read that cache rather than
re-importing markdown per document. The authored markdown literal is retained
untouched — it is the source of truth the schema emits and the blueprint prints;
the content is a derived projection of it.

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

The seed is **illustration-first**: a field carrying *both* an `example` and a
`default` commits — and therefore renders — its **`example`**, not its default.
So a seeded document is *not* the plain fidelity render. The fidelity render
path's "`default:` wins" rule applies to authored and blank documents, where no
`example` is ever present; in a seed the `example` is present, so it wins.

- **Composable cards** are seeded one instance per declared kind; `body.example`
  fills the body when bodies are enabled.
- **The main card** carries `$quill` and `$kind: main`, so a seed round-trips
  through Markdown like an authored document.
- **Provenance is untracked.** A seeded `example` is committed as ordinary
  authored content, indistinguishable from hand-authored input. Carrying no
  `!must_fill` marker, it reads as done — an Unendorsed field seeded with an
  `example` raises no `validation::must_fill` warning. Whether a field's
  value came from seeding or later authoring is not recorded; correctness
  and renderability do not depend on the distinction.

Seeding is the **filled-out twin of the blueprint**
([BLUEPRINT.md](BLUEPRINT.md) § "The blueprint and its filled-out twin"): the
blueprint shows the form to fill (`!must_fill` markers, `# e.g.` hints), while the seed
hands back a committed `Document` already carrying the `example:` values, the
rest deferred to the render floor for fidelity. It is the only "filled-out"
projection — there is no annotated `example` string. Implemented by
`Quill::seed_document` (with `seed_main` / `seed_card`) in `quillmark`.

### Per-document seed overlays (`$seed`)

Seeding a *new card into an existing document* — `Quill::seed_card(kind,
overlay)` — adds one more rung above `example:`: a curated, per-document
**overlay** read from the main card's `$seed` map. Per field the precedence is
**`$seed` overlay › `example:` › absent** (ordered by field declaration order), and `default`
/ `zero` stay deferred to the render floor exactly as everywhere else, so the
"never persist a `default`" invariant holds. The overlay is *sparse* — fields it
omits keep flowing from the schema seed, so it tracks an evolving quill rather
than freezing a snapshot. This is how a template author customizes the values
new cards spawn with; it lives in the document (a template *is* a document), so
markdown writers and MCP agents see the same source. See
[CARDS.md](CARDS.md) "Per-kind Seed Overlays" for the `$seed` mechanics. The
`example: → absent` document-seeding above is the `overlay = None` case (a fresh
document carries no `$seed`).

## Schema emission

`QuillConfig::schema()` returns the structural schema as `serde_json::Value`. It includes:

- Field types, constraints, and `enum`/`default`/`example` annotations
- `ui` hints on fields (`group`, `compact`, `multiline`, `title`) and on cards (`title`, plus the `groups` registry that `group` references). Field display order is not a hint: it is the key order of the emitted `fields`/`properties` maps (declaration order)
- `body` blocks on cards (`enabled`, `example`)

The schema describes only the user-fillable fields. The quill reference
(`name@version`, available from quill metadata) and card-kind
discriminators (the `card_kinds` map keys themselves) are document-level
metadata, not schema fields, and do not appear in `fields`.

`QuillConfig::schema_yaml()` is a YAML wrapper over the same value. The schema is pinned by serde attributes on `FieldSchema`, `CardSchema`, `UiFieldSchema`, `UiCardSchema`, and `BodyCardSchema` — there is no parallel mirror struct.

For LLM/MCP authoring, see [BLUEPRINT.md](BLUEPRINT.md) — `blueprint()` emits a document-shaped, pre-filled Markdown reference that's denser than schema for prompt-time use.

Top-level schema keys: `main`, optional `card_kinds` (map keyed by card name). `main` and each entry in `card_kinds` share the same `CardSchema` shape: `fields` (map keyed by field name), optional `description`, optional `ui`, optional `body`. Each `FieldSchema` includes `type`, optional `description`/`default`/`example`/`enum`/`values`/`inline`/`properties`/`items`/`ui`. `inline` is valid only on the prose types (`richtext`, `plaintext`). `values` declares an `enum` field's domain (required there); `enum` is its deprecated one-release alias on `string`. `items` (the element schema, itself a `FieldSchema`) is required on `array` fields and rejected elsewhere; `properties` is used by `object` fields (and by an array's `object`-typed `items`).

### `default` and `example`

`default` and `example` are both type- and shape-valid values, but they
encode opposite author intents:

- **`default`** is the value the *majority* of authors want. Because most
  authors want it, the field can be omitted entirely: at render time the
  default fills any field the document leaves out (an
  authored value always wins — `resolve_fields` in
  `quillmark::orchestration`). A field with a `default:` is **Endorsed** — the
  rendered value is shippable as-is — and the blueprint renders that concrete
  default value with a type-only annotation (no marker). Type-empty defaults
  (`default: ""`, `[]`, `false`, `0`) are the canonical way to mark a
  "skippable" cell.
- **`example`** matches the semantic and type *shape* of the desired
  value but is *not* the value most authors want. It documents shape, not
  the choice — so it never becomes the rendered value; it only surfaces in
  the blueprint's `# e.g.` line.

### Unendorsed vs. Endorsed fields

A field is **Unendorsed** when no `default:` is declared — the quill author
has endorsed no value, so the blueprint stamps the `!must_fill` marker to
ask an LLM or author to supply one. That is a *communication device on the
blueprint surface*, not a requirement: a missing (or present-null) Unendorsed
field zero-fills silently at render, and a surviving marker raises only the
non-fatal `validation::must_fill` warning, never a render gate. "Must-fill"
therefore lives solely on the blueprint/marker surface; the schema axis is
endorsement, not obligation.

A field is **Endorsed** when `default:` is declared; the rendered default
is shippable as-is (the author can keep or override it).

There is no separate `required:` axis; the presence or absence of
`default:` is the sole author choice per field. See
[BLUEPRINT.md](BLUEPRINT.md) for how the two cells render.

Identity fields (`name`, `version`, `backend`, `author`, `description`) live on the parent metadata object (Wasm: `Quill.metadata` getter; Python: `Quill.metadata`). Both bindings also expose `backend_id`/`backendId` directly; Python additionally exposes `quill_ref`, a derived `name@version` string.

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
