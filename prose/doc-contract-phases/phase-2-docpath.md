# Phase 2 — canonical `DocPath`

> **Gate**: grammar design — the edge cases below are the actual work.
> The wire format barely changes: the target grammar is what
> `crates/core/src/quill/validation.rs:297,311` already emits. This phase
> is a type, a parser export, a retrofit, and doc fixes.

## Goal

One typed document-model path with exactly one serializer and an exported
parser, so no consumer ever regexes a `Diagnostic.path` string. Today paths
are `format!()`-assembled per emit site: the engine ships two card shapes
(`cards[<i>]` whole-card, `cards.<kind>[<i>].<field>` card field), the
editor guesses a third (`$cards.<i>.<field>` — the plate-JSON array name in
a document-model path), and ERROR.md's lone example drops the `<kind>`
segment the engine emits.

## Grammar

- Card fields are kind-qualified: `cards.<kind>[<i>].<field>`.
- The unknown-kind whole-card case stays `cards[<i>]` — there is no kind to
  qualify with; this is the only bare-index form.
- Main-card fields are bare names (`recipient`); the main body is
  `main.body`.

Edges to resolve before the type lands (each becomes a fixture in
phase 4): card bodies (`cards.<kind>[<i>].body`), `$ext` addressing (or an
explicit "not addressable" ruling), nested container indices inside a field
value.

## Work

- Core: `DocPath` — `Main | Card{kind, index} | Field(name) | Index(i) |
  Body` — with the workspace's only serializer and parser. Every emit site
  (`validation.rs`, edit mutators, coercion) constructs `DocPath`, never a
  string.
- Retrofit `path` onto edit diagnostics — the half deferred from phase 1.
  This needs call-site plumbing, not just serialization: `IndexOutOfRange`
  must learn which card array, `FieldConform` which card.
- WASM: export the parser (string ↔ structured form). Whether `path`
  crosses the boundary as string-plus-parser only or also as a structured
  object is settled here.
- Canon: fix ERROR.md's example (`cards[2].author` →
  `cards.<kind>[2].author`); state the two path namespaces loudly —
  plate JSON delivers sigiled `data.$cards` (template-author contract,
  `CARDS.md`), document-model paths use unsigiled `cards`. No rename:
  renaming the plate key breaks every existing Quill template, a blast
  radius far beyond the editor.

## Acceptance

- Grepping the workspace for `format!` producing a document-model path
  returns nothing; all sites go through the `DocPath` serializer.
- The exported parser round-trips every emitted path; the editor's
  `diagnostics.ts` routes card diagnostics through it with no best-effort
  guessing left.
- ERROR.md's example is correct and the `$cards`/`cards` namespace split is
  canon.

## Status

**Shipped**: `DocPath` (`crates/core/src/path.rs`) — the type, the one
serializer, the `FromStr` parser; the `validation.rs` + `compose.rs` retrofit
(every document-model path now built through `DocPath`, no `format!`); the
WASM `parseDocPath` / `formatDocPath` export (structured `DocPathSeg[]`);
ERROR.md's corrected example, the `DocPath` grammar section, and the
`$cards`/`cards` namespace split in canon.

**Deferred** — a later slice, unblocked now that the type exists:

- **Per-variant `path` on edit (mutator) diagnostics.** The batched twins keep
  the bare field-name `path` phase 1 set; the single mutators still carry none.
  Attaching kind-qualified paths needs the call-site plumbing this doc names —
  `IndexOutOfRange` learning its card array, `FieldConform` its card — threaded
  through ~50 binding call sites, so it lands on its own.
- **The coercion namespace.** `CoercionError` paths are schema-space
  (`card_kinds.<kind>.<field>`, bare field names), not document-model
  (`cards[<i>]`) anchors, and are dropped where coercion becomes
  `EditError::FieldConform`. They stay their own namespace; folding them (or
  ruling them out) rides with the edit-diagnostic slice above.
