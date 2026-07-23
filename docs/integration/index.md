# Integrating Quillmark

Quillmark embeds in an app through two published bindings — **Python** (`quillmark` on PyPI) and **JavaScript/WASM** (`@quillmark/wasm` on npm) — over one core engine. Rust consumers use the crates directly ([docs.rs](https://docs.rs/quillmark)).

Three objects carry every integration:

- **Engine** — a backend registry and render dispatcher. It holds no quills; every render routes through it, and it never constructs a quill. `Quillmark()` in Python, `new Engine()` in JavaScript.
- **Quill** — portable, declarative config data (schema, plate, assets). Loaded once, rendered by any engine whose backend matches; it never renders itself.
- **Document** — the typed model of one Quillmark Markdown file (root block, body, cards). Built from Markdown, from a blank canvas, or from stored JSON.

Field I/O is schema-bound: `quill.writer(doc)` writes each field by its declared type and `quill.reader(doc)` reads it back. `Document` itself is quill-free data — parse, persist, structure.

Both bindings share the core model, diagnostics, and storage format; they differ in language idiom (`snake_case` vs. camelCase), packaging, and extras (canvas preview is JavaScript-only). Exact method signatures live with each binding — `import quillmark` and the `@quillmark/wasm` `.d.ts` — rather than in a table here that would drift as the pre-1.0 surface moves; the task pages below show the calls in context.

## Where to go

- **[Programmatic Construction](programmatic.md)** — build and mutate a `Document` in memory (database row → PDF).
- **[Error Handling](error-handling.md)** — the diagnostic model every binding raises.
- **[Persistence](persistence.md)** — store a `Document` as versioned JSON.
- **[Blueprint & Seeding](../quills/blueprint.md)** — the LLM/MCP authoring surface and its filled-out twin.

!!! note "Advanced: live preview"
    Canvas / `LiveSession` editor integration — incremental `apply`, `paint`, and the click/caret geometry bridge — is WASM-only and single-consumer today. Its design contract lives in [PREVIEW.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PREVIEW.md); the [Quickstart](../getting-started/quickstart.md#live-preview-canvas) has the minimal paint loop.
