# Quillmark — .NET bindings

.NET bindings for Quillmark's format-first Markdown rendering engine,
**symmetrical with the [Python binding](../python)**: the same concepts, the
same method names (in .NET casing), and the same single-exception error
contract.

Tested in CI (the `dotnet` job) and published to NuGet as the `Quillmark`
package. The interop design and error contract are documented in the
crate-level rustdoc (`src/lib.rs`); the binding's place among the language
surfaces is covered by the canon bindings doc (`prose/canon/BINDINGS.md`).

## Architecture

```
┌────────────────────┐   P/Invoke    ┌──────────────────────────┐   Rust    ┌───────────┐
│  Quillmark (C#)     │ ───────────▶ │  quillmark_dotnet (cdylib) │ ───────▶ │ quillmark │
│  idiomatic surface  │   qm_* C ABI  │  C-ABI marshaling layer    │           │  (core)   │
└────────────────────┘               └──────────────────────────┘           └───────────┘
```

This is the .NET analogue of how the Python binding ships a PyO3 extension
module: one Rust crate (`crates/bindings/dotnet`) exposing a flat C ABI, plus a
hand-written managed layer (`csharp/Quillmark`) that reassembles the idiomatic,
typed API. Structured data (cards, schema, metadata, diagnostics, field values)
crosses the boundary as JSON produced by the **same `serde` types** the other
bindings use, so the shapes never drift.

## Layout

| Path | What |
|------|------|
| `Cargo.toml`, `src/` | The native `cdylib` and its C ABI (`qm_*`). |
| `csharp/Quillmark/` | The managed `Quillmark` assembly (public API). |
| `csharp/QuillDemo/` | Console demo mirroring `python/examples/quill_demo.py`. |
| `csharp/Quillmark.Tests/` | xUnit suite mirroring the Python `tests/`. |

## Quick start

```csharp
using Quillmark;

using var engine = new Quillmark.Quillmark();          // backend registry + dispatcher
using var quill  = Quill.FromPath("path/to/quill");    // engine-free, validated config

using var doc = Document.FromMarkdown("""
~~~
$quill: my_quill
$kind: main
title: Hello World
~~~

# Hello
""");

using RenderResult result = engine.Render(quill, doc, OutputFormat.Pdf);
result.Artifacts[0].Save("output.pdf");
```

## API mapping (Python → .NET)

| Python | .NET |
|--------|------|
| `Quillmark()` | `new Quillmark()` |
| `engine.render(quill, doc, OutputFormat.PDF, ppi=, pages=, producer=)` | `engine.Render(quill, doc, OutputFormat.Pdf, ppi:, pages:, producer:)` |
| `engine.supported_formats(quill)` | `engine.SupportedFormats(quill)` |
| `engine.registered_backends()` | `engine.RegisteredBackends()` |
| `Quill.from_path(p)` | `Quill.FromPath(p)` |
| `quill.metadata` / `.schema` | `quill.Metadata` / `.Schema` (`JsonElement`) |
| `quill.backend_id` / `.blueprint` / `.quill_ref` | `quill.BackendId` / `.Blueprint` / `.QuillRef` |
| `quill.validate(doc)` | `quill.Validate(doc)` |
| `quill.seed_document()` / `seed_main()` / `seed_card(k)` | `quill.SeedDocument()` / `SeedMain()` / `SeedCard(k)` |
| `Document.from_markdown` / `from_json` / `try_from_json` | `Document.FromMarkdown` / `FromJson` / `TryFromJson` |
| `Document.make_card(kind, fields, body)` | `Document.MakeCard(kind, fields, body)` |
| `doc.to_markdown()` / `to_json()` / `clone()` / `equals()` | `doc.ToMarkdown()` / `ToJson()` / `Clone()` / `Equals()` |
| `doc.main` / `cards` / `body` / `card_count` / `warnings` | `doc.Main` / `Cards` / `Body` / `CardCount` / `Warnings` |
| `doc.set_field` / `set_fill` / `remove_field` | `doc.SetField` / `SetFill` / `RemoveField` |
| `doc.push_card` / `insert_card` / `remove_card` / `move_card` | `doc.PushCard` / `InsertCard` / `RemoveCard` / `MoveCard` |
| `doc.set_ext*` / `remove_ext*` (+ card variants) | `doc.SetExt*` / `RemoveExt*` (+ `Card` variants) |
| `result.artifacts[0].save(...)` / `.bytes` / `.mime_type` | `result.Artifacts[0].Save(...)` / `.Bytes` / `.MimeType` |
| `QuillmarkError` (`.diagnostics`) | `QuillmarkException` (`.Diagnostics`) |

## Error contract

A single exception type — `QuillmarkException` — is thrown for every failure
mode, always carrying a non-empty `.Diagnostics` list, exactly like Python's
`QuillmarkError.diagnostics` and the WASM binding's thrown error.

```csharp
try
{
    Document.FromMarkdown(badMarkdown);
}
catch (QuillmarkException ex)
{
    foreach (Diagnostic d in ex.Diagnostics)
    {
        Console.WriteLine($"{d.Severity} {d.Code} {d.Message} {d.Path}");
        Console.WriteLine(d.ToString());
    }
}
```

## Build & test

```bash
./scripts/build-dotnet.sh            # native cdylib + managed build + tests
./scripts/build-dotnet.sh --release  # release variant
```

Or manually:

```bash
cargo build -p quillmark-dotnet
cd crates/bindings/dotnet/csharp
dotnet test Quillmark.Tests/Quillmark.Tests.csproj
```

The managed `Quillmark.csproj` copies the cargo-built native library
(`libquillmark_dotnet.so` / `.dylib` / `.dll`) next to its output so the
default P/Invoke resolver finds it; build the native crate first.

## Known limitations

- **Render-only.** Same scope as the Python binding — one-shot `engine.Render`;
  the iterative `RenderSession`/canvas-preview surface stays WASM-only.
- **Release RIDs.** The NuGet package ships native libraries for `linux-x64`,
  `win-x64`, `osx-x64`, and `osx-arm64`. `linux-arm64` is not yet in the matrix
  (add via cross-compilation or an arm runner when needed).
- **`QmBytes` by-value return.** Artifact bytes cross as a blittable 16-byte
  `{ ptr, len }` struct returned by value; correct on every shipped RID and
  exercised end-to-end by the render test (which checks the `%PDF` header), so a
  future ABI mismatch fails CI loudly rather than corrupting silently.
- **Hand-written P/Invoke.** `NativeMethods.cs` is maintained by hand; consider
  `csbindgen` codegen over the C ABI once the surface stabilizes.

## License

Apache-2.0
