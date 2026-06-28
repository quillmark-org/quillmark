# Canvas Preview (WASM)

> **Implementation**: `crates/core/src/session.rs`, `crates/backends/typst/src/`, `crates/backends/pdfform/src/`, `crates/bindings/wasm/src/`

## TL;DR

A WASM path that paints a rasterized page directly into a
`CanvasRenderingContext2d`, alongside the byte-output verbs (`render` for
PDF/PNG/SVG) without replacing them. It is multi-backend: any backend whose
session can rasterize a page (Typst, and pdfform under its preview seam) paints
through one generic painter. Each `paint` writes a **complete** raster — every
piece of page content already visible — so the consumer never composites.

## Why

For live previews of long documents, the byte-output formats are
sub-optimal:

- **Iframed SVG**: each iframe is its own browser document. N pages → N
  documents; teardown and memory cost grow linearly.
- **Inline SVG**: scales with content complexity (every glyph is a DOM
  node); long, dense documents produce huge DOM trees.
- **PNG**: pays zlib encode + decode on every render, and you typically
  hold N decoded bitmaps.

A canvas painter skips the encode/decode round-trip entirely — pixels go
straight from the rasterizer into the canvas backing store. No PNG
compression, no SVG XML parse, no second layout pass in the browser. For long
documents the consumer can keep memory bounded to the visible viewport — only
paint pages near it, repaint as the user scrolls.

## The seam

`core` carries a backend-neutral canvas seam on `SessionHandle`; the WASM
painter dispatches through it generically, never downcasting to a backend
session type:

```rust
// quillmark-core
pub trait SessionHandle: Any + Send + Sync {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;
    fn as_any(&self) -> &dyn Any;

    // Canvas seam — default None = "no painter".
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> { None }
    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> { None }
}
```

A backend opts into canvas simply by overriding the two seam methods; there is
no separate capability flag. Capability is **derived** from the seam:
`RenderSession::supports_canvas()` is true exactly when the session exposes
`page_size_pt` for its pages, so `paint`/`pageSize` succeed precisely when the
session reports canvas — the gate cannot drift from the implementation because
there is nothing to keep in sync. For a pre-session estimate (a GUI deciding
whether to mount a canvas UI before opening a session), the engine's
`supportsCanvas(quill)` derives a hint from the backend's output formats
(`quillmark_core::formats_support_canvas`: a backend that emits a visual-page
format, PNG or SVG, can paint); the session-level answer is authoritative.

### Complete-raster contract

`render_rgba` returning `Some` guarantees a **complete** page raster: all
content is visible in the returned pixels and the caller paints them with no
compositing of its own. Backends satisfy it differently:

- **Typst** rasterizes its laid-out page natively (`typst-render` →
  `tiny_skia::Pixmap` → unpremultiply → RGBA8).
- **pdfform** pre-flattens the bound field values into the page content
  streams at session-open, then rasterizes that flat PDF via hayro — so field
  values appear in the raster on their own, with no regions-compositing by the
  caller.

The `regions` sidecar (on `RenderResult`, see [SCHEMAS.md](SCHEMAS.md) and the
region type in `crates/core/src/region.rs`) carries per-field geometry and
bound value for interactive **overlays** drawn on top of the raster. It is
never needed to complete the picture.

## TypeScript surface

Capability and rendering live on the **engine** (it holds the resolved
backend); `Quill` is declarative data. Canvas is in the backend builds only.

```ts
class Engine {
  supportsCanvas(quill: Quill): Promise<boolean>;     // probe before mounting canvas UI / open()
  supportedFormats(quill: Quill): Promise<OutputFormat[]>;
  open(quill: Quill, doc: Document): Promise<RenderSession>;
  render(quill: Quill, doc: Document, opts?: RenderOptions): Promise<RenderResult>;
}

class RenderSession {
  readonly pageCount: number;
  readonly backendId: string;
  readonly supportsCanvas: boolean;
  readonly warnings: Diagnostic[];

  render(opts?: RenderOptions): RenderResult;
  pageSize(page: number): PageSize;     // { widthPt, heightPt } in pt; report-only
  paint(
    ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
    page: number,
    opts?: PaintOptions,
  ): PaintResult;
}

interface PaintOptions {
  layoutScale?: number;   // layout px per pt; layout decision; default 1
  densityScale?: number;  // backing-store density multiplier; default 1
}

interface PaintResult {
  layoutWidth: number;    // canvas.style.width target; independent of densityScale
  layoutHeight: number;
  pixelWidth: number;     // canvas.width the painter wrote (clamped at 16384)
  pixelHeight: number;
}
```

### DPR / clamp math

The painter owns `canvas.width` / `canvas.height` and sizes the backing store
on every call; consumers own `canvas.style.*` and read `layoutWidth` /
`layoutHeight` from the result. The effective rasterization scale is:

```
renderScale = layoutScale × densityScale
```

Fold `window.devicePixelRatio`, in-app zoom, and `visualViewport.scale` into
`densityScale`. If the largest backing dimension would exceed
**`MAX_BACKING_DIMENSION` (16384 px per side)** — the floor that works across
browsers (Chrome/Firefox ~32k, Safari 16k, lower on memory-constrained mobile)
— the painter clamps `densityScale` proportionally and reports the actual
backing dimensions. Detect a clamp via:

```
pixelWidth < round(layoutWidth × densityScale)
```

Each `paint` resets the backing store (writing `canvas.width` clears it), so
paint is always a full repaint — consumers never call `clearRect`.

### Regions overlay transform

A consumer drawing overlays from `regions` must flip the Y axis: region
`rect = [x0, y0, x1, y1]` is in PDF points with a **bottom-left** origin, a
canvas is **top-left** in device pixels. For a page `pageHeightPt` tall (from
`pageSize`) painted at `renderScale`, the box's top-left canvas corner is the
PDF rect's *upper* edge (`y1 = rect[3]`), not its lower edge (`y0 = rect[1]`):

```
x_canvas_left = rect[0] × renderScale
y_canvas_top  = (pageHeightPt − rect[3]) × renderScale
width_canvas  = (rect[2] − rect[0]) × renderScale
height_canvas = (rect[3] − rect[1]) × renderScale
```

## Feature / build mapping

Canvas ships per-backend, compile-time aligned so the capability flag and the
painter cannot disagree:

| Build                                     | Backend  | Canvas | Notes                                                    |
| ----------------------------------------- | -------- | ------ | -------------------------------------------------------- |
| `pkg/core/` (no features)                 | —        | no     | `Document` + `Quill` only; no engine, no Typst           |
| `pkg/backends/typst/` (`typst`)           | typst    | yes    | native page raster                                       |
| `pkg/backends/pdfform/` (`pdfform-preview`) | pdfform | yes    | pre-flatten + hayro raster; ships hayro/vello_cpu (wasm) |
| (`pdfform`, tiny)                         | pdfform  | no     | form-fill → PDF only; no `web-sys`, no painter           |

`pdfform-preview` is a strict superset of `pdfform`: it adds
`quillmark-pdfform/preview` (the hayro raster + SVG seam) and the `web-sys`
canvas surface. The pdfform backend reports `supports_canvas() == true` only
under `preview`, which `pdfform-preview` enables — so the tiny `pdfform` build
is honestly canvas-free. `build-wasm.sh` builds all three artifacts (core,
typst, pdfform) sequentially; `runtime/runtime.js` maps each backend id to its
build with a `{ formats, canvas }` manifest, drift-guarded by
`runtime.test.js`.

## Non-goals

- Native (CLI / Python) exposure. Capability is WASM-only.
- Text selection, find-in-page, accessibility. Canvas has none of these by
  design — if you need them, keep an SVG/PDF export path alongside.
- Click-to-jump or cursor-to-region mapping. Investigated as a Typst spike
  (jump_from_click / jump_from_cursor + an OriginMap) but deferred — not
  needed for the preview itself.

## Decisions and rationale

- **One generic painter over the `SessionHandle` seam, not a per-backend
  downcast.** `paint` calls `page_size_pt` / `render_rgba` on the opaque
  session; every canvas backend implements the same two methods. Adding a
  canvas backend is overriding the two seam methods (`page_size_pt` /
  `render_rgba`) — capability is then derived from the seam, with no separate
  flag to flip and no binding to touch.
- **Complete raster, never compose-from-regions.** Both backends hand back a
  finished page (Typst natively, pdfform by pre-flattening values into content
  streams before rasterizing). Regions are an overlay sidecar, not a
  compositing input — the painter stays a dumb blit.
- **Method on `RenderSession`, not a sub-handle.** A `Preview` sub-handle would
  be justified only if paint shipped with `click()` / `locate_cursor()` (shared
  state). With paint alone the sub-handle is ceremony.
- **Not an `OutputFormat`.** Canvas is a side-effecting paint into a JS object,
  not a serializable byte stream. Forcing it into the enum would leak
  `wasm_bindgen` into `core` or make `Artifact` dishonest.
- **Coalesce at the session, not the format.** One compile feeds bytes
  (`render`), pixels (`paint`), and metadata (`pageSize`, `warnings`).
- **`layoutScale` and `densityScale` separated, both optional.** A single
  scalar conflated layout (how big on screen) with sharpness (how many backing
  pixels). The split mirrors how editor consumers think: `layoutScale` is a
  layout decision, `densityScale` a sharpness decision folding `devicePixelRatio`
  + zoom + `visualViewport.scale`. Both default to 1 because the painter cannot
  know the consumer's DPR (SSR, tests, off-screen).
- **Painter owns `canvas.width`/`height`; consumer owns `canvas.style.*`.**
  Folding backing-store math into the painter eliminates a class of "blurry on
  retina" bugs and lets the 16384-px clamp live in one place.
- **Unpremultiplied RGBA on the wire.** Rasterizers produce premultiplied
  alpha; `ImageData` expects non-premultiplied. The backend unpremultiplies
  before handing back the buffer. One allocation per repaint; fine for edit
  cadence.
- **`warnings` accessor on `RenderSession`.** Session-level diagnostics attached
  at `Backend::open` are otherwise invisible to canvas consumers (only surfaced
  via `render()`'s `RenderResult`).

## Lifecycle and consumer flow

```js
import { Engine } from '@quillmark/wasm';      // single root export
const engine = new Engine();

if (!(await engine.supportsCanvas(quill))) return;   // non-canvas backends have no painter
const session = await engine.open(quill, doc);       // compiles once, caches the snapshot
const densityScale = (window.devicePixelRatio || 1) * userZoom;  // userZoom is a UI control

const result = session.paint(canvas.getContext('2d'), page, {
  layoutScale: 1,                             // layout px per pt
  densityScale,                               // includes devicePixelRatio + zoom
});

canvas.style.width  = `${result.layoutWidth}px`;   // CSS box, layout px
canvas.style.height = `${result.layoutHeight}px`;
```
