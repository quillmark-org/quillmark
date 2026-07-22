# Phase 2 — one address grammar (`DocPath`)

> **Gate**: the geometry translation seam — where plate-space per-kind keys
> become `DocPath` (and back) inside the session. The diagnostic-lane grammar
> is settled and shipped.

## Goal

One typed document-model path with exactly one serializer and an exported
parser, on **every** address that crosses the WASM boundary — diagnostics and
geometry both — so no consumer ever regexes a path string.

The boundary today speaks two grammars with different index semantics:

- **Diagnostics** — `Diagnostic.path` emits `DocPath`
  (`cards.<kind>[<i>].<field>`, absolute indices). Shipped, zero live parsed
  traffic: the reference fixture's `validate()` is empty.
- **Geometry** — `FieldRegion.field` / `ContentHit` emit
  `$cards.<kind>.<ordinal>.<field>` with **per-kind ordinals**, the plate-space
  grammar minted for the `_qm-plaintext` table
  (`crates/backends/typst/src/helper.rs:323`). This lane carries all the live
  traffic: the editor parses it on every diagnostic route and pointer
  interaction (`quillmark-editor src/lib/visual/diagnostics.ts`) and bridges
  per-kind → absolute itself (`perKindCardIndex`), on the assumption — wrong
  since the diagnostic retrofit — that the boundary has one grammar.

Typing the diagnostic lane while geometry stays stringly is the disease
surviving at the busier site.

## Grammar

- Card fields are kind-qualified: `cards.<kind>[<i>].<field>`; `<i>` is the
  absolute card index.
- A whole-card case whose `$kind` has no schema — absent, or present but not a
  declared card kind — is `cards[<i>]`, the only bare-index form. Geometry
  never emits it: an unschematized kind renders no fields.
- Main-card fields are bare names (`recipient`); the main body is `main.body`;
  card bodies are `cards.<kind>[<i>].body`.
- Grammar edges — card bodies, `$ext` addressing (or an explicit "not
  addressable" ruling), nested container indices — each becomes a phase-5
  fixture.

## Shipped

- `DocPath` (`crates/core/src/path.rs`) — the type, the one serializer, the
  `FromStr` parser.
- The `validation.rs` + `compose.rs` retrofit: every document-model path built
  through `DocPath`, no `format!`; fill paths resolve the card's `$kind`
  against the schema, so an unschematized kind anchors bare-index.
- WASM `parseDocPath` / `formatDocPath` (structured `DocPathSeg[]`).
- Canon: ERROR.md's corrected example and `DocPath` grammar section; the
  `$cards`/`cards` namespace split.

## Remaining

1. **Geometry-lane unification** — the phase's completion criterion.
   `regions()` / `fieldBoxes(field)` / `fieldAt` / `positionAt` / `locate`
   speak `DocPath` strings in both directions (returns and arguments). The
   plate-space per-kind grammar is untouched template-side — the
   `_qm-plaintext` table contract stays — but it stops crossing the consumer
   boundary: the session holds the composed document and owns the
   plate-space ↔ `DocPath` translation, so per-kind ordinals never reach a
   consumer. Editor payoff: `parsePath` and the ordinal bridge delete; every
   address routes through `parseDocPath`. This is the 0.96 break's
   centerpiece — the migration entry leads with it.
2. **Mutator-path retrofit** — every edit diagnostic carries a `DocPath`:
   `IndexOutOfRange` learns which card array, `FieldConform` which card,
   threaded through the binding call sites. Completes the one-envelope
   invariant: every diagnostic from every producer is
   `{severity, code, message, path}`-addressable. (The batched twins carry a
   bare field-name `path` from phase 1; the single mutators carry none.)
3. **Coercion-namespace ruling** — `CoercionError` paths are schema-space
   (`card_kinds.<kind>.<field>`, bare field names), not document-model
   anchors. Fold them into `DocPath` space, or name them a distinct schema
   namespace in ERROR.md; either way the ruling is written, not implied.

## Acceptance

- No address string crosses the WASM boundary outside the `DocPath` grammar;
  grepping the workspace for `format!` producing a boundary address returns
  nothing.
- The exported parser round-trips every emitted path — diagnostic and
  geometry lanes; the editor routes both through it with no best-effort
  parsing left.
- ERROR.md states the grammar, the namespace split (plate-space `$cards`
  template-side; `DocPath` consumer-side), and the coercion ruling.
- The unreleased migration guide covers the geometry re-grammar.
