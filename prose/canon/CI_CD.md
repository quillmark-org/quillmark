# Quillmark Rust Workspace — CI/CD

**Status**: Implemented

Published crates: `quillmark-core`, `backends/quillmark-typst`, `quillmark`, `bindings/quillmark-cli`.

Not published: `quillmark-fixtures`, `quillmark-fuzz`, `bindings/quillmark-python`, `bindings/quillmark-wasm`.

---

## 1) Continuous Integration (CI)

**Trigger**: pull requests and pushes to any branch except version tags.
**Jobs** (all Linux, run in parallel):

| Job | What it does |
|-----|-------------|
| `lint` | `cargo fmt --all -- --check` (Clippy commented out, not yet enforced) |
| `test` | `cargo test --locked` in a matrix: default features and `--all-features` |
| `docs` | `cargo doc --no-deps --locked` with `-Dwarnings` |
| `wasm` | `cargo check --package quillmark-wasm --target wasm32-unknown-unknown --locked` |

Excluded: multi-OS matrix, MSRV, security scanners, coverage, benchmarks.

---

## 2) Continuous Delivery (CD)

### Release Preparation (`release-prepare.yml`)

**Trigger**: `workflow_dispatch` from GitHub UI with a `bump` input (`patch` or `minor`) and an optional `prerelease` flag.

1. Installs `cargo-release` and runs `cargo release version <bump>` to bump all workspace `Cargo.toml` versions and intra-workspace dependencies.
2. Commits the bump directly to `main` and atomically pushes the `vX.Y.Z` tag alongside it.

The push uses a GitHub App token so the resulting tag fires the release workflow (events from the default `GITHUB_TOKEN` are suppressed by GitHub).

### Release & Publish (`release.yml`)

**Trigger**: push of a `v*` tag.

**Phase 1 — Release** (runs first):
1. Extracts the version from the tag name and validates it matches the workspace `Cargo.toml`.
2. Creates a GitHub Release for the tag (marked as a pre-release for `-rc` versions).

**Phase 2 — Publish** (all run in parallel, after release):

| Target | Registry | Auth |
|--------|----------|------|
| Rust crates | crates.io | OIDC Trusted Publishing via `rust-lang/crates-io-auth-action` (`id-token: write`) |
| WASM bindings | npm | OIDC Trusted Publisher (`id-token: write`) |
| Python bindings | PyPI | OIDC Trusted Publishing via `pypa/gh-action-pypi-publish` (`id-token: write`) |

- **Crates**: `cargo publish --locked --no-verify`
- **WASM**: builds via `./scripts/build-wasm.sh`, runs `npm test`, publishes `@quillmark/wasm` with `--provenance`
- **Python**: builds wheels via `maturin-action` for Linux (x86_64, aarch64), Windows (x64), macOS (aarch64) — Python 3.10–3.12 — plus sdist, then uploads to PyPI

---

## 3) Versioning

- SemVer across all workspace crates and bindings.
- Version bumps are initiated via GitHub UI (`workflow_dispatch`) and executed by `cargo-release` in CI, which commits directly to `main` and pushes the `vX.Y.Z` tag.
- WASM npm package version is derived from the workspace version at build time (`scripts/build-wasm.sh`).
- Python package version is derived from the workspace Cargo.toml via maturin's `dynamic = ["version"]`.
