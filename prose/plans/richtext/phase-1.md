# Phase 1 — type + codecs, engine-off

`RichText` and its markdown projection, exercised in isolation. This is where
the canonical serialization and mark semantics — what storage and the seam
still commit to — were frozen, gated on the [Spike-A contract](phase-0.md).

**Status: landed.** Crate `crates/richtext` (`quillmark-richtext`), unit +
property test suite. For the crate's shape and its place in the workspace
today, see ARCHITECTURE.md — phase 2 later inverted the dependency arrow
(`core` depends on it, not the other way around); that revision belongs to
phase-2.md, not here.

## The freeze — what storage and the seam still commit to

- **Shape.** Canonical JSON with every object key recursively sorted, so the
  bytes are independent of `serde_json`'s `preserve_order` feature. Pinned by
  a golden-bytes test; changing it bumps the schema version (see
  DOCUMENT_STORAGE.md for the version-bump playbook).
- **Mark set** (open): `strong · emph · underline · strike · code · link{url}`
  (formatting) + `anchor{id}` (zero-width identity) + `unknown{tag, attrs}`
  (round-trips opaque, never reuses a reserved built-in `type` name).
  Normalization is idempotent: same-kind formatting marks union on
  adjacent/overlap; different kinds overlap freely (Peritext-style, not a run
  model); identity marks never merge; sort by `(start, end, kind-ord, attrs)`;
  formatting-mark edges never sit on a `\n`.
- **Coordinates** count Unicode scalar values throughout.
- **`Line.continues`** — a within-block hard line break, distinct from a
  paragraph boundary. A model-shape decision and a one-way door: it exists so
  phase-2 emit can tell a `#linebreak()` from paragraph spacing.
- **`ListItem { ordered, start, ordinal }`** — positional item identity, no
  minted ids.
- **Islands** — tables are block islands, images inline slots, both
  `Lossless`; ids minted sequentially (`isl-N`) so import stays a pure
  function. Real per-creation minting, the actual determinism boundary, is
  Phase 4.

The freeze was built from the written contract, then independently reviewed
against the phase-0 spike code; every finding from that review is fixed and
pinned by tests (golden bytes, round-trip, and a property suite covering
special chars, hard breaks, nested containers, astral chars).

## Documented limits (recorded, not hidden)

- **Degenerate, non-authorable corpora don't round-trip**: a mark spanning a
  hard break, an empty first line in a hard-break block (markdown has no
  blank-then-forced-break).
- **Coarse diff** (prefix/suffix trim, phase 1): superseded by PR-B Myers/LCS
  `delta::diff` on `integration/richtext`. Anchor survival held under coarse
  diff; the `Delta` was not a minimal edit script.
- **Canonicalizations** (distinct markdown → one corpus): hard break →
  `continues` line; adjacent same-shape sibling lists merge; adjacent
  blockquotes merge; empty blocks/containers keep one empty line; sequential
  island ids.

## Why it still matters

Every later phase builds on these exact bytes: phase 2's typst emitter, the
storage `CanonicalRichText` newtype, and the render seam all consume this
freeze unchanged. Changing any rule here is a schema-version bump, not a
patch — see DOCUMENT_STORAGE.md and CONVERT.md for how storage and the typst
backend consume it today.
