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

## Status

**Shipped** the engine side. `QuillCardUi` gains
`groups?: Record<string, QuillGroupUi>` — the canonical mapping form
(`UiCardSchema`'s serializer) the wire already carried, just untyped.
`ContentIsland.props` is typed per the open `type` as `TableProps` /
`ImageProps` (with `TableCell`), an open discriminated union mirroring
`ContentMark` so an unschematized island type keeps opaque `props` — the shapes
`KnownIslandType` already owns engine-side, now pinned at the boundary. (Like
`ContentMark`, the open arm means TS does not auto-narrow `props` on a
discriminant check; the value is single-source shapes, not narrowing ergonomics.) All four
new types re-export from `@quillmark/wasm` and are held present by the
`runtime.types.test-d.ts` drift guard. The audit found no third gap:
`title` / `group` / `compact` / `multiline` are the only other emitted `ui`
keys, all already typed. Canon home: [CONVERT.md](../canon/CONVERT.md) § Island
props. The consumer-side half — the editor's `groups` cast and its codec's
hand-written island schemas — deletes in `quillmark-editor` against these types,
as phase 2's `parsePath` removal does; the ledger seams retire there.
