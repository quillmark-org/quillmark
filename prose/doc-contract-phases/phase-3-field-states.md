# Phase 3 — `fieldStates()`, the resolved-value view

> **Gate**: consumer shape — settled **lean** against the editor's
> architecture. The view answers one question: the value the render projection
> uses and the rung that produced it. It does not absorb `validate()`.

## Goal

An engine projection that makes field resolution observable data instead of an
inferred behavior chain: per declared field,
`{ value, source: "authored" | "default" | "zero" }`, nested `main` + `cards`
(the document's own two-level structure, fields in declaration order), the body
rowed as `$body` (`enabled` ≡ declared; a body-disabled kind carries no row —
collision-proof, since a user field can never be `$`-prefixed).

The editor's default-rung shim — schema `default:` bound as a control
placeholder, a second implementation of rung 2 — deletes; `SCHEMAS.md`'s
editor row flips the consumer-side payload × schema join to a non-goal. First
named consumer beyond the shim: the editor's `RESTING_FIELDS` design, whose
resting typography typesets the *resolved* value.

## Constraints

1. **Built from the shared rung producers** — `resolve_value_sourced`, the
   same commitment cut as the render projection. Canon's rule is that no
   surface owns a precedence *policy* (`SCHEMAS.md` § "Value sources and
   projections"); this is an acceptance criterion, not a guideline.
2. **No diagnostics in the view.** `validate()` stays the diagnostics door.
   The editor merges three producers (`validate()`, session warnings, render
   errors) and re-keys positional → stable session ids regardless; engine-side
   bucketing of one producer deletes no consumer code while dragging
   document- and card-level slot semantics into a value view.
3. **Values only.** `example:` and other schema guidance read from
   `quill.schema`, where the consumer already reads controls, labels, groups,
   and enum domains. The view does not duplicate schema data.
4. **WASM only.** Python parity waits for a Python consumer naming a call
   site — the Tier-1 scope cut (#970) applies to new surfaces too.
5. **Source is top-level only** — one rung per field; the zero-fill inside an
   authored dict or array is a value detail, not a per-subpath source.

## Work

- Core: slim the projection to rows of `{ value, source }` — the per-row,
  card-level, and document-level diagnostics slots, `$seed` routing, and
  `example` come out.
- WASM: `fieldStates()` returns the lean shape; the TS types shrink to match.
- Python: remove `field_states`.
- Canon: the `SCHEMAS.md` editor-row flip stands — it lands in the same
  release as the surface.
- Migration: re-cut the unreleased 0.96 entry to the lean shape.

## Acceptance

- The editor's placeholder shim is deletable: resolved value + provenance come
  from `fieldStates()`; completeness and errors keep coming from `validate()`.
- The value is byte-for-byte the render projection's on every fixture (asserted
  against `compile_data`); `source` is the rung that same projection cut, held by
  construction — both halves read the one `resolve_value_sourced` producer — not
  a separate cross-check, since the render path keeps the value and drops the tag.
- `SCHEMAS.md` names the consumer-side join a non-goal.

## Status

**Shipped** the lean shape: `field_states.rs` rows are `{ value, source }` over
the shared `resolve_value_sourced` producer; the WASM `FieldState` /
`MainFieldStates` / `CardFieldStates` / `FieldStates` types dropped
`diagnostics` and `example`; Python `field_states` removed; `validate()` stays
the diagnostics door; the `SCHEMAS.md` editor-row flip re-lands on the lean
surface. The rich projection (per-row bucketing, `$seed`/card slots, `example`,
Python parity) is preserved at `d079280` — resurrect from that commit when a
consumer names a cut piece.

## Deferred

- **Per-subpath provenance** inside authored containers — a rung per leaf of a
  nested dict/array. Needs evidence that a container-aware editor wants it; a
  phase-5 fixture pins top-level-only meanwhile.
- **Diagnostics bucketing / a one-call view** — returns only with evidence
  that a consumer deletes code by it (constraint 2's bar).
