# Quillmark

Format-first Markdown rendering: Markdown + YAML card metadata → PDF/SVG/PNG via a Typst backend. Crates: `core` (parsing/schema/traits), `quillmark` (orchestration), `backends/typst`, `bindings/{python,wasm,cli}`, `fixtures` (test Quills).

Design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md)

Released migration guides in [`docs/migrations/`](docs/migrations/) are era-accurate and immutable; only the working (unreleased) one is mutable.

In a cloud environment, commit early and often to leverage CI/CD builds and tests.

## Tests

```bash
cargo test --workspace
```

WASM: `./scripts/build-wasm.sh` → `cd crates/bindings/wasm && npm test`  
Python: `cd crates/bindings/python && uv run maturin develop && uv run pytest`
