# Quillmark

Schema-driven document engine: Markdown + YAML card metadata → a fully typeset PDF/SVG/PNG via a Typst backend. Crates: `core` (parsing/schema/traits), `quillmark` (orchestration), `backends/typst`, `bindings/{python,wasm,cli}`, `fixtures` (test Quills).

Design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md)

Comments and docs: dense, present-tense, no marketing — see the `dense-prose` skill.

Released migration guides in [`docs/migrations/`](docs/migrations/) are era-accurate and immutable; only the working (unreleased) one is mutable.

The version in `Cargo.toml` is the *last released* version, not a working one. A CI/CD workflow will automatically bump the version on the next release. 

In a cloud environment, commit early and often to leverage CI/CD builds and tests.

## Tests

```bash
cargo test --workspace
```

WASM: `./scripts/build-wasm.sh` → `cd crates/bindings/wasm && npm test`. in Cloud environment.
Python: `cd crates/bindings/python && uv run maturin develop && uv run pytest`

In Claude Cloud environment, run bindings tests on CI/CD instead of locally.
