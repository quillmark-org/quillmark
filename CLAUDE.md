# Quillmark

Schema-driven document engine: Markdown + YAML card metadata → rendered PDF/SVG/PNG via a Typst or PDF-form backend.

Crates: `core` (parsing, schema, traits) · `quillmark` (orchestration) · `richtext` (`RichText` corpus model; the workspace's only markdown parser) · `backends/{typst,pdfform}` · `quillmark-pdf` (Typst-free AcroForm stamping) · `bindings/{python,wasm,cli}` · `fixtures` (test Quills) · `fuzz` (property tests).

Design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md). Comments and docs follow the `dense-prose` skill: dense, present-tense, unsold.

- Released guides in [`docs/migrations/`](docs/migrations/) are immutable; edit only the unreleased one.
- The `Cargo.toml` version is the last *released* version, not a working one; CI bumps it on release.
- Commit early and often — CI runs builds and tests on push.

## Tests

- `cargo test --workspace`
- WASM: `./scripts/build-wasm.sh && cd crates/bindings/wasm && npm test`
- Python: `cd crates/bindings/python && uv run maturin develop && uv run pytest`
