# Bindings

> **Implementation**: `crates/bindings/`

## TL;DR

Quillmark exposes one core engine through several language surfaces — Python
(PyO3), WebAssembly (wasm-bindgen), .NET (P/Invoke over a C ABI), and a CLI
binary. Every surface drives the same `quillmark` core: the same
`Document`/`Quill`/`Card` model, the same `serde` diagnostics, and the same
capability principle. Surfaces differ only in language idiom, packaging, and
which extras they expose (canvas preview is WASM-only).

## Shared model

- **Capability principle.** A `Quill` is portable, declarative config data.
  Its format capability (`supportedFormats`) and rendering are resolved by the
  `Quillmark` engine *against* a quill at render time — never by the quill
  itself. So `quill.metadata` is a pure, infallible config snapshot, while
  `render` / `supportedFormats` can fail for an unregistered backend.
- **One model, serialized across every boundary.** The `Document`/`Card` model
  and `Diagnostic`s cross each language boundary as the same core `serde`
  shapes (`CardWire`, the versioned storage DTO, `Diagnostic`) — so a card or
  an error reads identically no matter which surface emits it. See
  [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [CARDS.md](CARDS.md),
  [ERROR.md](ERROR.md).
- **Uniform errors.** Each binding raises a single error type that always
  carries a non-empty diagnostic list (`QuillmarkError.diagnostics` /
  thrown `Error.diagnostics` / `QuillmarkException.Diagnostics`).

The WASM binding is the reference surface; Python and .NET mirror it and catch
up on a best-effort basis (see their status notes below). New contract work
lands in WASM first.

## Python — `bindings/quillmark-python`

PyO3 bindings published as `quillmark` on PyPI. A `snake_case` surface mirroring
the shared model; one-shot `engine.render` (no canvas).

> **Status: experimental, second-class binding.** The Python surface lags the
> WASM binding in coverage and in error-shape uniformity. Do not gate releases
> on Python parity.

## WebAssembly — `bindings/quillmark-wasm`

wasm-bindgen bindings published as `@quillmark/wasm`. Builds with
`--target bundler` and `--weak-refs` so wasm-bindgen handles are reclaimed by
`FinalizationRegistry`; `.free()` remains as the eager teardown hook. Requires
Node 22+ / current evergreen browsers.

Ships **multiple artifacts from one crate** behind a single public root export.
The root `@quillmark/wasm` is a hand-written **canonical runtime layer** that
re-exports the internal Typst-less **core** build's `Document` + `Quill`
(load / validate / schema / seed / blueprint) verbatim and adds an `Engine`
render dispatcher. Each backend (Typst today) is a **private** build with its
own linear memory, lazily loaded on the first render — there is no public
`/core` or `/render` subpath. The core build is ~0.66 MB gzip; the Typst backend
~8 MB (Typst dominates), loaded only when something renders. Backend handles
never escape the `Engine`: it clones the quill tree + `doc.toJson()` into the
backend's memory as serialized data and frees the clones. See the
[as-built 0.90 design](../../docs/migrations/0.89-to-0.90.md).

Beyond the byte-output verbs (`engine.render`, `RenderSession.render`), the
canvas-capable backend builds (Typst, and pdfform under its preview seam)
expose a **canvas preview** path on `RenderSession` (`pageCount`, `pageSize`,
`paint`, …). See [PREVIEW.md](PREVIEW.md).

## .NET — `bindings/quillmark-dotnet`

C-ABI `cdylib` consumed from C# via P/Invoke, published as `Quillmark` on
NuGet — the .NET analogue of the PyO3 module. A flat `qm_*` C ABI over
`quillmark` plus a hand-written managed layer (`csharp/`) that reassembles the
typed surface, deliberately **symmetrical with the Python binding**
method-for-method. Structured data (cards, schema, metadata, diagnostics, field
values) crosses as `serde` JSON from the same core types; stateful objects
cross as opaque handles; panics are trapped at the boundary (the analogue of
PyO3's trapping / the WASM panic hook) and surface as the single
`QuillmarkException`. The NuGet package carries the native library per RID under
`runtimes/<rid>/native/`.

> **Status: experimental, second-class binding.** Mirrors the Python surface and
> shares its footing — render-only (no canvas preview), best-effort parity, not
> a release gate.

## CLI — `bindings/quillmark-cli`

Standalone `quillmark` binary. See [CLI.md](CLI.md).

## Links

- [CLI.md](CLI.md) — command-line surface
- [PREVIEW.md](PREVIEW.md) — WASM multi-backend canvas preview (Typst, pdfform)
- [ERROR.md](ERROR.md) — the diagnostic model that crosses every boundary
- Per-binding API detail: the respective `crates/bindings/*/` rustdoc and READMEs
