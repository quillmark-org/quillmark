# Phase 3 — `fieldStates()`, the resolved-field view

> **Gate**: consumer evidence. The shape (flat vs. nested main + `cards`,
> whether `example:` rides along as guidance) is settled against the
> editor's actual call sites, not engine-side taste.

## Goal

An engine projection that makes field resolution observable data instead of
an inferred behavior chain: per field,
`{ value, source: "authored" | "default" | "zero", diagnostics }`.
The editor deletes its TypeScript re-implementation of the commitment
ladder, and `SCHEMAS.md`'s editor row — today "a `Document`-payload × schema
join the UI consumer performs directly (no engine projection)" — flips to a
non-goal. "removeField → default renders" stops being a four-step inference
from tests: re-read the view, the field reports `source: "default"`.

## Constraints

Three, or this surface becomes the drift it exists to kill:

1. **Built from the shared rung producers** — `zero_value` for the floor,
   the same precedence cut as the render projection. Canon's rule is that
   no surface owns a precedence *policy* (`SCHEMAS.md` § "Value sources and
   projections"); this is an acceptance criterion, not a guideline.
   Otherwise `fieldStates()` is another ladder implementation, not another
   projection of the one ladder.
2. **It does not fully collapse `validate()`.** `UnknownCard` and
   `BodyDisabled` are document/card-level, not per-field — the view carries
   a document- and card-level diagnostics slot for them, or the editor
   keeps calling `validate()` and the one-call claim softens accordingly.
   Bodies pose the same question: a card body has authored/absent state but
   is not a field; decide whether the view carries a body row.
3. **Shape from evidence.** Flat map vs. nested, `example:` included or
   values-only, Python parity — each settled by reading the editor's call
   sites before the WASM signature freezes.

## Work

- Core: the projection, assembled from the existing rung producers and the
  validation pass; diagnostics inside the view carry phase-1 codes and
  phase-2 `DocPath` paths.
- WASM: `fieldStates()` on the engine surface. Python parity decided under
  constraint 3.
- Canon: the `SCHEMAS.md` editor-row flip lands *with* the surface, not
  before — canon must not declare the only working mechanism a non-goal
  while its replacement is unshipped.

## Acceptance

- The editor's TS ladder is deletable: every value + provenance +
  completeness question it answered is answered by `fieldStates()` (plus
  `validate()` only if constraint 2 lands on the softer variant).
- `source` provenance agrees with the render projection on every fixture —
  same rung, same value, byte-for-byte on the zero floor.
- `SCHEMAS.md` names the consumer-side join a non-goal.
