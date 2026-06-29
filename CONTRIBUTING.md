# Contributing to Quillmark

## Documentation

Comments and docs are dense, present-tense, and unsold:

- Say what the code can't say faster. Delete comments that restate the code; prefer a clearer name over a comment.
- No marketing words (powerful, seamless, robust, first-class, simply…). State the capability plainly.
- Describe what *is*, not how it got here — except `docs/migrations/` (immutable) and load-bearing legacy.
- Minimal examples on public APIs; err toward brevity.

Where things live:

- API docs: standard in-line Rust doc comments (`///`).
- Canonical design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md).
- User guide: `docs/` (rendered by mkdocs).
- Full style rubric and review pass: the `dense-prose` skill (`.claude/skills/dense-prose/`); `maintain-canon` covers canon structure.

## Binding tests

**WASM:** repo root → `./scripts/build-wasm.sh` → `cd crates/bindings/wasm` → `npm install` (first time) → `npm run test`

**Python:** `cd crates/bindings/python` → `uv sync --extra dev` → `uv run maturin develop` → `uv run pytest`