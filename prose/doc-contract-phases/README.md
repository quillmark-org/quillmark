# Document-contract rework — phase plan

> **Tracker**: [#1005](https://github.com/borb-sh/quillmark/issues/1005)
> (supersedes #1004, spun out of #1003). Plans-tier working doc per
> [`prose/README.md`](../README.md); removed once the rework lands.

## Problem

One disease with several sites: the engine↔editor boundary carries surfaces
that are not contracts. The editor (`borb-sh/quillmark-editor`) is a
version-pinned downstream consumer of `@quillmark/wasm`; every non-contract
surface is one it reverse-engineers from tests and probes, and an engine change
to any of them breaks the pinned consumer while upstream CI stays green.

- **Error identity in prose** — mutator failures carry identity as an
  `[EditError::…]` message prefix with `code: None`; `edit::` absent from
  ERROR.md's namespace list, so "consumers route on codes" is false for the
  edit surface. *Cured — phase 1.*
- **Per-site path assembly** — `Diagnostic.path` strings `format!()`-assembled
  per emit site; the engine emits two card shapes, the editor guesses a third.
  *Cured for diagnostics — phase 2.*
- **A second, untyped address grammar** — geometry (`FieldRegion.field`,
  `ContentHit`) crosses the boundary as `$cards.<kind>.<ordinal>.<field>` with
  **per-kind ordinals**: different syntax *and* different index semantics from
  document-model paths. This is the boundary's highest-traffic parsed string —
  the editor parses it on every diagnostic route and pointer interaction and
  bridges ordinals→absolute indices itself. *Open — phase 2's remaining work.*
- **Consumer-side resolution** — the commitment ladder is engine semantics, but
  the editor shims the default rung (schema `default:` bound as a control
  placeholder) because no engine projection exposes resolved values. *Phase 3.*
- **Untyped reach-arounds** — the typed surface is narrower than what its
  consumer reads: `QuillCardUi` omits `groups` (the editor casts around it),
  `ContentIsland.props` is `unknown` (the codec keeps hand-written shapes the
  surface does not pin). *Phase 4.*
- **No executable cross-repo check** — nothing runs the same expectations in
  both repos. *Phase 5.*

## Decision

The editor stays downstream and pinned. The ladder *semantics* stay canon
(`SCHEMAS.md`: `authored › default: › zero`, null ≡ absent, the non-persist
invariant). The *surfaces* pivot: the engine exports real contract surfaces —
one address grammar, one diagnostic envelope, a resolved-value view, a typed
surface with no reach-arounds — and freezes them behind an executable
cross-repo conformance suite at 1.0.

**0.96 is the one coordinated break.** Re-grammaring geometry addresses breaks
the editor's live traffic, so everything the editor must absorb — codes, the
prefix deletion, DocPath on both lanes — rides that single migration. Phases 3
and 4 are additive and ship with it or after. Pre-1.0 breaks cost a
`docs/migrations/` entry; the consumer never builds against HEAD, so the
conformance suite is the only mechanism that keeps both repos honest.

## Phases

| Phase | Gate | Delivers |
|---|---|---|
| [1 — `edit::` codes](phase-1-edit-codes.md) | none — shipped | Namespaced `code` on every mutator diagnostic, WASM + Python; prefix deleted; null ≡ absent ratified |
| [2 — one address grammar](phase-2-docpath.md) | geometry translation seam | `DocPath` on every boundary address, geometry included; mutator-path retrofit; coercion-namespace ruling |
| [3 — `fieldStates()`](phase-3-field-states.md) | shape settled — lean | `{value, source}` per field; consumer-side join a non-goal |
| [4 — typed-surface completion](phase-4-typed-surface.md) | editor's boundary ledger | `QuillCardUi.groups`; typed island props |
| [5 — conformance suite](phase-5-conformance.md) | phases 1–4 settled | Cross-repo fixture set + `contractVersion`; freezing it *is* the 1.0 release |

## Dependency edges

Edges, not a release train — each phase ships when ready:

- **1 → 2** *(discharged)*: phase 1 shipped codes without paths; all path
  work — including the mutator retrofit — rides the `DocPath` type.
- **2 ⇸ 3**: no edge. The lean view carries no diagnostics, so no paths; the
  two phases couple only through the release that bundles them.
- **4** is independent of 1–3 and additive (no break).
- **1, 2, 3, 4 → 5**: fixtures assert codes, paths, sources, values, and typed
  shapes, so they are written against the settled surface — earlier means
  written twice.

## Non-goals (whole rework)

- Changing ladder semantics or reopening null ≡ absent as tri-state.
- Moving the editor in-tree.
- Renaming the plate-JSON `$cards` key or the plate-space per-kind address
  grammar (template-author blast radius). Plate-space addressing stays
  template-side and documented; phase 2 stops it from *crossing the consumer
  boundary*, which is the actual defect.
- Diagnostics bucketed inside `fieldStates()`, `example:` on the view, Python
  `field_states` parity — cut from the rich shape implemented at `d079280`;
  each returns only when a consumer names a call site
  ([phase 3](phase-3-field-states.md) §Deferred).
- Per-subpath provenance inside authored containers (consumer-evidence gate).
