# Quillmark for .NET

Schema-driven document engine: Markdown + YAML card metadata → a fully typeset **PDF / SVG / PNG** via a Typst backend.
The managed surface mirrors the Python and WASM bindings.

## Install

```bash
dotnet add package Quillmark
```

Native libraries for `linux-x64`, `win-x64`, `osx-x64`, and `osx-arm64` ship in
the package; no extra setup. Targets .NET 8+.

## Quick start

```csharp
using Quillmark;

using var engine = new QuillmarkEngine();              // backend registry + dispatcher
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

`QuillmarkEngine`, `Quill`, `Document`, and `RenderResult` own native resources —
wrap them in `using` (or `Dispose()`).

## API

**`QuillmarkEngine`** — `Render(quill, doc, format?, ppi?, pages?, producer?)`,
`SupportedFormats(quill)`, `RegisteredBackends()`.

**`Quill`** — `FromPath(path)`; then `BackendId`, `QuillRef`, `Metadata`,
`Schema` (`JsonElement`), `Blueprint`, `Validate(doc)`, `SeedDocument()`,
`SeedMain()`, `SeedCard(kind)`.

**`Document`**
- Construct: `FromMarkdown`, `FromJson`, `TryFromJson` (null when not a stored doc).
- Persist: `ToMarkdown()`, `ToJson()` (versioned, byte-deterministic).
- Read: `Main`, `Cards`, `Body`, `CardCount`, `Warnings`, `QuillRef`.
- Edit main card: `SetField`, `SetFill`, `RemoveField`, `ReplaceBody`, `SetQuillRef`.
- Edit cards: `MakeCard(kind, fields?, body?)`, `PushCard`, `InsertCard`,
  `RemoveCard`, `MoveCard`, `SetCardKind`, `UpdateCardField`, `RemoveCardField`,
  `UpdateCardBody`.
- Consumer state: `SetExt*` / `RemoveExt*` (and `Card`-indexed variants).
- Seed overlays: `SetSeedNamespace(kind, overlay)` / `RemoveSeedNamespace(kind)`
  (root-only; feeds `Quill.SeedCard`).

**`RenderResult`** — `Artifacts`, `Warnings`, `Format`, `RenderTimeMs`.
**`Artifact`** — `Bytes`, `Format`, `MimeType`, `Save(path)`.

Field values accept any JSON-serializable object; `Metadata`, `Schema`, and card
payloads surface as `System.Text.Json` values.

## Error handling

Every failure throws `QuillmarkException`, always carrying a non-empty
`.Diagnostics` list.

```csharp
try
{
    using var doc = Document.FromMarkdown(badMarkdown);
}
catch (QuillmarkException ex)
{
    foreach (Diagnostic d in ex.Diagnostics)
    {
        Console.WriteLine(d);                      // pretty-printed
        // d.Severity, d.Code, d.Message, d.Path, d.Location, d.Hint
    }
}
```

## Notes

- **Render-only.** One-shot `engine.Render`; the iterative canvas-preview surface
  is WASM-only.
- Building from source: `./scripts/build-dotnet.sh` (native cdylib + `dotnet test`).

## License

Apache-2.0
