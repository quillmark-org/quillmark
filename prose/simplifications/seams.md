# Cross-crate: the richtext value seam and the delta-commit protocol

All entries need judgment — each spans crates or changes a contract.

### Four sites re-implement the dual-shape richtext decode

`core/src/document/wire.rs:314` (`body_from_wire`), the coercion branch
`core/src/quill/config.rs:359`, `literal_corpus` at
`core/src/quill/config.rs:1646`, and the validation backstop
`core/src/quill/validation.rs:470` each independently implement "JSON object →
`serial::from_canonical_value`, string → `from_markdown`, then optional
`is_inline` check / re-canonicalize". The accepted `$body`/richtext-value
encodings are defined in four places that already drift: wire.rs accepts
`null`, config.rs adds array-unwrap/scalar leniency, validation.rs
`.ok()`-swallows decode errors. The inline-violation prose is separately
restated three times (coercion's `inline_check`, `validation.rs`'s
`not_inline_hint`, `config.rs`'s `richtext_inline_error`). Fix: one shared
decoder (`RichText::from_json_or_markdown` beside `from_canonical_value`, or
next to `document::import_body`) plus a single `check_inline`, with per-site
error wrapping and *deliberate* per-site leniency kept local — the leniency
differences are intentional in places and must be preserved, which is what
makes this a judgment call.

### Every apply round-trips each richtext value through the canonical codec three times

Per `apply`/`applyFieldDelta`: `to_data_json` serializes the
already-normalized body via `to_canonical_value` (clone + normalize + build
tree + `sorted_value` rebuild — `core/src/document/mod.rs:346`), coercion
re-parses with `from_canonical_value` and re-serializes
(`core/src/quill/config.rs:382`), and backend codegen parses a third time
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

### ~~Transactionality is the caller's job~~ (done for the corpus apply)

`RichText::apply_field_change` is now all-or-nothing: a multi-op bundle stages
on a scratch copy and swaps on success (`richtext/src/ops.rs`), and
`Card::apply_body_change` documents the same contract, so no consumer needs to
snapshot-and-restore around a *half-applied body*. The WASM binding still holds
one body snapshot (`wasm/src/engine.rs`), but only as the transaction boundary
for the compile and record steps that run *after* the (now atomic) body
mutation and can still fail — which is the separate "delta-commit orchestration
lives in the WASM binding" seam above, not corpus transactionality. Close that
seam (a core/quillmark-level `apply_field_delta` owning apply + compile + record
+ rollback) and the caller-side snapshot goes with it.
