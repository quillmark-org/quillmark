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

### session.rs:195, session.rs:210 — public session API with no consumer

`change_log()` and the `FieldChange` re-export (session.rs:4, lib.rs:47) have
zero uses; `record_field_delta_at`/`record_field_change_at` are copy-paste
twins exercised only by one unit test — the wasm delta path uses
`ensure_base_revision` + `apply_for_field_delta`. Three public methods plus a
re-export to keep semver-stable for a client that does not exist. Fix: keep
only the used surface; add record/bundle variants when a consumer appears.
Possibly deliberate forward surface — decide, then either wire a consumer or
cut.

### document/edit.rs:448 — `Card::set_body_corpus` is a public alias of `overwrite_body`

One line, a second public write path to the body corpus. No longer dead: the
`ab082e8` rollback rewrite made the wasm binding call it as the body-restore
step (`wasm/src/engine.rs`), so it now has exactly one production consumer —
the original "no consumer outside unit tests" premise is void. Residual: the
rollback could call `overwrite_body` directly; whether the alias earns its
public surface is now a question about the rollback path, better framed by the
"transactionality is the caller's job" seam entry than as dead code.
