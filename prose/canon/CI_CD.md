# Quillmark Rust Workspace — CI/CD

> **Implementation**: `.github/workflows/`

## TL;DR

Four workflows. `ci.yml` runs lint/test/wasm/dotnet on every PR and non-tag push. `release-prepare.yml` computes the next version, bumps the workspace, and opens a release PR. `release.yml` tags and publishes to crates.io, npm, PyPI, and NuGet when that PR merges. `docs.yml` builds MkDocs and deploys to GitHub Pages on stable releases.

Published crates: `quillmark-core`, `quillmark-typst`, `quillmark`, `quillmark-cli`. Not published: `quillmark-fixtures`, `quillmark-fuzz`, `quillmark-python`, `quillmark-wasm`, `quillmark-dotnet`.

---

## 1) CI (`ci.yml`)

**Trigger**: pull requests and pushes to any ref except version tags (`tags-ignore: v**`).
**Jobs** (all Linux, parallel):

| Job | What it does |
|-----|-------------|
| `lint` | `cargo doc --no-deps --locked` with `RUSTDOCFLAGS=-Dwarnings` (Clippy commented out, not yet enforced) |
| `test` | `cargo test --workspace --all-features --locked` |
| `wasm` | first asserts the no-default-features core graph excludes Typst (`cargo tree -i quillmark-typst` must fail), then builds via `./scripts/build-wasm.sh --ci`, then `npx vitest run` |
| `dotnet` | builds the native cdylib, then `dotnet test` (`Quillmark.Tests`) against the Typst backend |

The `wasm` job caches `target/wasm32-unknown-unknown/wasm-ci` + `pkg` under key `wasm-ci-${os}-${hashFiles('Cargo.lock','crates/**/*.rs')}`, rebuilding only when the lockfile or any crate source changes. The `wasm-ci-` namespace is deliberately disjoint from `release.yml`'s `wasm-release-` cache so a CI build (debug `wasm-ci` profile) can never be restored into a release job and published to npm.

Excluded: multi-OS matrix, MSRV, security scanners, coverage, benchmarks.

---

## 2) Release prepare (`release-prepare.yml`)

**Trigger**: `workflow_dispatch` with `bump` (`patch`/`minor`) and a `release_candidate` boolean.

1. **Compute next version** (custom bash, not `cargo-release`): reads the current `quillmark` version via `cargo metadata`.
   - If current is `X.Y.Z-rc.N`: the base is fixed and `bump` is ignored. `release_candidate=true` → `X.Y.Z-rc.(N+1)`; false → promote to `X.Y.Z`.
   - Otherwise apply `bump` (`minor` → `X.(Y+1).0`, `patch` → `X.Y.(Z+1)`), appending `-rc.1` when `release_candidate=true`.
2. `cargo release version <computed> --workspace --no-confirm --execute` writes the literal computed string into every `Cargo.toml` and updates intra-workspace deps.
3. Seeds a `CHANGELOG.md` section from `git log` since the last stable tag, then pushes `release/vX.Y.Z` and opens a PR into `main`.

The PR uses a GitHub App token (`TAGGER_APP_ID`/`TAGGER_PRIVATE_KEY`) so CI runs on it and so its merge fires `release.yml` — PRs opened with the default `GITHUB_TOKEN` do not trigger workflow events.

---

## 3) Release & publish (`release.yml`)

**Trigger**: a `release/v*` PR merged into `main` (`pull_request: closed` + `merged == true`).

**`prepare` job** (App token; the tag-creation ruleset blocks `GITHUB_TOKEN`): reads the workspace version, tags `vX.Y.Z`, extracts the matching `CHANGELOG.md` section, and creates a GitHub Release — `--prerelease` for versions containing `-`, else `--latest`.

**Publish jobs** (parallel, `needs: prepare`, OIDC `id-token: write`):

| Target | Registry | Command |
|--------|----------|---------|
| Rust crates | crates.io | `cargo publish --locked --no-verify` via `rust-lang/crates-io-auth-action` |
| WASM | npm | `npm publish --access public --provenance` (Trusted Publisher) |
| Python | PyPI | `pypa/gh-action-pypi-publish` over prebuilt wheels |
| .NET | NuGet | `dotnet pack` + `dotnet nuget push` over per-RID native libraries |

- **WASM**: restores the `wasm-release-` cache (`wasm-release` profile), builds via `./scripts/build-wasm.sh`, runs `npx vitest run`, publishes `@quillmark/wasm`. Pre-release versions (containing `-`) publish with `--tag next` so they land on the `next` dist-tag instead of `latest`.
- **Python**: `maturin-action` builds wheels for Linux (x86_64, aarch64), Windows (x64), macOS (aarch64) across Python 3.10–3.12, plus an sdist; artifacts are gathered and uploaded with `skip-existing`.
- **.NET**: builds the cdylib per RID (`linux-x64`, `win-x64`, `osx-x64`, `osx-arm64`), stages each under `runtimes/<rid>/native/`, then `dotnet pack` + `dotnet nuget push --skip-duplicate` the `Quillmark` package (gated on the `NUGET_API_KEY` secret).

---

## 4) Docs (`docs.yml`)

**Triggers**: published GitHub Releases, PRs touching `docs/**`/`mkdocs.yml`/the workflow, and `workflow_dispatch`.

- `build`: `mkdocs build --strict`; uploads the Pages artifact except on PRs (PRs are build-only validation).
- `deploy`: runs only for `workflow_dispatch` or a published **non-prerelease** release (RCs are skipped), deploying to GitHub Pages. Serialized via `concurrency: pages` with `cancel-in-progress: false`.

---

## Versioning

- SemVer across all crates and bindings; one workspace version drives everything.
- WASM npm version is derived from the workspace version at build time (`scripts/build-wasm.sh`); Python version comes from the workspace `Cargo.toml` via maturin `dynamic = ["version"]`; the .NET NuGet version is passed to `dotnet pack` (`-p:Version=`) from the workspace version.
