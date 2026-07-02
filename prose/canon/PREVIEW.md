# Live Preview (WASM)

> **Implementation**: `crates/core/src/session.rs`, `crates/backends/typst/src/`, `crates/backends/pdfform/src/`, `crates/bindings/wasm/src/`

## TL;DR

The preview surface is two verbs: `render(quill, doc, opts)` — stateless
one-shot bytes for CLI / server / export — and `open(quill, doc)` →
**`LiveSession`**, a persistent, incremental compiler that owns preview. Reads
(`render`, `paint`, `pageSize`, `regions`) serve the session's current
compile; `apply(doc)` recompiles in place and returns a `ChangeSet` naming the
dirty pages. `paint` writes a rasterized page directly into a
`CanvasRenderingContext2d`; each paint is a **complete** raster — every piece
of page content already visible — so the consumer never composites. It is
multi-backend: any backend whose session can rasterize a page (Typst, pdfform)
paints through one generic painter.

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
straight from the rasterizer into the canvas backing store. For long documents
the consumer keeps memory bounded to the visible viewport — paint only pages
near it, repaint as the user scrolls.

The edit loop is the second half of the argument. Re-opening a session per
keystroke rebuilds the entire compilation world (fonts, packages, assets) and
repaints every visible page whether or not it changed. A `LiveSession` keeps
the world alive across edits and reports what an edit visibly changed, so the
per-keystroke cost is *incremental recompile + repaint of `dirty ∩ visible`*.

## The seam

`core` carries a backend-neutral session seam on `SessionHandle`; the WASM
painter dispatches through it generically, never downcasting to a backend
session type:

```rust
// quillmark-core
pub trait SessionHandle: Any + Send + Sync {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;
    fn as_any(&self) -> &dyn Any;

    // Edit seam — default Err = "apply unsupported".
    fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> { ... }

    // Canvas seam — default None = "no painter".
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> { None }
    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> { None }
}
```

A backend opts into canvas by overriding the two seam methods; there is
no separate capability flag. Capability is **derived** from the seam:
`LiveSession::supports_canvas()` is true exactly when the session exposes
`page_size_pt` for its pages, so `paint`/`pageSize` succeed precisely when the
session reports canvas — the gate cannot drift from the implementation because
there is nothing to keep in sync. For a pre-session estimate (a GUI deciding
whether to mount a canvas UI before opening a session), the engine's
`supportsCanvas(quill)` derives a hint from the backend's output formats
(`quillmark_core::formats_support_canvas`: a backend that emits a visual-page
format, PNG or SVG, can paint); the session-level answer is authoritative.

## Live edits — `apply` and `ChangeSet`

`apply(json_data)` recompiles the session against new document data.
**Transactional**: on `Err` the previous compile stays live — every read keeps
serving the last-good document, and the session recovers on the next
successful apply. On `Ok` reads serve the new compile and the returned
`ChangeSet { page_count, dirty_pages }` names the pages whose rendered content
changed (including added pages; removed pages are implied by `page_count`). A
preview repaints `dirty ∩ visible` and nothing else — that repaint bound, not
compile speed, is the throughput lever.

Per-backend, apply is an implementation choice, not a flag:

- **Typst** recompiles incrementally. The session persists its `QuillWorld`
  (fonts, packages, assets parsed once at `open`); an edit swaps the helper
  package's `lib.typ` via `Source::replace` (incremental reparse) and
  recompiles — `comemo` reuses every memoized eval/layout result the edit did
  not reach. Dirty pages come from per-page fingerprints of *visible* frame
  content; introspection `Tag` items are excluded because a page-spanning
  element's tag carries a hash of content on other pages and would dirty
  page 0 on an end-of-document edit.
- **pdfform** recompiles fully — its compile is a re-resolve + re-flatten,
  cheap by construction. Dirty pages are those carrying a field whose resolved
  spec changed.

**Cache eviction.** Typst's `comemo` cache is process-global and grows
unboundedly without eviction; an editing loop compiles once per keystroke. The
Typst backend evicts entries older than 10 compiles after *every* compile
(`compile.rs`, matching typst-cli's watch policy) — the one-shot path leaks
otherwise too, so eviction is unconditional, not a session feature.

**One session type.** Immutability is an invariant, not a type: reads between
edits see a stable document because apply swaps the compile only on success,
and the preview consumer (ours — the session surface is `@experimental` and
preview is WASM-only by non-goal) executes serially. There is no separate
frozen snapshot type and no change-generation counter — with a single owned
consumer there is no cross-edit reader to protect. If a long-lived read-only
viewer ever needs to shed the retained world, a `freeze()` that drops it and
keeps the pageable document is a *mode* to add, not a second type.

### Complete-raster contract

`render_rgba` returning `Some` guarantees a **complete** page raster: all
content is visible in the returned pixels and the caller paints them with no
compositing of its own. Backends satisfy it differently:

- **Typst** rasterizes its laid-out page natively (`typst-render` →
  `tiny_skia::Pixmap` → unpremultiply → RGBA8).
- **pdfform** pre-flattens the bound field values into the page content
  streams at session-open (and again at each `apply`), then rasterizes that
  flat PDF via hayro — so field values appear in the raster on their own, with
  no regions-compositing by the caller.

Field geometry is a **session-level query**, `LiveSession::regions()` (see
[SCHEMAS.md](SCHEMAS.md) and the region type in `crates/core/src/region.rs`) —
not a field on `RenderResult`. Only the interactive-preview path wants it, and
that path holds a session; a one-shot byte render (PDF/PNG/SVG) never does, so
it is read off the current compile with no render — re-read it after each
committed `apply`. Each region carries per-field geometry keyed on the **quill
schema field path** — the address the editor uses — for **overlays** and
**cross-navigation** (click a rendered field → focus it in the editor, or
highlight the page rectangle for the focused field).
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
  open(quill: Quill, doc: Document): Promise<LiveSession>;
  render(quill: Quill, doc: Document, opts?: RenderOptions): Promise<RenderResult>;
}

class LiveSession {
  readonly pageCount: number;
  readonly backendId: string;
  readonly supportsCanvas: boolean;
  readonly warnings: Diagnostic[];

  apply(doc: Document): ChangeSet;      // in-place recompile; transactional
  render(opts?: RenderOptions): RenderResult;
  regions(): FieldRegion[];             // schema-field geometry; session query, no render
  pageSize(page: number): PageSize;     // { widthPt, heightPt } in pt; report-only
  paint(
    ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
    page: number,
    opts?: PaintOptions,
  ): PaintResult;
}

interface ChangeSet {
  pageCount: number;      // page count after the edit
  dirtyPages: number[];   // repaint dirty ∩ visible; removed pages implied by pageCount
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
- Built-in click→region hit-testing in the painter. The painter is a dumb
  blit; it maps no clicks itself. A consumer builds field cross-navigation on
  top, hit-testing a click against the `regions` sidecar (keyed on the schema
  field path) — see [SCHEMAS.md](SCHEMAS.md).

## Decisions and rationale

- **Two verbs, one session type.** `render` is the stateless one-shot;
  `open` → `LiveSession` owns preview. The frozen single-compile snapshot is
  not a separate type: its immutability survives as the swap-on-commit
  invariant of a transactional `apply`, and its "hold last-good while
  computing next" behavior falls out of the same invariant with no
  special-casing. Third-party preview controllers are out of scope, so no
  defensive snapshot type and no change-generation counter guards a consumer
  we do not ship.
- **One generic painter over the `SessionHandle` seam, not a per-backend
  downcast.** `paint` calls `page_size_pt` / `render_rgba` on the opaque
  session; every canvas backend implements the same two methods. Adding a
  canvas backend is overriding the two seam methods (`page_size_pt` /
  `render_rgba`) — capability is then derived from the seam, with no separate
  flag to flip and no binding to touch.
- **`apply` reports dirty pages, not new handles.** Page identity is the index;
  a `ChangeSet` is data. Nothing borrowed from a previous compile outlives an
  edit because reads resolve against the current compile at call time.
- **Complete raster, never compose-from-regions.** Both backends hand back a
  finished page (Typst natively, pdfform by pre-flattening values into content
  streams before rasterizing). Regions are an overlay sidecar, not a
  compositing input — the painter stays a dumb blit.
- **Method on `LiveSession`, not a sub-handle.** A `Preview` sub-handle would
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
- **`warnings` accessor on `LiveSession`.** Session-level diagnostics attached
  at `Backend::open` are otherwise invisible to canvas consumers (only surfaced
  via `render()`'s `RenderResult`).
- **`regions()` on `LiveSession`, not `RenderResult`.** Field geometry is a
  property of the current compile, and only the interactive-preview path wants
  it — that path holds a session (it `paint()`s) and produces no byte artifact.
  Hanging regions off `RenderResult` forced an export-only consumer to receive
  geometry it never reads, and forced a paint-only consumer to run a throwaway
  byte render just to harvest the sidecar. A session method computes it from
  already-resolved placements with no rasterization, serving both the canvas and
  SVG-overlay previews from the one handle they already hold.

## Lifecycle and consumer flow

```js
import { Engine } from '@quillmark/wasm';      // single root export
const engine = new Engine();

if (!(await engine.supportsCanvas(quill))) return;   // non-canvas backends have no painter
const session = await engine.open(quill, doc);       // compiles once; the session persists its world
const densityScale = (window.devicePixelRatio || 1) * userZoom;  // userZoom is a UI control

const result = session.paint(canvas.getContext('2d'), page, {
  layoutScale: 1,                             // layout px per pt
  densityScale,                               // includes devicePixelRatio + zoom
});

canvas.style.width  = `${result.layoutWidth}px`;   // CSS box, layout px
canvas.style.height = `${result.layoutHeight}px`;

// Edit loop: apply, repaint dirty ∩ visible. On throw, the canvas still
// shows the last-good compile — keep it and surface the diagnostics.
function onEdit(editedDoc) {
  const { pageCount, dirtyPages } = session.apply(editedDoc);
  for (const p of dirtyPages) if (isVisible(p)) repaint(p);
}
```
