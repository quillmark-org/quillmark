# Contributing to Quillmark

## Documentation

- API docs: standard in-line Rust doc comments (`///`), minimal examples on public APIs, err toward brevity.
- Design docs and specifications: `prose/designs/`.
- User guide: `docs/` (rendered by mkdocs).

## Binding tests

**WASM:** repo root → `./scripts/build-wasm.sh` → `cd crates/bindings/wasm` → `npm install` (first time) → `npm run test`

**Python:** `cd crates/bindings/python` → `uv sync --extra dev` → `uv run maturin develop` → `uv run pytest`