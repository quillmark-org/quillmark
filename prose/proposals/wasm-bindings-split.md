# WASM Bindings Split: Core + Render

> **Motivation**: a web content editor that only loads quill schemas and
> validates documents currently downloads and instantiates the full
> rendering binding — ~8.7 MB gzipped — of which Typst is ~96%. Splitting
> the binding into a small **core** (load / validate / schema / seed /
> blueprint) and a heavy **render** (Typst-backed preview / export)
> decouples editor and serverless cold start from rendering.
> Pre-1.0; breaking changes acceptable.

## TL;DR

Publish **two** WASM artifacts from one crate, gated by a `render` cargo
feature, shipped as one npm package with two entry points
(`@quillmark/wasm/core`, `@quillmark/wasm/render`). Render is an **API
superset** of core — same types and methods plus the engine; values cross
between the two as serialized data, not shared handles (separate linear
memories — see *Cross-module handoff*).

The split is enabled by one architectural change: **`Quill` no longer
holds a backend.** It becomes engine-free, portable validated data. The
`Quillmark` engine shrinks to a backend registry + render dispatcher and
exists only in the render build. The boundary falls on *which types
exist*, not on feature-gated methods inside a shared type.

## Measurement

Both built with the shipping `wasm-release` profile (opt-level=z, fat
LTO, 1 codegen-unit, stripped), gzipped at -9:

| Artifact | Raw | Gzip |
|---|---:|---:|
| Core (`Document` + `Quill`: parse · load · schema · validate · seed · blueprint) | 2.0 MB | **0.66 MB** |
| Render (everything, incl. Typst) | 23 MB | **8.01 MB** |

Typst is the overwhelming bulk of render (~92% of the gzip); core is ~12×
smaller and excludes Typst from its dependency graph entirely. The core
floor is YAML/JSON/Unicode-table code plus the full `Document` editing API
and the wasm-bindgen/serde glue — not Typst.

> **As-built vs spike.** An early spike measured a leaner core at 0.34 MB
> gzip; the shipped core is 0.66 MB because it carries the complete
> `Document` mutation surface and binding glue the spike omitted, not any
> Typst. The win is 8.01 MB → 0.66 MB, not the spike's 8.7 → 0.34. The
> `build-wasm.sh` size budget guards core at 1.5 MB gzip — ~5× under render,
> so a Typst regression can't hide, with headroom for normal core growth.

## Design

### `Quill` is engine-free, portable data

Today `Quill = Arc<QuillSource> + Arc<dyn Backend>`, and the backend is
baked in at load by the engine that created it. The backend is only used
by `render` / `open` / `supported_formats`.

After this change `Quill` holds only `Arc<QuillSource>` plus the
*declared* `backend_id` (= `config.backend`, a string). Every remaining
method — `validate`, `schema`, `metadata`, `blueprint`, `seed_*`,
`compile_data`, `backend_id` — is a pure `source.config()` read. For that
to hold, `metadata` sheds its one impurity: it currently splices in
`supportedFormats` from `backend.supported_formats()` (a backend call),
which moves to the engine (see *Capability lives on the engine*). The
remaining `metadata` is the identity snapshot of `Quill.yaml` — pure
config — so the shared `QuillMetadata` TS type is honest in both builds.

Consequences:

- **A `Quill` requires no engine to construct or to use.** Construction
  moves onto `Quill`:
  - `Quill::from_tree(FileTreeNode) -> Result<Quill, …>` (pure;
    core-reachable)
  - `Quill::from_path(path) -> Result<Quill, …>` (filesystem walk stays
    in `quillmark`; core remains fs-agnostic). *Landed as a free fn,
    `quillmark::quill_from_path` — see Follow-up.*
- **A `Quill` is portable across engines.** It is `Send + Sync` data
  tagged with intent ("I want backend `typst`"); any engine with a
  matching backend can render it. Same id may map to different backend
  impls per engine — expected.
- **The backend-existence check moves from load to render time.** Loading
  never needs a backend (which is what lets a Typst-less core build load
  and validate). `engine.open` / `render` returns `UnsupportedBackend`
  when no registered backend matches `quill.backend_id()`.

### The engine is render-only

`Quillmark` becomes a backend registry + render dispatcher. **Locked
surface** — keep both verbs:

```
Quillmark::new() / register_backend(...)
engine.open(&quill, &doc) -> RenderSession
engine.render(&quill, &doc, opts) -> RenderResult   // one-shot convenience
engine.supported_formats(&quill)
engine.supports_canvas(&quill)
```

Keep `render` *and* `open`: `render` is `open` + `session.render` with a
default-format fill — a free convenience over the canonical session, no
duplicated work — so deleting it only makes the one-shot case two calls
and churns CLI/Python for nothing. The `RenderSession` is the canonical
renderable; `render` is the open-and-render-once sugar.

`RenderSession` is unchanged as a renderable (`render` / `paint` /
`pageSize` / warnings) and additionally mirrors the resolved-backend
capability (`supportedFormats`, `supportsCanvas`) so the render flow can
read it from the live session without a second engine call.

**Remove the factory.** `engine.quill(tree)` / `engine.quill_from_path`
are deleted; `Quill::from_tree` / `Quill::from_path` are the only
constructors, in both builds. The engine no longer loads quills — it only
renders them.

### Capability lives on the engine

Every question that depends on *which backend is resolved* — `supported_formats`,
`supports_canvas` — is answered by the thing that holds the resolved
backend, never by core `Quill`. The engine is the right home, not the
`RenderSession`: the engine knows its registered backends without opening
anything, so capability is a **cheap, static, non-failing** query, whereas
`engine.open` compiles the document (Typst's expensive, fallible step). A
capability question is a *pre-render gate* — `supports_canvas` exists to
decide whether to mount a canvas UI *before* committing to render — so it
must not require `open`; that would invert precondition→action. And
`supported_formats` is a backend-static fact (Typst is always PDF/SVG/PNG);
gating it behind a fallible per-document `open` would couple a static
capability to one document's validity. So:

- `engine.supported_formats(&quill)` / `engine.supports_canvas(&quill)` —
  static capability, render build, no compile.
- `RenderSession` mirrors both for the render flow once a session exists.

This retires the `backend_id == "typst"` magic string. Capability is
answered by asking the real backend, not by guessing from an id. The id
itself remains as declared intent:

- `backend_id` — stays on `Quill` (it is `config.backend`, pure data,
  core-reachable). A core editor that wants a cheap "probably previewable"
  heuristic can compare it itself; the library no longer blesses the string
  as a capability.

### `supportedFormats` is render-side: deferred, not lost

A core editor might want the export-format list up front. We **accept the
boundary** rather than expose formats from core, because both ways to do so
re-introduce the coupling the split exists to remove:

- A static `backend_id → &[formats]` table in core is the magic string one
  level up: it forks the source of truth (the backend crate *is* the
  authority on what it emits) and goes stale on any backend change. It also
  contradicts the portability claim above — the same id may map to
  different backend impls per engine, so a static core table assuming one
  canonical impl is wrong.
- Declaring formats in `Quill.yaml` changes the meaning: "what a backend can
  emit" is a capability the author doesn't control, so a manifest field
  would let the manifest lie. (Author-*intended* targets — "this is a print
  quill, PDF only" — are a legitimately different, curation concept that
  could live in core config, but that is a separate feature, not this split.)

The boundary is not actually lossy for any real flow: a core-only editor
validates and seeds, it does not export. By the time the user exports,
render is lazy-loaded and `engine.supported_formats(&quill)` answers
cheaply. The fact is *deferred to when render is in scope*, the same as
every other render-side fact — not lost.

### Resulting binding surfaces

- **Core build** = `Document` + `Quill`. **No `Quillmark` engine.**
  Construct via `Quill.fromTree(...)`; then validate / schema / metadata
  (identity only, no `supportedFormats`) / seed / blueprint / `compile_data`
  / `backendId` (declared). No capability surface.
- **Render build** = core + `Quillmark` (engine) + `RenderSession` +
  Typst. Same `Quill.fromTree(...)` constructor. Capability
  (`supportedFormats` / `supportsCanvas`) and rendering go through the
  engine; the `RenderSession` carries the live render surface and mirrors
  capability.

| Surface | Home | Build |
|---|---|---|
| `backendId` (declared string) | `Quill` | core |
| `validate` · `schema` · `metadata` · `blueprint` · `seed_*` · `compileData` | `Quill` (pure config) | core |
| `supported_formats(&quill)` · `supports_canvas(&quill)` | `Quillmark` engine (static) | render |
| `open` · `render` (sugar) | `Quillmark` engine | render |
| `render(opts)` · `paint` · `pageSize` · `pageCount` · `warnings` + capability mirror | `RenderSession` | render |

### Cross-module handoff is data, not handles

Two WASM modules have separate linear memories; a `Quill` / `Document`
handle from core is unusable by render. This is fine because both models
are serializable: a quill is a `Map<string, Uint8Array>` tree, and a
`Document` round-trips through `toJson` / `fromJson`. Intended flow: the
editor boots **core** (~0.66 MB gzip) for schema/validation/seeding; on
preview/export it lazy-loads **render** (superset) and re-feeds the tree +
`doc.toJson()`. The cross-module case is just the extreme of `Quill`
being portable data.

## Plan

### Phase 1 — Engine-free `Quill` (orchestration / core)
- Drop `Arc<dyn Backend>` from `Quill`; hold only `Arc<QuillSource>`.
- Add `Quill::from_tree` (pure) and `Quill::from_path` (fs walk in
  `quillmark`); delete `engine.quill` / `engine.quill_from_path`.
- Move `open` / `render` / `supported_formats` / `supports_canvas` onto
  `Quillmark`; resolve backend at render time, error `UnsupportedBackend`
  on no match. `render` stays as `open` + `session.render` sugar.
- Strip `supportedFormats` out of `Quill::metadata`; it becomes a pure
  identity/config read. `supportedFormats` is read via the engine instead.
- Move `seed_*` to take `&QuillSource` (already only reads `config()`).
- `compile_data` / `validate` stay pure, hung off `Quill`.
- **Same-PR, not follow-up:** update Python/CLI call sites. Both bind the
  exact surface that moves — `engine.quill_from_path`, `quill.render` /
  `open` / `supported_formats` / `validate` / `seed_*` — so the workspace
  will not compile until they migrate to `Quill::from_*` + `engine.*`.
  `cargo test --workspace` is the gate. (Feature-*splitting* those bindings
  remains out of scope; keeping them compiling is not.)

### Phase 2 — Feature-gate the binding
- `default = ["render"]`; `core` = no Typst.
- Gate behind `#[cfg(feature = "render")]`: the `Quillmark` engine,
  `RenderSession`, canvas paint, and all `quillmark_typst::` /
  backend imports. `quillmark` dep uses `default-features = false` in
  the core build.
- Keep the shared TS (`QuillSchema` / `Card` / metadata
  `typescript_custom_section`) in one module compiled into both.

### Phase 3 — Build, package, CI
- `build-wasm.sh`: build twice (core / render) → `pkg/core/`, `pkg/render/`.
- `package.template.json`: two `exports` entry points; render lazy-loadable.
- CI/release: build + test both; size-budget check on core to keep Typst
  from creeping back in.

### Phase 4 — Tests & docs
- Core JS tests: load → schema → validate → seed → blueprint; assert no
  render API and no capability surface present (no `supportedFormats` on
  `metadata`, no `supportsCanvas`).
- Render tests: existing suite, plus capability now read via the engine
  (`supported_formats` / `supports_canvas`) and mirrored on the session.
- Update `ARCHITECTURE.md`, `QUILL.md` (the `QuillSource` vs `Quill`
  split: `Quill` no longer holds a backend; engine no longer constructs
  it), and `PREVIEW.md`. Document the data-not-handles handoff.
- Write the working (unreleased) migration guide under `docs/migrations/`
  for the 0.89 → next breaking step: the removed `engine.quill` /
  `quill_from_path` factory (→ `Quill.fromTree` / `from_path`), `render` /
  `open` / `supported_formats` / `supports_canvas` moving off `Quill` onto
  the engine, `supportedFormats` dropped from `Quill.metadata`, and the
  backend now resolved at render time. Add its `docs/migrations/index.md`
  entry. (Released guides are immutable; only this working one is mutable.)

## Follow-up: collapse `Quill` / `QuillSource` — **landed**

> **Done in this branch.** Bundled with the split rather than deferred: the
> refactor was already breaking, so consolidating the trauma into one change
> beat spreading a second core-rename wave across a later release.

Once `Quill` held no backend it was just `QuillSource` + config-read methods;
the only difference was the method set and a wrapper. That was accidental
duplication, not a concept boundary, so the two types collapsed into one.

**As landed:**
- `QuillSource` is renamed to **`Quill`** in `quillmark-core`; the orchestration
  wrapper is deleted and `quillmark` re-exports the core type.
- `Backend::open` takes `&Quill`. The consumer methods (`compile_data`,
  `validate`, `dry_run`, `seed_*`, `check_quill_reference`) and the `seed`
  module moved into core, since inherent methods must live with the type.
- `Quill::from_tree` is the core constructor (→ `Vec<Diagnostic>`);
  `quillmark::quill_from_path` / `quill_from_tree` are free functions that
  surface a `RenderError` and keep fs out of core.
- The `Arc<QuillSource>` was vestigial and is dropped — `Quill` is held by value.
- The `.source()` indirection is gone: `self.inner.source().config()` →
  `self.inner.config()` across the engine and bindings. JS/Python consumers see
  no change (bindings already hid `QuillSource`).

The original deferral rationale (kept for the record) follows.

**Constraint — collapse goes *into* core.** The reason `QuillSource`
exists is the `Backend` trait: `open(plate, &QuillSource, json)`. Backends
live in crates that depend on `quillmark-core`, not on `quillmark`, so the
unified type must live in core and be the value handed to backends. That
rules out merging upward into `quillmark::Quill`.

**Resulting shape.** One core type named **`Quill`** (drop the
`QuillSource` name — it matches the domain noun and the existing binding
surface; the `quillmark` crate re-exports it), `Backend::open(&Quill)`,
`Quill::from_tree` in core, `from_path` as a `quillmark` free function (fs
stays out of core). The name migrates *down* into core: today `Quill` is
the orchestration wrapper and `QuillSource` the core data type; the
collapse deletes the wrapper and renames the core type. Bindings already
expose only `Quill` and hide `QuillSource`, so JS/Python consumers see no
change from the rename.

**Concrete win beyond dedup:** the `.source()` indirection disappears.
Every `self.source.config()` / `self.inner.source().config()` (pervasive
in orchestration and the WASM getters) becomes a direct `quill.config()`.
The collapse is a readability gain across the codebase, not only the
removal of a redundant type — which is the argument for doing it rather
than leaving the two-type shape.

**Resolved fork — `Arc`:** the `Arc<QuillSource>` was vestigial (nothing else
shares the source), so it is dropped; `Quill` is held by value and multi-thread
consumers wrap it themselves.

## Out of scope
- *Feature-splitting* Python / CLI into core/render builds (best-effort
  follow-up). Keeping them compiling against the moved surface is **in**
  scope for Phase 1 — see there.
- Sharing one linear memory across modules; new render features.
