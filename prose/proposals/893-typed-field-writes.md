# Typed field writes: schema-carried types, one commit dispatch

> **Issue**: [#893](https://github.com/quillmark-org/quillmark/issues/893) ·
> **Status**: proposal — for discussion, not committed design.
> Symbol references are accurate on `integration/richtext` at time of writing.

## TL;DR

Make the schema — not the value, and not the method name — carry the field's
type at the write site, via three layers:

1. **One per-type dispatch** in core (`commit_value`), unifying
   `coerce_value_strict`'s arms with the strict write path behind a leniency
   mode — the single encoding of "what a type accepts."
2. **One typed writer per address**: `Card::commit_field(name, value,
   &FieldSchema)` — subsumes `set_field_richtext` and never grows a per-type
   sibling.
3. **A schema-bound editor** as the front door: `quill.editor(&mut doc)`
   resolves field types itself, so editors and MCP servers call one verb,
   `set`, and never pass `inline` or a type token.

The **write-method surface** stays **O(1) in corpus-backed types**: a new
content model (e.g. `Plaintext`) adds no writer in core, wasm, or Python. The
type is not otherwise free — it still touches the `FieldType` enum, the
`commit_value` arm, the separate `validate_value` dispatch, and its corpus
caches (see Growth story). `QuillValue` stays JSON-shaped; the wire is
unchanged.

## The observation

The `O(types × addresses)` sprawl the issue projects comes from encoding the
type in the **method name** (`set_field_richtext`, `updateCardRichtextField`,
…). The issue's own forces point at the alternative carrier: the schema is
the type's only authoritative home (`FieldType` in
`crates/core/src/quill/types.rs`, including constraints like `inline` that no
value tag could carry), and exactly one place already dispatches on it
(`QuillConfig::coerce_value_strict`, `crates/core/src/quill/config.rs`). So
route writes through that same dispatch, parameterized by who supplies the
schema — rather than inventing a second type carrier (value tags) or a
heuristic (shape detection).

## Layer 1 — one commit dispatch

Extract the per-type logic into a single function beside the coercion code:

```rust
enum Leniency { Render, Write }   // richtext-decode mode; today only that arm reads it

/// The one encoding of "what a type accepts and what gets stored".
fn commit_value(value: &QuillValue, schema: &FieldSchema, mode: Leniency)
    -> Result<QuillValue, CommitError>;
```

- `coerce_value_strict` becomes a thin caller with `Leniency::Render`;
  behavior unchanged.
- The deliberate strict-vs-lenient split (the scalar→string reduction the
  write path must not perform — noted at the richtext arm of
  `coerce_value_strict`) becomes an explicit `mode` branch **inside one
  richtext arm**, replacing the two open-coded sites that today encode "what
  a richtext value accepts" (`config.rs` coercion vs `edit.rs`
  `set_field_richtext`). Scalar arms ignore `mode` — `ed.set("qty", "3")`
  stores `3` either way — so `Leniency` is a per-arm knob, not a global
  strictness dial.
- Each `FieldType` arm declares a **storage policy**: *canonical-at-write*
  (richtext and future corpus models — the committed form carries identity
  marks) vs *coerce-at-render* (scalars — a typed write stores the coerced
  canonical, but nothing forces scalars through this path; plain `set_field`
  stays opaque). Null passes through, matching the null ≡ absent rule.

## Layer 2 — one typed writer per address

```rust
impl Card {
    /// Write-time commit: validate/normalize per the field's schema type and
    /// store the canonical form. The typed sibling of the opaque `set_field`.
    pub fn commit_field(&mut self, name: &str, value: impl Into<QuillValue>,
                        schema: &FieldSchema) -> Result<(), EditError>;
}
```

- `set_field_richtext(name, value, inline)` deprecates to a wrapper (builds a
  `richtext { inline }` `FieldSchema`, delegates) — not quite one line: it
  remaps the unified `CommitError` back onto
  `EditError::FieldRichtext{Decode,NotInline}`, whose names cross the binding
  boundary (`EditError::variant_name`) and must not drift. Its `inline: bool`
  parameter disappears from the API: the schema *is* the parameter.
- The verb `commit` matches the issue's vocabulary ("write-time commit") and
  names the two write disciplines: **`set_field` = store opaque, coerce at
  render** (keystroke-level state, data-in-flight) vs **`commit_field` =
  canonicalize now, fail now** (editor blur/save, agent writes). Neither
  discipline is forced on the other.
- `apply_field_richtext_change` (delta splicing) stays per-model by nature:
  incremental change bundles are content-model-native ops, not field writes,
  and a future `Plaintext` model brings its own delta vocabulary. It does not
  count against the O(1) goal.

## Layer 3 — the schema-bound editor

`commit_field` still asks the caller to fetch a `FieldSchema` per call. Every
consumer that wants typed writes (editor, MCP server) already holds the
resolved `Quill`/`QuillConfig` — it renders with it. Bind once:

```rust
let mut ed = quill.editor(&mut doc);   // TypedEditor<'a>: &mut Document + &QuillConfig
ed.set("subject", "Q3 results")?;      // schema: richtext(inline) → strict corpus commit
ed.set("qty", "3")?;                   // schema: integer → strict coerce, stores 3
ed.card(2)?.set("desc", corpus_json)?; // card kind → CardSchema → field type
ed.set_all([...])?;                    // batched, all-or-nothing, mirrors set_fields
```

- A field absent from the schema stores opaquely (mirrors `coerce_payload`'s
  passthrough for unknown fields; the field bag stays a bag).
- This answers the "Document lacks the resolved schema" force at the editor
  boundary — binding once, not a tag on every value, not a parameter on
  every call.

## Per-consumer surfaces

- **WASM editor** — the wasm `Quill` exposes a `schema` getter already; pass
  the handle per call (wasm-bindgen cannot hold a long-lived `&mut Document`
  inside a bound object): `doc.commitField(quill, name, value)` and
  `doc.commitCardField(quill, index, name, value)`. Two methods total for all
  current and future types; `updateCardRichtextField` deprecates into them.
  Values stay in the wire encoding the seam already speaks (corpus object |
  markdown string | scalar) — no new envelope.
- **Python** — `card.commit_field(name, value, schema)`, or the bound
  `quill.editor(doc)` mirroring Rust; slots into the
  [PROGRAMMATIC.md](../canon/PROGRAMMATIC.md) flow. Net-new, not a migration:
  Python has `set_field` / `set_fields` but no richtext field writer today, so
  it currently has no corpus-field write path at all.
- **MCP servers / LLM agents** — an LLM patches a field with a markdown
  string; the commit imports it to corpus at write, so identity marks live on
  the stored value from that moment, with no per-type tool schema. The MCP
  tool surface stays `set(field, value)`; `get_specs` already tells the model
  the types.
- **Batch generators (DB row → PDF)** — unchanged: opaque `set_field` +
  render coercion; they never pay the typed path.
- **Storage / diff / sync tooling** — commit-at-write makes the stored form
  canonical, so DTO diffs over committed fields are semantic rather than
  encoding noise, and patch/CRDT layers converge on one representation.
  Conditional on discipline: `set_field` (opaque) and `commit_field`
  (canonical) both write the same field, so a scalar stores as `"3"` or `3` by
  write path, not type — mixed paths add variance, not remove it.
- **Read-back parity** — the same dispatch can later grow a `project`
  operation (richtext → markdown string, plaintext → text), generalizing
  `field_markdown` into one schema-keyed `ed.get(name)` instead of per-type
  projections. Same O(1) argument; can ship separately.

## Rejected shapes

- **Value self-identification (type tag on `QuillValue`)** — breaks the
  "JSON-shaped, nothing more" invariant with crate-wide blast radius,
  complicates wire/DTO/`.qmd` round-trips and schema-less load, and still
  cannot carry schema-side constraints like `inline` — the schema gets
  consulted anyway.
- **Shape auto-detection** — ambiguous the moment a second corpus model
  exists (two canonical corpora are both objects; disambiguating requires a
  tag, see above). `decode_richtext_value`'s object-vs-string dispatch
  survives *inside* the richtext commit arm, as a codec detail rather than a
  type oracle.
- **Per-type writers** — the `O(types × addresses)` future the issue exists
  to avoid; every new writer also re-encodes the strict/lenient split by
  hand.

## Growth story (the acceptance test)

On the **write surface** it holds: adding a `Plaintext` corpus type adds no
method on `Card`, `Document`, the wasm surface, the Python surface, or any MCP
tool schema. The type is not otherwise O(1): it still adds one `FieldType`
variant, one `commit_value` arm, one `validate_value` arm (a second per-type
dispatch this proposal does *not* merge), and — if it carries schema literals —
its own corpus caching and render-floor commit (+ optionally a `project` arm
and delta ops). The claim earned is a flat write surface, not a flat per-type
cost.

## Migration

1. Extract `commit_value` from `coerce_value_strict`; coercion calls it with
   `Leniency::Render` (behavior-preserving; existing coercion tests pin it).
2. Add `Card::commit_field`; reimplement `set_field_richtext` over it and
   deprecate.
3. Add `TypedEditor` (core) and the binding methods; deprecate
   `updateCardRichtextField` / `setFieldRichtext` in wasm.
4. (Separate, optional) the `project` read-back generalization.

## Open questions

- Verb and type names: `commit_field` / `TypedEditor` vs alternatives
  (`set_field_typed`, `SchemaWriter`).
- Whether a typed scalar write stores the coerced canonical (proposed: yes —
  stored == rendered) or validates strictly but stores the authored form.
- Whether `TypedEditor` needs a fill-marking twin (`commit_fill`) or fill
  stays on the opaque path.
- Whether the wasm surface should also offer engine-side resolution from the
  `$quill` reference (`engine.commitField(doc, …)`) in addition to the
  explicit quill handle.
- `Leniency` placement: a whole-`commit_value` parameter, or a field on the
  richtext codec (`RichtextDecodeMode`) — since only that arm reads it.
- Whether `validate_value` (`validation.rs`) folds into the same dispatch, or
  stays a third, separately-drifting encoding of "what a type accepts."
- Error taxonomy: one `CommitError` remapped per caller (`EditError` /
  `CoercionError`), or a shared error both paths surface directly.
- Whether the editor routes *all* scalar writes through commit, so stored form
  is path-independent and the diff-stability claim holds.
