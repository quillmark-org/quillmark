# Quickstart

Get started with Quillmark in Python or JavaScript.

=== "Python"

    ## Installation

    ```bash
    uv pip install quillmark
    ```

    ## Basic Usage

    ```python
    from quillmark import Document, Quillmark, OutputFormat

    engine = Quillmark()
    quill = engine.quill_from_path("path/to/quill")

    markdown = """---
    QUILL: my_quill
    title: Example Document
    ---

    # Hello World
    """

    doc = Document.from_markdown(markdown)
    result = quill.render(doc, OutputFormat.PDF)

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
    import { Document, Quillmark } from "@quillmark/wasm";

    const engine = new Quillmark();
    const enc = new TextEncoder();

    const quill = engine.quill(new Map([
      ["Quill.yaml", enc.encode("quill:\n  name: my_quill\n  backend: typst\n  version: \"1.0.0\"\n  description: Demo\n  plate_file: plate.typ\n")],
      ["plate.typ", enc.encode("#import \"@local/quillmark-helper:0.1.0\": data\n#data.BODY\n")],
    ]));

    const markdown = `---
    QUILL: my_quill
    title: Example Document
    ---

    # Hello World`;

    const doc = Document.fromMarkdown(markdown);
    const result = quill.render(doc, { format: "pdf" });
    const pdfBytes = result.artifacts[0].bytes;
    ```

    ## Live Preview (Canvas)

    For editor-style previews, paint pages directly into a `<canvas>` instead
    of round-tripping through PNG/SVG. `paint` is Typst-only and WASM-only,
    and shares the cached compile with the byte-output `render` path.

    ```javascript
    const session = quill.open(doc);                   // compile once

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
    - `layoutScale` (default `1`) is layout pixels per Typst point — how big
      the page looks. `densityScale` (default `1`) is the backing-store
      density multiplier — how sharp it is. Fold `devicePixelRatio`, in-app
      zoom, and `visualViewport.scale` into one `densityScale` value.
    - If `layoutScale * densityScale` would push either dimension past 16384
      px, `densityScale` is clamped to fit; compare `result.pixelWidth` to
      `round(result.layoutWidth * densityScale)` to detect the clamp.
    - Pass an `OffscreenCanvasRenderingContext2D` to rasterize off the main
      thread.
    - Canvas pixels are opaque to the DOM — there's no text selection or
      find-in-page. Keep an SVG/PDF path alongside if you need either.

    Full design rationale: [PREVIEW.md](https://github.com/nibsbin/quillmark/blob/main/prose/designs/PREVIEW.md).
