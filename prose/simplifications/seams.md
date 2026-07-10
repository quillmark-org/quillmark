# Cross-crate: the richtext value seam and the delta-commit protocol

All entries need judgment — each spans crates or changes a contract.

### Every apply round-trips each richtext value through the canonical codec three times

Per `apply`/`applyFieldDelta`: `to_plate_json` serializes the
already-normalized body via `to_canonical_value` (clone + normalize + build
tree + `sorted_value` rebuild — `core/src/document/mod.rs:382`), coercion
re-parses with `from_canonical_value` and re-serializes
(`core/src/quill/config.rs:405`), and backend codegen parses a third time
before `emit_richtext` (`backends/typst/src/helper.rs:180`). ~3 full
parse/serialize + normalize passes over every corpus per keystroke preview
recompile. Fix: treat a corpus emitted by `to_canonical_value` as already
canonical across the internal seam — skip the coercion re-parse/re-serialize
for object inputs after validation, or carry `RichText` values through the
pipeline and serialize only at the backend boundary. Compounds with
`to_canonical_value`'s double tree build (richtext.md).

### The delta-commit orchestration lives in the WASM binding

The phase gate (`field != "$body"`), quill-ref check, `compile_data`
sequencing, `apply_for_field_delta`, and document rollback — ~60 lines of
transactional protocol — sit in `wasm/src/engine.rs:1531`, while the
`quillmark` orchestration crate (which owns the analogous `open`/`render`
seams and holds both `Quill` and `Document`) has no delta-commit helper. The
Python binding and any future consumer must replicate the protocol, and when
delta targets extend beyond `$body`, every binding is edited in lockstep. Fix:
a core/quillmark-level
`apply_field_delta(session, config, doc, field, base_revision, delta)` owning
field-address routing and the transaction; bindings become thin wrappers.

The corpus apply is now atomic (`RichText::apply_field_change` stages on a
scratch copy and swaps on success), so the binding's remaining body snapshot
guards only the compile and record steps that run *after* the body mutation —
not a half-applied body. Closing this seam absorbs that snapshot into the
core-owned transaction.
