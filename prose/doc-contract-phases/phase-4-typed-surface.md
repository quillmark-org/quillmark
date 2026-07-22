# Phase 4 — typed-surface completion

> **Gate**: the editor's boundary ledger (quillmark-editor
> `prose/canon/DOCUMENT_MODEL.md` § Stability seams) — both items are named
> there, with live call sites shipping today.

## Goal

The typed WASM surface is as wide as what its consumer reads. A consumer
casting around a type to reach schema JSON is a contract gap of the same
species as an unstated grammar: behavior the pinned consumer depends on that no
surface pins. Two known instances:

1. **`QuillCardUi` omits `groups`.** The hand-written interface
   (`crates/bindings/wasm/src/engine.rs`) carries only `title?`; real quills
   carry `ui.groups`, and the editor reads group order by casting the schema
   JSON around the type (quillmark-editor `src/lib/visual/structure.ts`).
2. **`ContentIsland.props` is `unknown`.** The editor's codec keeps
   hand-written table/image props schemas the surface does not pin.
   `KnownIslandType` (`crates/content/src/island.rs`) already owns per-type
   props dispatch (`normalize_props`, `cell_marks`); the canonical shapes
   exist engine-side, untyped at the boundary. Prerequisite-adjacent to
   island/table authoring (quillmark-editor #16).

## Work

- **Audit**: diff the schema JSON the editor consumes against the hand-written
  TS interfaces (`QuillCardUi`, `QuillFieldUi`, `QuillCardSchema`); every
  consumed key gets typed or gets an explicit not-contract ruling.
- WASM: widen `QuillCardUi` with `groups`; export per-type island props
  typings (`TableProps` / `ImageProps`, shapes from `KnownIslandType`'s
  canonical forms) and narrow `ContentIsland.props` accordingly.
- Canon: the boundary's island-props shape gets a home (CONVERT.md or
  CARDS.md, wherever islands live); the editor's ledger drops both stability
  seams once typed.

## Acceptance

- The editor deletes its `groups` cast; the codec's island schemas re-export
  boundary types instead of tracking them by hand.
- Additive only — no break. Ships with 0.96 or after, independent of
  phases 1–3.
