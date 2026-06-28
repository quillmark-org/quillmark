# Quickstart

=== "Python"

    ## Installation

    ```bash
    uv pip install quillmark
    ```

    ## Basic Usage

    ```python
    from quillmark import Document, Quill, Quillmark, OutputFormat

    engine = Quillmark()                       # backend registry + render dispatcher
    quill = Quill.from_path("path/to/quill")   # portable, declarative config data

    markdown = """~~~
    $quill: my_quill
    $kind: main
    title: Example Document
    ~~~

    # Hello World
    """

    doc = Document.from_markdown(markdown)
    result = engine.render(quill, doc, OutputFormat.PDF)

    with open("output.pdf", "wb") as f:
        f.write(result.artifacts[0].bytes)
    ```

=== "JavaScript"

    ## Installation

    ```bash
    npm install @quillmark/wasm
    ```

    ## Basic Usage

    ```javascript
    // The single root import is the canonical API: Quill/Document (re-exported
    // from the internal Typst-less core build) plus the Engine render
    // dispatcher. An editor that only validates uses Quill/Document and loads
    // no backend — Typst loads lazily on the first render.
    import { Document, Quill, Engine } from "@quillmark/wasm";

    const enc = new TextEncoder();

    // A Quill is portable, declarative data — no engine needed to load it.
    const quill = Quill.fromTree(new Map([
      ["Quill.yaml", enc.encode("quill:\n  name: my_quill\n  backend: typst\n  version: 1.0.0\n  description: Demo\n\ntypst:\n  plate_file: plate.typ\n")],
      ["plate.typ", enc.encode("#import \"@local/quillmark-helper:0.1.0\": data\n#data.at(\"$body\")\n")],
    ]));

    const markdown = `~~~
    $quill: my_quill
    $kind: main
    title: Example Document
    ~~~

    # Hello World`;

    const doc = Document.fromMarkdown(markdown);

    // Rendering goes through the Engine. Its methods are async — the first call
    // lazily loads the Typst backend binary; the canonical quill crosses into
    // backend memory internally (no manual fromTree/fromJson needed).
    const engine = new Engine();
    const result = await engine.render(quill, doc, { format: "pdf" });
    const pdfBytes = result.artifacts[0].bytes;
    ```

    ## Live Preview (Canvas)

    For editor-style previews, paint pages directly into a `<canvas>` instead
    of round-tripping through PNG/SVG. `paint` is Typst-only and WASM-only,
    and shares the cached compile with the byte-output `render` path.

    ```javascript
    const session = await engine.open(quill, doc);     // compile once (async)

    // Surface session-level diagnostics from compile time.
    for (const w of session.warnings) console.warn(w.message);

    function renderPage(canvas, page, userZoom = 1) {
      const densityScale = (window.devicePixelRatio || 1) * userZoom;
      const result = session.paint(canvas.getContext("2d"), page, {
        layoutScale: 1,
        densityScale,
      });
      canvas.style.width  = `${result.layoutWidth}px`;
      canvas.style.height = `${result.layoutHeight}px`;
    }

    for (let p = 0; p < session.pageCount; p++) renderPage(canvases[p], p);

    session.free();                                    // when doc changes
    ```

    Key contract points:

    - The painter owns `canvas.width` / `canvas.height` and rewrites them on
      every call (so each `paint` is a full repaint — no `clearRect` needed).
      The consumer owns `canvas.style.*` and reads `result.layoutWidth` /
      `layoutHeight` to size the display box.
    - Fold `devicePixelRatio` and in-app zoom into `densityScale`;
      `layoutScale` controls display size.
    - If `layoutScale * densityScale` would push either dimension past 16384
      px, `densityScale` is clamped to fit; compare `result.pixelWidth` to
      `round(result.layoutWidth * densityScale)` to detect the clamp.
    - `pageCount` and `pageSize(page)` are stable for the session's lifetime
      (the compiled document is an immutable snapshot) — cache them.

    Full design rationale: [PREVIEW.md](https://github.com/quillmark-org/quillmark/blob/main/prose/canon/PREVIEW.md).
