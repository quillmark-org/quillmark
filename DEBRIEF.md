# Debrief — engine-free `Quill`, split WASM, single core type

> For the reviewer. This branch (`feat/backend-decouple`) is one large,
> intentional breaking change. Pre-1.0; breaking changes are acceptable. This
> document is the map and the rationale; the design proposal is
> [`prose/proposals/wasm-bindings-split.md`](prose/proposals/wasm-bindings-split.md)
> and the user-facing migration is
> [`docs/migrations/0.89-to-0.90.md`](docs/migrations/0.89-to-0.90.md).

## What and why

A web content editor that only loads quill schemas and validates documents was
forced to download and instantiate the **full** rendering binding — ~8 MB gzip,
~96% of it Typst — just to call `validate`. The goal: let an editor load,
validate, schema, seed, and blueprint **without** Typst, and lazy-load rendering
only on preview/export.

That size win is enabled by one architectural move, which then exposed a second:

1. **Engine-free `Quill`** — `Quill` no longer holds a backend. It becomes
   portable, validated data. The `Quillmark` engine shrinks to a backend
   registry + render dispatcher. The boundary falls on *which types exist*, not
   on feature-gated methods inside a shared type.
2. **WASM split** — one crate, two artifacts gated by a `render` cargo feature:
   a Typst-less **core** and a Typst-backed **render** superset, shipped as one
   npm package with `./core` and `./render` subpath exports.
3. **Collapse `QuillSource` → `Quill`** — once `Quill` held no backend, it was
   just `QuillSource` + a method set; the two types were accidental duplication
   and merged into one core type.

Changes 1+2 are the "split" (proposal phases 1–4). Change 3 is the proposal's
deferred follow-up, pulled into this branch at the user's request to consolidate
the breakage into a single release rather than spread two core-rename waves.

## Commits (in order)

| Commit | Scope |
|---|---|
| `415f149`, `641ac77`, `e61747e` | Proposal doc + review refinements (design only) |
| `d9e3918` | **Phase 1–2**: engine-free `Quill`; feature-gated wasm core/render |
| `7d06bd0` | **Phase 3**: build two artifacts; npm package; vitest aliases |
| `2196954` | Loosen core size budget; record as-built sizes |
| `99e2a6a` | **Collapse** `QuillSource`/`Quill` into one core type |
| (this commit) | **Phase 4**: core JS tests, canon docs, migration guide, CHANGELOG |

~44 files, +1041/−631 vs `main`.

## The new shape

| Surface | Home | Build |
|---|---|---|
| `backend_id` (declared string) | `Quill` (core) | core |
| `validate` · `schema` · `metadata` · `blueprint` · `seed_*` · `compile_data` · `dry_run` | `Quill` (core, pure config reads) | core |
| `Quill::from_tree` (→ `Vec<Diagnostic>`) | `quillmark-core` | core |
| `quill_from_path` / `quill_from_tree` (→ `RenderError`) | `quillmark` free fns | render |
| `supported_formats(&quill)` · `supports_canvas(&quill)` | `Quillmark` engine (static capability) | render |
| `open` · `render` | `Quillmark` engine | render |
| `render(opts)` · `paint` · `pageSize` · `pageCount` · `warnings` + capability mirror | `RenderSession` | render |

Capability (`supported_formats`, `supports_canvas`) lives on the **engine**, not
on `RenderSession`: the engine answers it from its registered backends *without
opening/compiling a document*, so it's a cheap, non-failing pre-render probe.
`Backend` gained `fn supports_canvas(&self) -> bool { false }` (Typst overrides
to `true`), which retired the `backend_id == "typst"` magic string that
previously stood in for canvas capability.

## Where to look (change map)

**Core (`crates/core`)**
- `src/quill.rs` — `QuillSource` renamed to `Quill`, held **by value** (the
  `Arc` was vestigial and dropped). Struct + module wiring.
- `src/quill/compose.rs` — **new**; the consumer methods that used to live in
  the orchestration crate (`compile_data`, `validate`, `dry_run`, `seed_*`,
  `check_quill_reference`, helpers). They had to move here: inherent methods
  must be defined in the type's crate.
- `src/quill/seed.rs` (+ `seed/tests.rs`) — **moved** from `quillmark`; now take
  `&Quill`.
- `src/backend.rs` — `Backend::open(&Quill)` (was `&QuillSource`); new
  `supports_canvas()` default method.

**Backend (`crates/backends/typst`)** — `&QuillSource` → `&Quill` throughout;
`supports_canvas() -> true`.

**Orchestration (`crates/quillmark`)**
- `src/orchestration/quill.rs` — **deleted** (the wrapper is gone).
- `src/orchestration/engine.rs` — engine is now registry + dispatcher:
  `resolve_backend`, `open`, `render`, `supported_formats`, `supports_canvas`;
  backend resolved at render time (`UnsupportedBackend` on no match).
- `src/load.rs` — **new**; `quill_from_path` / `quill_from_tree` free functions
  (fs walk lives here, not core).
- `src/lib.rs` — re-exports core `Quill`; exposes the loaders.

**Bindings**
- `wasm/src/engine.rs` — `Quill.fromTree` static ctor; `render`/`open`/capability
  moved to the `Quillmark` wrapper; `supportedFormats` stripped from
  `Quill.metadata`; engine/`RenderSession`/canvas/`web-sys`/`quillmark_typst`
  gated behind `#[cfg(feature = "render")]`.
- `wasm/Cargo.toml` — `default = ["render"]`; `render = ["dep:quillmark-typst",
  "quillmark/typst", "dep:web-sys"]`. **`quillmark` is a direct path dep, not
  `workspace = true`** (see Risks).
- `python/src/types.rs` — Python's public API is **unchanged**: `PyQuill` now
  carries an `Arc<Quillmark>` and routes `render`/`open`/`supported_formats`/
  `supports_canvas` through it; `.source()` calls dropped.
- `cli/src/commands/*` — `load_quill` uses `quill_from_path`; `render` goes
  through a local engine.

**Build / packaging**
- `scripts/build-wasm.sh` — builds both variants → `pkg/core/`, `pkg/render/`;
  release-only core gzip budget (see Risks).
- `wasm/package.template.json` — `./core` + `./render` subpath exports; root is
  render.
- `wasm/vitest.config.js` — `@quillmark-wasm` → render, `@quillmark-wasm/core` →
  core (order matters; see the comment there).

**Docs** — `prose/canon/{QUILL,ARCHITECTURE,PREVIEW,INDEX}.md`,
`docs/migrations/0.89-to-0.90.md` (+ index), `docs/getting-started/quickstart.md`,
`CHANGELOG.md`.

## Decisions worth scrutinizing

1. **`Quill::from_tree` returns `Vec<Diagnostic>`, but the binding-facing
   loaders return `RenderError`.** Core keeps the raw-diagnostics primitive;
   `quillmark::quill_from_tree`/`quill_from_path` wrap it into
   `RenderError::QuillConfig`. The WASM `Quill.fromTree` calls the wrapping free
   fn (not the core primitive). Is the two-error-type seam worth it, or should
   core's `from_tree` just return `RenderError` (which also lives in core)?
2. **`Quill` is held by value; `Quillmark::render` takes `&Quill`.** No shared
   ownership remains. Cheap clones now deep-copy the config/file tree. Confirm
   nothing in hot paths clones `Quill` per render.
3. **Capability on the engine, not the session.** `supports_canvas` /
   `supported_formats` resolve a backend but never compile. The session also
   mirrors them post-open. Two homes for the same fact — intentional (cheap
   probe vs live session), but worth a sanity check that they can't disagree.
4. **Python keeps `Quill.render(...)` by pairing each `PyQuill` with an
   `Arc<Quillmark>`.** This preserves the Python surface but means a Python
   `Quill` is *not* engine-free the way the Rust/JS one is. Deliberate
   (minimize Python churn), but a reviewer may want it flagged.
5. **`render` kept as engine sugar.** `engine.render` = `open` + `session.render`
   with a default-format fill. We deliberately did *not* remove it (it's free
   and saves the 90% one-shot case two calls).
6. **`backend_id` stays on core `Quill` as declared intent.** A core editor that
   wants a cheap "probably previewable" hint can compare it; the library no
   longer blesses the string as a capability.

## Risks / sharp edges

- **`default-features = false` does not work through `workspace = true`.** The
  workspace `quillmark` dep defaults its features on, and a member's
  `default-features = false` is ignored — this silently pulled Typst into the
  core build until caught via `cargo tree`. Fixed by making `quillmark` a
  **direct path dep** in `crates/bindings/wasm/Cargo.toml` (the crate is
  `publish = false`, so no version pin needed). If anyone "tidies" that back to
  `workspace = true`, the core build re-bloats with Typst and the size budget
  catches it only on a release build.
- **Core size is 0.66 MB gzip, not the proposal's spike-claimed 0.34 MB.** The
  spike omitted the full `Document` editing API + binding glue. Still 8.01 →
  0.66 (~12×), Typst excluded. The budget is **1.5 MB** (≈5× under render) and
  **only enforced on the `wasm-release` profile** (the `wasm-ci` profile is
  unoptimized, so its absolute size is meaningless). CI runs `--ci`, so the hard
  budget only fires on release; CI's real guarantee is the feature graph.
- **Backend errors moved from load time to render time.** `Quill::from_tree`
  now succeeds for an unknown backend; `UnsupportedBackend` surfaces from the
  first engine call. Slightly later failure for a Typst-having consumer; for a
  Typst-less core it's the only correct behavior. `quill_engine_test.rs` was
  restructured to assert this.
- **Cross-module WASM handoff is data, not handles.** Core and render are
  separate linear memories. The proposal's "API superset" is at the type level;
  a `Quill`/`Document` from core is not a usable handle in render — cross as
  `tree` + `doc.toJson()`. Documented, but a foot-gun for consumers who assume
  object interchange.

## Validation

All green at time of writing:

| Suite | Result |
|---|---|
| `cargo test -p quillmark-core` (incl. moved seed tests) | 538 passed |
| `cargo test -p quillmark-typst` | 217 passed |
| `cargo test -p quillmark` (integration) | all passed |
| `cargo build -p quillmark-cli`, `cargo check -p quillmark-python` | clean |
| wasm `--no-default-features` (core) + default (render) on `wasm32` | both compile, zero warnings; Typst absent from core graph (verified via `cargo tree -i quillmark-typst`) |
| `build-wasm.sh` (release) | core 2.0 MB raw / **0.66 MB gz**; render 23 MB / **8.01 MB gz**; budget OK |
| vitest (`core` + `basic` + `canvas`) | **117 passed** (5 new core-bundle tests + 112 render) |

Repo scan confirms zero residual `QuillSource`, no orphaned `seed` module, and
the only remaining `.source()` is `std::error::Error::source()`.

## Scope boundaries

- **In scope:** the three changes above, across core/typst/quillmark + all three
  bindings, build/packaging/CI, docs, and the migration guide.
- **Out of scope:** *feature-splitting* Python/CLI into core/render builds
  (their public APIs are preserved and they keep compiling — that was a hard
  requirement, not optional). Sharing one linear memory across the two WASM
  modules. New render features.

## Suggested review focus

1. The error-type seam (decision #1) and whether the two loader paths are worth
   it.
2. The `cfg(feature = "render")` gating in `wasm/src/engine.rs` — is anything
   that should be render-only leaking into core, or vice-versa? (The generated
   `pkg/core/wasm.d.ts` is the ground truth: it should expose `Document` +
   `Quill` only, no `Quillmark`/`RenderSession`/`paint`.)
3. The `quillmark` direct-path-dep workaround in the wasm `Cargo.toml`.
4. `compose.rs` — the moved methods are a near-verbatim relocation; diff them
   against the deleted `orchestration/quill.rs` to confirm no behavior drift
   (only `self.source.X()` → `self.X()` and `&QuillSource` → `&Quill`).
5. Python's `Arc<Quillmark>`-per-`Quill` pairing (decision #4).
