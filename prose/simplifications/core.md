# crates/core

## Needs judgment

### quill/compose.rs:359 — corpus companion caches leak maybe-populated state to callers

`default_corpus`/`example_corpus` are populated only by a loader post-pass
(config.rs:1580), so each consumer carries its own fallback: `resolve_value`
falls through to the raw markdown `default` — which then crosses the seam
un-imported, since `resolve_fields` runs after coercion in `compile_data`, so
the "seam carries corpus" invariant silently breaks for a schema built outside
the loader — and seed.rs:88 implements a three-tier lookup with
`unwrap_or_else(RichText::empty)` swallowing failures. Fix: enforce at
construction (populate the companions in `FieldSchema` construction) or expose
an accessor that computes on cache miss, so the invariant lives in the type.
