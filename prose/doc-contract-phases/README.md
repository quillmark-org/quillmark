# Document-contract rework — phase plan

> **Tracker**: [#1005](https://github.com/borb-sh/quillmark/issues/1005)
> (supersedes #1004, spun out of #1003). Plans-tier working doc per
> [`prose/README.md`](../README.md); removed once the rework lands.

## Problem

The editor (`borb-sh/quillmark-editor`) is a version-pinned downstream
consumer of `@quillmark/wasm`. Three engine surfaces grew as implementation
details, so the editor reverse-engineered them from tests — and any engine
change to them breaks the pinned consumer while upstream CI stays green:

- **Error identity** — mutator failures carry identity as an `[EditError::…]`
  message prefix with `code: None`
  (`crates/bindings/wasm/src/engine.rs:1829`,
  `crates/bindings/python/src/errors.rs:14`). `edit::` is absent from
  ERROR.md's namespace list, so "consumers route on codes, not on a type"
  is false for the whole edit surface.
- **Addressing** — `Diagnostic.path` strings are `format!()`-assembled per
  site (`crates/core/src/quill/validation.rs:297,311`); the engine emits two
  card-path shapes, the editor guesses a third, and ERROR.md's lone example
  matches none of them.
- **Resolution** — the commitment ladder (`authored › default: › zero`) is
  engine semantics, but the editor re-implements it in TypeScript because
  `SCHEMAS.md` blesses a consumer-side payload × schema join as the editor
  projection.

## Decision

The editor stays downstream and pinned. The ladder *semantics* stay as canon
(`SCHEMAS.md`: `authored › default: › zero`, null ≡ absent, the non-persist
invariant). The *surfaces* pivot: instead of ratifying the observed shapes
(#1004), the engine exports real contract surfaces — codes, a canonical path
type, a resolved-field view — and freezes them behind an executable
cross-repo conformance suite at 1.0. Pre-1.0 breaks cost a
`docs/migrations/` entry; the consumer never builds against HEAD, so the
conformance suite is the only mechanism that keeps both repos honest.

## Phases

| Phase | Gate | Delivers |
|---|---|---|
| [1 — `edit::` codes](phase-1-edit-codes.md) | none — ships now | Namespaced `code` on every mutator diagnostic, WASM + Python; prefix deleted; null ≡ absent ratified |
| [2 — canonical `DocPath`](phase-2-docpath.md) | grammar design | One path type, one serializer, exported parser; path retrofit at every emit site; `$cards` namespaces documented |
| [3 — `fieldStates()`](phase-3-field-states.md) | editor call sites | Resolved-field view with `source` provenance; consumer-side join becomes a non-goal |
| [4 — conformance suite](phase-4-conformance.md) | phases 1–3 settled | Cross-repo fixture set + `contractVersion`; freezing it *is* the 1.0 release |

## Dependency edges

Edges, not a release train — each phase ships when ready:

- **1 → 2**: phase 1 deliberately ships codes *without* paths; ad-hoc
  `format!()` paths on edit diagnostics would recreate the disease at a new
  site. All path work — including the edit-diagnostic retrofit — rides with
  the `DocPath` type.
- **2 → 3**: diagnostics inside the `fieldStates()` view carry
  `DocPath`-grammar paths.
- **1, 2, 3 → 4**: fixtures assert codes, paths, sources, and values, so
  they are written against the settled surface — earlier means written
  twice.

Phase 1's null ≡ absent ratification is a standalone doc edit with no
dependencies in either direction.

## Non-goals (whole rework)

- Changing ladder semantics or reopening null ≡ absent as tri-state.
- Moving the editor in-tree.
- Renaming the plate-JSON `$cards` key (template-author blast radius);
  the two path namespaces are documented instead.
