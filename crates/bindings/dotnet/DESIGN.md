# .NET binding — design

The .NET binding's public surface mirrors the [Python binding](../python)
method-for-method (in .NET casing) and shares its single-exception error
contract. This document records the interop design and trade-offs.

## The interop options considered

| Option | Verdict |
|--------|---------|
| **Hand-written C ABI (`cdylib`) + P/Invoke** | **Chosen.** Zero extra build tooling, mirrors how PyO3 ships a native module + thin language layer, and the `quillmark` crate already produces clean `serde` JSON for every structured type. Fully under our control. |
| `csbindgen` / `Interoptopus` (generate C# from Rust) | Viable later as a *codegen* over the same ABI to cut the hand-written `NativeMethods.cs`, but adds a build-time dependency and indirection. Revisit once the surface churns less. |
| UniFFI (multi-language from one Rust IDL) | Most "symmetrical to Python" in spirit (it targets Python too), but it would mean re-expressing the surface in UDL and adopting a third-party .NET backend — a larger commitment that fights the existing per-binding hand-tuned surfaces. |

**Key simplification.** C has no rich object marshaling, so instead of mirroring
PyO3's per-method object conversions, the ABI uses **JSON as the structured-data
currency** and **opaque handles** for stateful objects. Cards, schema, metadata,
diagnostics, and field values all cross as UTF-8 JSON serialized by the *same*
core `serde` types (`CardWire`, `Diagnostic`, `RenderOptions`, …) the Python and
WASM bindings use. The typed C# surface (`Document.SetField`, `Quill.Metadata`,
…) is reassembled on top. This keeps the ABI small (~68 functions for the whole
surface) and guarantees the data shapes can never drift from the other bindings.

## Layers

- **`src/abi.rs`** — marshaling primitives: owned C strings, an owned byte
  buffer (`QmBytes`) for artifact bytes, opaque handle boxing/freeing, and a
  thread-local last-error.
- **`src/lib.rs`** — the full `qm_*` surface over `quillmark`: engine
  (`new`/`render`/`supported_formats`/`registered_backends`), `Quill`
  (`from_path` + all readers + `validate` + seeding), `Document` (constructors,
  statics, readers, every main-card and composable-card mutator, the `$ext`
  family), and `RenderResult`/`Artifact` accessors. The logic mirrors
  `python/src/{types,enums,errors}.rs`; only the marshaling differs
  (serde-JSON strings instead of PyO3 objects).
- **`csharp/Quillmark/`** — the managed surface: `Quillmark`, `Quill`,
  `Document`, `RenderResult`, `Artifact`, `Diagnostic`, `Location`, `Card`, the
  `OutputFormat`/`Severity` enums, and `QuillmarkException`. Handles use the
  standard dispose pattern (`NativeObject`) so each Rust `Box` is freed exactly
  once.

## Error contract (symmetry preserved)

Python raises one exception type (`QuillmarkError`) always carrying a non-empty
`.diagnostics` list. The ABI reproduces this without a richer-than-C protocol: a
fallible call returns a null pointer (handle/string) or `-1` (status) and parks
a JSON `{ message, diagnostics }` payload in a thread-local that C# drains into a
`QuillmarkException` with a non-empty `.Diagnostics`. Optional *values* are
encoded as JSON `null` inside a valid string, so a null pointer unambiguously
means a real failure (the one benign exception, `try_from_json`, clears the
error slot and returns a null handle by contract).

## Build, test, release

- **Local:** `./scripts/build-dotnet.sh` builds the native cdylib then the
  managed assembly/tests (mirrors `build-wasm.sh`). The managed
  `Quillmark.csproj` copies the cargo-built native library next to its output
  via the `CopyNativeLibrary` target.
- **CI:** the `dotnet` job in `.github/workflows/ci.yml` builds the cdylib,
  installs the .NET SDK, and runs `Quillmark.Tests` against the real Typst
  backend on every PR.
- **Release:** `build-dotnet-native` builds the cdylib per RID
  (`linux-x64`, `win-x64`, `osx-x64`, `osx-arm64`); `publish-dotnet` stages each
  under `runtimes/<rid>/native/`, runs `dotnet pack`, and pushes the `Quillmark`
  package to NuGet (requires the `NUGET_API_KEY` secret).

## Open items / known limitations

1. **`linux-arm64`** is not yet in the release matrix (no first-party hosted
   runner used here); add via cross-compilation or an arm runner when needed.
2. **`QmBytes` returned by value (assumption, not a defect).** Artifact bytes
   cross as a 16-byte `{ ptr, len }` blittable struct returned by value. The
   binding assumes P/Invoke marshals this correctly per platform ABI, which
   holds for every shipped RID (SysV x86-64 `RAX:RDX`, Win64 hidden-pointer,
   AArch64 `X0:X1`) and is exercised end-to-end by the render test (which reads
   the bytes and checks the `%PDF` header), so an ABI mismatch on a future
   target would fail CI loudly rather than corrupt silently. Switching to an
   out-param (or a length-then-copy two-call) would erase the assumption but is
   speculative until such a target exists.
3. **Thread-local error.** Matches PyO3's implicit per-call model; correct as
   long as each fallible call's result/error is consumed before the next call on
   the same thread, which the C# wrappers do inline.
4. **No `RenderSession`/canvas.** Same scope as Python (one-shot `render` only);
   the iterative preview surface stays WASM-only.
5. **`NativeMethods.cs` is hand-written.** Consider `csbindgen` codegen over the
   ABI once it stabilizes.
