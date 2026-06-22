# Spike: replace the blueprint `<must-fill>` sentinel with the `!must_fill` tag

> **Status**: spike / proposal — not yet implemented.
> **Scope**: `crates/core` (blueprint, validation, SCHEMAS contract), with
> downstream touch on bindings and docs.
> **Touchstones**: [`BLUEPRINT.md`](../canon/BLUEPRINT.md),
> [`SCHEMAS.md`](../canon/SCHEMAS.md), [`QUILL_VALUE.md`](../canon/QUILL_VALUE.md),
> [`markdown-spec.md`](../references/markdown-spec.md) §3.4.

## TL;DR

The blueprint marks every Unendorsed cell with the **string** `<must-fill>`,
which *replaces* the value. The document model already carries a richer,
fully-plumbed **YAML tag** `!must_fill` that *annotates* a value — it rides
alongside real data and round-trips through parse, emit, wire, and storage.
The two mechanisms are disconnected: the blueprint never emits the tag, and
validation never reads it.

This spike proposes collapsing the two into one: the blueprint emits
`field: !must_fill <value>` where `<value>` is a renderable placeholder (the
field's `example`, else its type-zero), and validation gates on the tag
(`QuillValue::fill()`) instead of string-equality. The payoff is one concept
instead of two, a blueprint whose every cell is type-valid and renderable, and
a "must fill" signal that survives editing in structured (non-Markdown)
consumers instead of being a fragile magic string.

The open design question — *what value is appended, and does it subsume
`; delete-ok`* — is the substance of this document.

## Background: two parallel "must fill" concepts

### 1. The `<must-fill>` string sentinel (blueprint ↔ validation)

A literal string constant, `MUST_FILL_SENTINEL = "<must-fill>"`, defined in
`crates/core/src/quill/validation.rs:13`.

- **Produced** by the blueprint emitter for every Unendorsed cell (no
  `default:`): `crates/core/src/quill/blueprint.rs:425` (scalars/arrays),
  `:249` (inside a Markdown block scalar), and recursively for typed-table /
  typed-dict leaves.
- **Detected** by validation via exact string equality, *before* per-type
  checks: `crates/core/src/quill/validation.rs:455-468`. A survivor fires the
  fatal `validation::must_fill_sentinel`.
- **Tolerated** by coercion, which passes it through unchanged so validation
  (not coercion) owns the diagnostic (`SCHEMAS.md:38-40`).

Properties of the sentinel:

- It *is* the value. The cell holds no real data — `count: <must-fill>` is a
  string in an integer cell, kept legal only because the sentinel check
  short-circuits type validation.
- Detection is string-equality, so authoring the literal text as content
  requires quoting (`"<must-fill>"`) — see `BLUEPRINT.md` § "Writing the
  literal string `<must-fill>` as content".
- A document carrying it does **not render** meaningfully; the contract is
  "replace before shipping."

### 2. The `!must_fill` YAML tag (document model)

A first-class annotation on the value tree, already implemented end to end:

- **Value tree.** `QuillValue` is "an annotated value tree": every `Node`
  carries a `fill: bool` flag, the in-memory form of the tag
  (`crates/core/src/value.rs:18-47`). The JSON projection is deliberately
  **fill-free** — fill never reaches `as_json()` (`value.rs:1-10`). Accessors:
  `fill()`, `set_fill`/`with_fill`, `set_fill_at`, `fill_paths`,
  `nonroot_fill_paths` (`value.rs:241-295`).
- **Parse.** `document::prescan` recognises `!must_fill` in block style at any
  depth — top-level (`prescan.rs:265-327`), nested mapping keys (`:330-397`),
  and `- key:` sequence-item heads (`:199-231`). It strips the tag from the
  cleaned YAML (serde_saphyr drops custom tags), records `fill: true` on the
  `PreItem::Field` or a path in `nested_fills`, and rejects the tag on a
  mapping (`fill_target_errors`). `!must_fill` is the *only* fill tag; every
  other custom tag drops with `parse::unsupported_yaml_tag`
  (`prescan.rs:650-708`).
- **Emit.** `document::emit::emit_field` writes the tag back for scalars,
  empty/non-empty sequences, and null (`emit.rs:333-378`), with the sibling
  `emit_field_inline` for sequence-item heads (`:567-608`).
- **Wire / storage.** `PayloadItemWire` carries `fill: bool` plus a
  `nestedFills` path list across the binding boundary (`document/wire.rs:39-66`,
  round-trip test `:320-346`); the storage DTO persists the same.
- **Spec.** `markdown-spec.md:249-265` canonises the tag: scalars and
  sequences only, block style only, not on `$` keys, not on a bare sequence
  element, not inside a flow collection.

### The gap

Validation **never reads `fill`**. A grep for `.fill()` / `set_fill` across
`crates/core/src/quill/**` (where validation and the blueprint live) returns
nothing. Because the JSON projection is fill-free, a `!must_fill`-tagged value
validates today exactly as if the tag were absent:

```
title: !must_fill "Curriculum Vitae"   # parses, round-trips, validates OK — tag is inert
```

So the tag is presently a **round-trip-only annotation with no authoring
semantics**, while the string sentinel carries all the semantics through a
fragile channel. The blueprint and validation speak one language; the document
model speaks another.

## The problem with the status quo

1. **Two implementations of one idea.** A reader (human or LLM) sees
   `<must-fill>` in a blueprint but `!must_fill` in a hand-authored or
   wire-sourced document. Consumers must understand both.
2. **The sentinel is not type-valid data.** It hijacks the value cell, forcing
   a pre-type-check special case in validation and a pass-through hole in
   coercion. Every type (integer, boolean, datetime, enum, array, object leaf)
   has to tolerate a string where its type belongs.
3. **The signal is lost on edit in structured consumers.** A form/editor that
   round-trips through the wire (`CardWire`) has no field for "this string was
   the sentinel"; the only carrier is the literal text. The `fill` flag is
   exactly the structured channel that *does* survive — but the blueprint
   doesn't use it.
4. **A sentinel blueprint does not render.** `BLUEPRINT.md` § "Guarantees"
   already concedes the blueprint is parseable but not guaranteed to render.
   A tag-on-real-value blueprint *can* render (the value is type-valid),
   tightening the guarantee.

## Proposed change

Make `!must_fill` the single canonical placeholder mechanism.

### Blueprint emission

For an **Unendorsed** field, emit the tag in front of a renderable placeholder
value instead of the bare sentinel:

| Field (Unendorsed) | Today | Proposed |
|---|---|---|
| `name: string` | `name: <must-fill>  # string` | `name: !must_fill ""  # string` |
| `name: string`, `example: Jane` | `# e.g. Jane`<br>`name: <must-fill>  # string` | `name: !must_fill Jane  # string` |
| `count: integer` | `count: <must-fill>  # integer` | `count: !must_fill 0  # integer` |
| `severity: enum<low\|med\|high>` | `severity: <must-fill>  # enum<…>` | `severity: !must_fill low  # enum<…>` |
| `recipient: array<string>`, `example: [...]` | `# e.g. [...]`<br>`recipient: <must-fill>  # array<string>` | `recipient: !must_fill [...]  # array<string>` |

The appended value is the **placeholder cascade**: `example:` if present, else
the field's type-zero (`zero_value` from `SCHEMAS.md` § "Zero-filled render").
This makes the cell both a filled-in shape hint *and* a renderable value, while
the tag preserves the "replace me" intent. The `# e.g.` leading line becomes
redundant for the appended-example case (the example *is* the value now), a net
simplification.

### Validation

Replace string-equality detection with a tag check. In `validate_value`
(`validation.rs:445`), the document-context branch tests `value.fill()` (root)
and `value.fill_paths()` (nested) instead of comparing against
`MUST_FILL_SENTINEL`. `ValidationError::MustFillSentinel` keeps its code
(`validation::must_fill_sentinel`) and fatality; only the trigger changes. The
coercion pass-through hole (`SCHEMAS.md:38`) is **deleted** — the value is now
real typed data that coerces normally, and the tag rides on the fill flag,
which coercion already ignores.

### Endorsed fields and `; delete-ok`

Today the inline annotation encodes the cell state: Endorsed cells carry
`; delete-ok`, Unendorsed cells omit it and carry the sentinel
(`BLUEPRINT.md:94-98`). Under the proposal the **tag becomes the cell-state
signal** and the two states read as:

- **Endorsed** (has `default:`) → render the default, no tag. (Unchanged shape;
  `; delete-ok` may stay as-is or be dropped — see open question Q3.)
- **Unendorsed** (no `default:`) → render the placeholder *with* `!must_fill`.

One mental model — **`!must_fill` present → replace before shipping; absent →
shippable** — which is cleaner than the present "sentinel-in-value-cell XOR
`; delete-ok`-in-inline" split.

## Open design questions (with recommendations)

**Q1 — What value does the tag carry?**
*Recommendation:* `example:` › type-zero. It maximises shape information while
guaranteeing renderability and type-validity. (Alternative: always type-zero,
keeping `# e.g.` as the only example surface — simpler emitter, less helpful
cell.)

**Q2 — Markdown fields.** Markdown renders today as a `|-` block scalar with the
sentinel on its own indented line (`blueprint.rs:229-252`). But emit never
produces block scalars (`prefer_block_scalars: false`, `emit.rs:673-680`) and
`!must_fill` on a value emits inline (`key: !must_fill "..."`). Options: (a)
keep Markdown as the *one* type that still uses an inner placeholder line but
tag the field (`bio: !must_fill |-`) — needs a prescan/emit check that fill on a
block-scalar header round-trips; (b) emit Markdown fill inline as a quoted
scalar, losing the block shape in the blueprint. *Recommendation:* prototype (a)
first; it preserves the authoring affordance. This is the riskiest corner and
should anchor the spike's proof-of-concept.

**Q3 — Does `; delete-ok` survive?** If the tag fully encodes cell state,
`; delete-ok` is redundant on the inline. *Recommendation:* drop it in the same
change to avoid two signals for one fact — but that widens the blueprint diff
and the migration. Could be deferred to a follow-up. Decide explicitly.

**Q4 — Typed tables / dicts.** Unendorsed containers recurse to leaf sentinels
today (`blueprint.rs:298-311`, `:360-366`). The value tree already supports
nested fills via `set_fill_at` / `nested_fills`, and emit already writes nested
`!must_fill` (`emit.rs:436`, `:501-517`), so the machinery exists; the blueprint
emitter must switch from writing leaf sentinel strings to setting leaf fill
markers. The synthetic-row path (`- ` then per-property leaves) maps directly.

**Q5 — Round-trip / idempotence.** `BLUEPRINT.md` § "Guarantees" requires
blueprint → parse → emit → parse identity (`blueprint.rs:1051`). Block-style
`!must_fill` on scalars/sequences round-trips (covered by prescan/emit tests).
The new emitter must route through the same value-tree + fill-flag construction
the parser produces, so the existing round-trip test extends naturally. The
flow-collection / bare-element unsupported positions (`prescan.rs:408-422`) must
be avoided by the emitter — it already emits block style, so this is satisfied
by construction.

## Impact surface

| Area | File(s) | Change |
|---|---|---|
| Blueprint emitter | `crates/core/src/quill/blueprint.rs` | Emit `!must_fill <placeholder>`; build value tree with fill flags instead of writing the sentinel string; revisit `# e.g.` and `; delete-ok`. |
| Validation | `crates/core/src/quill/validation.rs` | Gate on `fill()` / `fill_paths()`; keep the `MustFillSentinel` variant + code; drop the string constant's authoring role. |
| Coercion | `quill::coerce_payload` (per `SCHEMAS.md:38`) | Remove the sentinel pass-through hole. |
| Canon | `prose/canon/BLUEPRINT.md`, `SCHEMAS.md` | Rewrite the sentinel sections (commitment ladder row, the "two seams", placeholder precedence table) around the tag. |
| Spec | `prose/references/markdown-spec.md` §3.4 | Already canonises the tag; note its new role as the blueprint placeholder. |
| Bindings | wasm / python / dotnet READMEs + any blueprint snapshot tests | Update example blueprints; the wire already carries `fill`. |
| Migration | `docs/migrations/<next>.md` | Author-visible: blueprints now show `!must_fill value`; quotes-to-author-the-literal guidance is dropped (the tag, not a string, is the signal). |

The infrastructure half (value tree, parse, emit, wire, storage, spec) is
**already done** — this is why the change is tractable. The work concentrates in
the blueprint emitter and the validation trigger.

## Compatibility & migration

- **Forward.** Old documents containing the literal `<must-fill>` string still
  exist in the wild (anything generated by a prior blueprint). Decide whether
  validation keeps recognising the *string* sentinel for one deprecation window
  (dual-trigger: `fill() || text == "<must-fill>"`) or breaks cleanly. *Rec:*
  dual-trigger for one minor version, with the string path warned/removed next.
- **Backward.** A `!must_fill`-bearing blueprint parsed by an *older* engine
  drops the tag (treats it as data) — that engine renders the placeholder
  silently instead of erroring. Acceptable for a forward-only authoring surface.
- **The literal-string escape hatch disappears**, which is a simplification:
  with the tag as signal, the string `<must-fill>` is ordinary content needing
  no quoting.

## Risks

1. **Markdown block-scalar fill** (Q2) is the only place the existing tag
   plumbing isn't obviously sufficient. Mitigate by prototyping it first.
2. **Blueprint snapshot churn.** Every bundled quill's blueprint changes
   shape; binding/docs snapshots and the `print_blueprint` example output move.
   Mechanical but broad.
3. **Two-signal confusion if `; delete-ok` stays** (Q3). Resolve the
   redundancy deliberately rather than letting both linger.
4. **Dual-trigger validation** during deprecation keeps the string constant
   alive a little longer; keep it strictly time-boxed.

## Recommendation

Proceed, in three phases:

1. **Proof-of-concept (the spike core).** Wire the blueprint emitter to set
   fill flags + placeholder values for scalar/array/enum Unendorsed fields;
   switch validation to `fill()`. Prove blueprint → parse → emit → parse
   identity and that an all-Unendorsed blueprint now *renders* (zero-filled).
   Resolve Q2 (Markdown) here.
2. **Containers + cleanup.** Extend to typed tables/dicts (nested fills),
   resolve Q3 (`; delete-ok`), delete the coercion pass-through hole.
3. **Docs + migration + bindings.** Rewrite `BLUEPRINT.md` / `SCHEMAS.md`
   around the tag, refresh snapshots, write the migration guide, and retire the
   string sentinel (drop the dual-trigger).

The decisive insight is that this is **mostly a consolidation, not a new
feature**: the canonical `!must_fill` mechanism already exists and is fully
plumbed; the spike connects the blueprint and validation to it and retires the
parallel string sentinel.
