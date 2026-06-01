# Quillmark

Format-first Markdown rendering: Markdown + YAML card metadata → PDF/SVG/PNG via a Typst backend. Crates: `core` (parsing/schema/traits), `quillmark` (orchestration), `backends/typst`, `bindings/{python,wasm,cli}`, `fixtures` (test Quills).

Design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md)

## Tests

```bash
cargo test --workspace
```

WASM: `./scripts/build-wasm.sh` → `cd crates/bindings/wasm && npm test`  
Python: `cd crates/bindings/python && uv run maturin develop && uv run pytest`
