# Handover: verify form-view removal + `Quill::validate`

Branch: `claude/form-projection-removal-impact-TH8D3`

This change was authored in a **network-restricted sandbox** where crates.io is
blocked and no dependency cache / `target/` exists, so **nothing was compiled or
run**. Every Rust/JS/Python edit was hand-reviewed against the surrounding code,
but this branch needs a real build + test pass in an unrestricted environment
before it can be trusted/merged.

## What changed (context for the verifier)

- Removed the schema-aware form view: `Quill::form` / `blank_main` /
  `blank_card` (Rust), `quill.form` / `blankMain` / `blankCard` (WASM + Python),
  and the `Form` / `FormCard` / `FormFieldValue` / `FormFieldSource` types.
  Deleted `crates/quillmark/src/form.rs` and `crates/quillmark/src/form/tests.rs`.
- Added `Quill::validate(&Document) -> Vec<Diagnostic>` (Rust), exposed as
  `quill.validate(doc)` in WASM (`Diagnostic[]`) and Python (`list[dict]`).
  It forwards canonical `validation::*` diagnostics and, unlike `render`,
  includes the non-fatal `validation::must_fill_absent` completeness signal.
- Ported tests: `crates/quillmark/tests/validate_test.rs`, a `quill.validate`
  suite in `crates/bindings/wasm/basic.test.js`, and
  `crates/bindings/python/tests/test_validate.py` (replaces `test_form.py`).
- Docs: `prose/canon/SCHEMAS.md`, `prose/BACKLOG.md`, both binding READMEs,
  new `docs/migrations/0.87-to-0.88.md` (+ index + mkdocs nav).

## Verification checklist

Run from the repo root. Tick each box; if a step fails, capture output before
fixing.

- [ ] **Workspace build + tests**
  ```bash
  cargo test --workspace
  ```
  - [ ] New test passes: `cargo test -p quillmark --test validate_test`
  - [ ] Schema golden still green (should be **unaffected** — we did not change
        schema emission order): `cargo test -p quillmark-core schema_snapshot`

- [ ] **Lint + docs** (CI denies warnings / broken intra-doc links)
  ```bash
  cargo clippy --workspace --all-targets -- -D warnings
  cargo doc --no-deps -p quillmark -p quillmark-core -p quillmark-typst
  ```
  Pay attention to: unused imports in
  `crates/quillmark/src/orchestration/quill.rs` (the `crate::form` import was
  removed — confirm `CardSchema`, `Diagnostic`, etc. are still used) and any
  doc-comment links in the new `validate` rustdoc.

- [ ] **WASM bindings**
  ```bash
  ./scripts/build-wasm.sh
  cd crates/bindings/wasm && npm test
  ```
  - [ ] `describe('quill.validate', …)` suite passes.
  - [ ] The Must-Fill/Endorsed suite's `validate surfaces diagnostics …` test
        passes.
  - [ ] Generated `.d.ts` no longer exports `Form` / `FormCard` /
        `FormFieldValue` / `FormFieldSource`, and `Quill` exposes `validate`.

- [ ] **Python bindings**
  ```bash
  cd crates/bindings/python && uv run maturin develop && uv run pytest
  ```
  - [ ] `tests/test_validate.py` passes (it skips if the native module is
        unavailable — confirm it actually ran, not skipped).
  - [ ] `quill.validate(doc)` returns a `list` of dicts; `quill.form` /
        `blank_main` / `blank_card` are gone.

- [ ] **Docs site (optional)**: `mkdocs build --strict` (catches the new
      migration page / nav wiring).

## Risk areas to scrutinize

- **Diagnostic wire shape.** `validate` serializes `Vec<quillmark_core::Diagnostic>`
  directly (same as the old `Form.diagnostics`). Confirm the JS/Python objects
  carry `code`, `path`, `hint`, `severity` with the expected casing/values
  (tests assert `severity === 'error'`, `code === 'validation::type_mismatch'`,
  etc.).
- **`validation::unknown_card` code + path.** The WASM/Python/Rust tests assert
  this exact code for an undeclared card kind. Verify against
  `crates/core/src/quill/validation.rs` if it fails.
- **`must_fill_absent` paths.** Tests expect bare field names (`title`,
  `count`) as `path`. Confirm validation emits top-level field paths unprefixed.
- **Python `json_to_py` → `PyList` downcast.** `validate` downcasts the
  serialized array to `PyList`; confirm `json_to_py` returns a list for a JSON
  array (it does at `crates/bindings/python/src/types.rs`).

## Follow-ups (not blockers; out of scope for this branch)

- Consider exposing `seed_main` / `seed_card` in WASM/Python (only
  `seedDocument` is exposed today) so consumers adding a single card have a
  pre-filled starter — the removed `blank_card` had no Document-path twin.
- `BACKLOG.md` still tracks a dedicated strict-completeness / finalize-gate
  query; `validate`'s `must_fill_absent` is the de-facto signal until then.
- Release tooling auto-seeds `CHANGELOG.md` and bumps versions from commits, so
  neither was hand-edited here. Confirm the migration guide's `0.87 → 0.88`
  framing matches the version the release picks.
