# Handover: verify `example()` removal

Branch: `claude/document-example-refactor-1tE7U`
Commit: `2f0e7a5` — "Remove example() reference document, fold into seeding"

## Why this handover exists

The change was authored in a web/remote container whose network policy blocks
crates.io, with no vendored or cached registry. **Nothing in this branch has
been compiled or tested.** Everything below was verified by hand (call sites
traced, signatures checked, no dangling references) but needs a real toolchain
to confirm.

## What changed (one-line summary)

The `example` reference document is removed. Its "show me a filled-out one"
role is served by seeding (`Quill::seed_document()`), which renders identically
because absent fields resolve `default:` → zero at the render floor. Internally
the `FillSource` fork in blueprint emission is gone; the blueprint always
renders `default:` else the `<must-fill>` sentinel.

## Build & test checklist

Run from repo root in an environment with crates.io access.

- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets` (no new warnings — several
      functions lost a parameter; watch for unused imports/vars)
- [ ] `cargo doc --no-deps -p quillmark-core` (the `blueprint()` doc links
      `[`Document`]: crate::Document` — confirm no broken intra-doc link)
- [ ] WASM: `./scripts/build-wasm.sh` then `cd crates/bindings/wasm && npm test`
- [ ] Python: `cd crates/bindings/python && uv run maturin develop && uv run pytest`

## Manual smoke tests (behavior parity)

The point of the refactor is that the seeded document renders identically to
the old `example()` document. Spot-check that:

- [ ] CLI render with **no input file** still produces output:
      `cargo run -p quillmark-cli -- render crates/fixtures/quills/<a_quill>`
      (e.g. `cmu_letter`, `usaf_memo`). It now renders the seeded document.
      Confirm the artifact is non-empty and visually sane.
- [ ] `cargo test -p quillmark --test usaf_memo_signature_test` — this test was
      repointed from `example()` to `seed_document()`; it asserts both the
      `Signature` and `Ind_0_Signature` widgets are emitted. Confirm the seed
      (one card instance per kind) still triggers both widgets.
- [ ] `crates/quillmark/tests/common.rs` render helper was repointed to
      `seed_document()`; any integration test using it should still render.

## Known behavioral difference (intended)

- The old `example()` **string** baked `example: › default: › zero` into every
  cell, so default/zero values were visible as text. `seed_document()` commits
  **only** `example:` values; defaults/zeros are absent and interpolated at
  render. The **rendered output is identical**; only an inspector reading the
  intermediate markdown source would see fewer fields. No consumer was found
  that reads that string for its content — confirm none exists downstream
  (web app / editor integrations outside this repo).

## Breaking changes to announce

- Rust: `QuillConfig::example()` removed.
- WASM: `Quill.example` getter removed (use `Quill.seedDocument()`).
- Python: `Quill.example` property removed (use `Quill.seed_document()`).
- CLI: `render` with no input file now renders the seeded document (same intent,
  same render).

See the `## Unreleased` entry in `CHANGELOG.md`.

## If something fails

- Unused-import errors in `blueprint.rs`: `zero_value` was removed from the
  `use super::{…}` line — re-check if any reintroduced code needs it.
- Type-inference error at `render.rs` `(quill.seed_document(), Vec::new(), None)`:
  the empty `Vec` must match the warnings type of the other match arm
  (`output.warnings`); annotate if inference fails.
- Doc-link failure: downgrade `[`Document`]: crate::Document` in `blueprint.rs`
  to plain text if `Document` isn't resolvable from that scope.

## Follow-ups (optional, not required to merge)

- Consider whether `Quill::seed_document()` logic (currently in
  `crates/quillmark/src/seed.rs`, operating on `Quill`) should move into
  `core` as `QuillConfig::seed() -> Document`, so `core` owns both schema
  projections (blueprint + seed). It only needs `QuillConfig`/`CardSchema` plus
  core `Document` types. Deferred from this change.
