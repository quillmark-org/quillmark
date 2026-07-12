# Cross-crate: the richtext value seam and the delta-commit protocol

All entries need judgment — each spans crates or changes a contract.

### Every apply round-trips each richtext value through the canonical codec three times

Per `apply`: `to_plate_json` serializes the
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

### ~~The delta-commit orchestration lives in the WASM binding~~ (moot — #886)

The per-field delta-commit path (`applyFieldDelta` and the
`apply_for_field_delta` transaction it drove) was **removed** in #886, so there
is no delta-commit protocol left to hoist. Whole-document `apply(doc)` is the
one edit verb, and its orchestration already lives in `quillmark`.
