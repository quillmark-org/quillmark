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

A backend opts into canvas by overriding the two seam methods; there is
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

Field geometry is a **session-level query**, `RenderSession::regions()` (see
[SCHEMAS.md](SCHEMAS.md) and the region type in `crates/core/src/region.rs`) —
not a field on `RenderResult`. Only the interactive-preview path wants it, and
that path holds a session; a one-shot byte render (PDF/PNG/SVG) never does, so it
is read once off the compiled session with no render. Each region carries
per-field geometry keyed on the **quill schema field path** — the address the
editor uses — for **overlays** and **cross-navigation** (click a rendered field →
focus it in the editor, or highlight the page rectangle for the focused field).
Two producers: **content fields** (a markdown body) auto-tag from their content
at the Typst eval site and recover their true rendered extent from the laid-out
frames; **form-field widgets** carry the path explicitly (pdfform from the form
mapping, a Typst `form-field` from its `field:` argument) and surface a region
only when they bind one — a widget with no schema field is a backend artifact,
not a routable field. The session returns **one region per logical field**: a
field arising from both a content tag and a bound widget, or from several
page-fragments, is collapsed (the bound widget wins; a page-spanning body anchors
to its first page), so a consumer looks a field up and gets one rectangle.
Geometry only, never a value, and never needed to complete the picture.

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
  regions(): FieldRegion[];             // schema-field geometry; session query, no render
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

For an **HTML/CSS overlay** on a `width:100%` canvas, prefer percentages of the
page over device pixels — they track the displayed size across DPI and
pane-resize for free, with no `renderScale` to thread; only the Y axis flips:

```
left%   = rect[0] / pageWidthPt  × 100
top%    = (pageHeightPt − rect[3]) / pageHeightPt × 100
width%  = (rect[2] − rect[0]) / pageWidthPt  × 100
height% = (rect[3] − rect[1]) / pageHeightPt × 100
```

The device-pixel form above is still the right one for painting an overlay
*into* a raster.

### Region geometry (`RegionMap`)

The transform above has exactly one right answer, and every interactive consumer
needs it, so the runtime layer ships it as a pure value object rather than
leaving each consumer to re-derive the Y-flip. `RegionMap` is a **per-page**
projection of `regions()` into draw-ready overlay boxes and a click hit-test —
no WASM, no DOM, no session reference, so a `render()`-only consumer never pulls
it in and it stays framework-neutral (it returns numbers; the consumer owns the
DOM):

```ts
class RegionMap {
  static from(regions: FieldRegion[], pageSize: PageSize, page: number): RegionMap;
  readonly page: number;
  readonly pageSize: PageSize;
  readonly fields: string[];                       // field paths on this page
  region(field: string): FieldRegion | undefined;  // raw region
  at(xPercent: number, yPercent: number): FieldRegion | undefined;  // hit-test, page %
  overlayPercent(field: string): OverlayBox | undefined;            // CSS-overlay box
  overlayDevice(field: string, renderScale: number): OverlayBox | undefined;  // raster box
  overlaysPercent(): FieldOverlay[];               // every field at once
  overlaysDevice(renderScale: number): FieldOverlay[];
}

interface OverlayBox { left: number; top: number; width: number; height: number; }
interface FieldOverlay { field: string; box: OverlayBox; }
```

`overlayPercent` emits the percent-of-page form (for a CSS overlay on a
`width:100%` canvas); `overlayDevice` emits the device-pixel form at
`renderScale` (= `layoutScale × densityScale`, for painting into a raster). Both
put the origin at the page's top-left with the Y axis already flipped, so a box
drops straight into `position:absolute` or `fillRect`. `at` takes the same
page-percent coordinates `overlayPercent` emits — the natural unit of a click on
a `width:100%` canvas — and returns the **smallest** region containing the
point, so the most specific field wins when boxes nest. Build one map per page;
it is a filter plus arithmetic.

## Feature / build mapping

Canvas ships per-backend, compile-time aligned so the capability flag and the
painter cannot disagree:

| Build                                     | Backend  | Canvas | Notes                                                    |
| ----------------------------------------- | -------- | ------ | -------------------------------------------------------- |
| `pkg/core/` (no features)                 | —        | no     | `Document` + `Quill` only; no engine, no Typst           |
| `pkg/backends/typst/` (`typst`)           | typst    | yes    | native page raster                                       |
| `pkg/backends/pdfform/` (`pdfform-preview`) | pdfform | yes    | pre-flatten + hayro raster/SVG/PNG; adds the `web-sys` painter |
| (`pdfform`, no `web-sys`)                 | pdfform  | no     | renders PDF + SVG + PNG, but no canvas painter           |

The pdfform backend always links its hayro raster seam, so it renders PDF, SVG,
and PNG without any preview feature (`supports_canvas() == true`). The wasm `pdfform-preview`
feature is a strict superset of `pdfform` that only adds the `web-sys` canvas
*painter*, so the in-browser `paint()` surface ships; a `pdfform` build without
`web-sys` still renders SVG/PNG but carries no painter. `build-wasm.sh` builds
the three artifacts (core, typst, pdfform — the last with `pdfform-preview`)
sequentially; `runtime/runtime.js` maps each backend id to its build with a
`{ formats, canvas }` manifest, drift-guarded by `runtime.test.js`.

## Non-goals

- Native (CLI / Python) exposure. Capability is WASM-only.
- Text selection, find-in-page, accessibility. Canvas has none of these by
  design — if you need them, keep an SVG/PDF export path alongside.
- Click→region hit-testing *in the painter*. The painter is a dumb blit; it maps
  no clicks itself. Hit-testing is a pure helper over the `regions` sidecar
  (`RegionMap.at`, keyed on the schema field path) that a consumer calls itself —
  see the `RegionMap` section above and [SCHEMAS.md](SCHEMAS.md).
- A stateful preview *controller* — an object owning the canvas, viewport,
  repaint scheduling, page virtualization, and click→field dispatch. Deferred,
  not declined (see Decisions): its state shape can't be settled without a real
  consumer. `paint`, `regions`, and `RegionMap` are the stateless primitives such
  a controller is assembled from.

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
- **`regions()` on `RenderSession`, not `RenderResult`.** Field geometry is a
  property of the compiled snapshot, and only the interactive-preview path wants
  it — that path holds a session (it `paint()`s) and produces no byte artifact.
  Hanging regions off `RenderResult` forced an export-only consumer to receive
  geometry it never reads, and forced a paint-only consumer to run a throwaway
  byte render just to harvest the sidecar. A session method computes it from
  already-resolved placements with no rasterization, serving both the canvas and
  SVG-overlay previews from the one handle they already hold.
- **Region geometry is a pure value object (`RegionMap`), not a session method or
  a painter feature.** The region transform (Y-flip, bottom-left→top-left,
  pt↔device-px) has one right answer and every interactive consumer needs it, so
  leaving it as prose invites each one to re-derive it and get the flip wrong.
  `RegionMap` encodes it once as data — no WASM, no DOM, no session reference — so
  it stays tree-shakeable (a `render()`-only consumer never loads it) and
  framework-neutral (it returns numbers; the consumer owns the DOM). Hanging it
  off `RenderSession` would drag geometry into the WASM-backed handle for no gain;
  baking it into the painter would break the dumb-blit contract.
- **The stateful preview controller is deferred, not declined.** An object owning
  the canvas, the viewport, repaint scheduling, and click→field dispatch is the
  `Preview` sub-handle this doc otherwise rejects as ceremony — justified only
  once paint ships with shared interactive state. That state's shape (how a
  viewport drives which pages repaint, how a document edit invalidates the
  compiled snapshot) can't be settled without a real consumer, and the session
  surface is `@experimental` precisely to admit that. Ship the stateless
  primitives (`paint`, `regions`, `RegionMap`) now; assemble the controller from
  them once the editor live-preview path makes its shape concrete.

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
