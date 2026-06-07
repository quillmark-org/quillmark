# Canvas Preview (WASM, Typst)

> **Implementation**: `crates/backends/typst/src/`, `crates/bindings/wasm/src/`

## TL;DR

A Typst-only, WASM-only path that paints rasterized pages directly into a
`CanvasRenderingContext2d`. Sits alongside the existing byte-output verbs
(`render` for PDF/PNG/SVG); does not replace them. Both paths share the
cached `PagedDocument` produced by `Backend::open`, so one compile feeds
both.

## Why

For live previews of long documents, the byte-output formats are
sub-optimal:

- **Iframed SVG**: each iframe is its own browser document. N pages → N
  documents; teardown and memory cost grow linearly.
- **Inline SVG**: scales with content complexity (every glyph is a DOM
  node); long, dense documents produce huge DOM trees.
- **PNG**: pays zlib encode + decode on every render, and you typically
  hold N decoded bitmaps.

A canvas painter skips the encode/decode round-trip entirely:

```
typst-render → tiny_skia::Pixmap → unpremultiply → ImageData → putImageData
```

Pixels go straight from the rasterizer into the canvas backing store. No
PNG compression, no SVG XML parse, no second layout pass in the browser.

For long documents, the consumer can keep memory bounded to the visible
viewport — only paint pages near the viewport, repaint as the user
scrolls.

## Non-goals

- Native (CLI / Python) exposure. Capability is WASM-only.
- Text selection, find-in-page, accessibility. Canvas has none of these by
  design — if you need them, keep an SVG/PDF export path alongside.
- Click-to-jump or cursor-to-region mapping. Investigated as a Typst spike
  (jump_from_click / jump_from_cursor + an OriginMap) but deferred — not
  needed for the preview itself.

## API

### Rust

```rust
// quillmark-core
pub trait SessionHandle: Any + Send + Sync {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;
    fn as_any(&self) -> &dyn Any;
}

impl RenderSession {
    pub fn page_count(&self) -> usize;
    pub fn warnings(&self) -> &[Diagnostic];
    pub fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    #[doc(hidden)]
    pub fn handle(&self) -> &dyn SessionHandle;
}
```

```rust
// quillmark-typst
pub struct TypstSession { /* PagedDocument + page_count */ }

impl TypstSession {
    pub fn page_size_pt(&self, page: usize) -> Option<(f32, f32)>;
    pub fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)>;
}

pub fn typst_session_of(s: &RenderSession) -> Option<&TypstSession>;
```

### TypeScript (WASM)

Capability and rendering live on the **engine** (it holds the resolved
backend); `Quill` is engine-free data. Canvas preview is in the **render**
build only.

```ts
class Quill {
  static fromTree(tree: Map<string, Uint8Array>): Quill;
  readonly backendId: string;          // declared intent; not a resolved capability
}

class Quillmark {
  supportsCanvas(quill: Quill): boolean;   // probe before mounting canvas UI / open()
  supportedFormats(quill: Quill): OutputFormat[];
  open(quill: Quill, doc: Document): RenderSession;
  render(quill: Quill, doc: Document, opts?: RenderOptions): RenderResult;
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
  layoutScale?: number;   // layout px per Typst pt; layout decision; default 1
  densityScale?: number;  // backing-store density multiplier; default 1
}

interface PaintResult {
  layoutWidth: number;    // canvas.style.width target; independent of densityScale
  layoutHeight: number;
  pixelWidth: number;     // canvas.width the painter wrote (clamped at 16384)
  pixelHeight: number;
}
```

The painter owns `canvas.width` / `canvas.height` — it sizes the backing
store on every call. Consumers own `canvas.style.*` (or layout) and read
`layoutWidth` / `layoutHeight` from the result. `layoutScale * densityScale`
is the effective rasterization scale; the painter clamps `densityScale`
if the largest backing dimension would exceed 16384 px.

## Architecture

The canvas path is a typed side channel — `core` stays output-format-only,
the typst crate owns the typed surface, the WASM binding wires it to
`web-sys`.

```
core::RenderSession            ← Box<dyn SessionHandle>
  └─ TypstSession              ← typst-only; holds PagedDocument
       └─ typst-render::render ← PagedDocument + scale → tiny_skia::Pixmap
            └─ Pixmap.demultiply() → RGBA8 buffer
                 └─ ImageData → ctx.putImageData
```

The seam in `core` is minimal: `SessionHandle: Any + as_any(&self)` plus a
`#[doc(hidden)]` `RenderSession::handle()` accessor. The typst crate owns
the downcast in one place (`typst_session_of`). Native bindings never
link the WASM side and never call the typed accessor; their behavior is
byte-identical.

## Lifecycle and consumer flow

```js
import { Quillmark } from '@quillmark/wasm';   // root = render build; canvas is render-only
const engine = new Quillmark();

if (!engine.supportsCanvas(quill)) return;    // non-typst backends have no painter
const session = engine.open(quill, doc);      // compiles once, caches PagedDocument
const densityScale = (window.devicePixelRatio || 1) * userZoom;  // userZoom is a UI control

const result = session.paint(canvas.getContext('2d'), page, {
  layoutScale: 1,                             // layout px per Typst pt
  densityScale,                               // includes devicePixelRatio + zoom
});

canvas.style.width  = `${result.layoutWidth}px`;   // CSS box, layout px
canvas.style.height = `${result.layoutHeight}px`;
```

Each `paint` call resets the backing store (writing `canvas.width`
clears it), so paint is always a full repaint. Consumers don't call
`clearRect`. If `layoutScale * densityScale` would push either dimension
past 16384 px, the painter clamps `densityScale` proportionally and
reports the actual backing dimensions in the result.

## Decisions and rationale

- **Method on `RenderSession`, not a sub-handle.** A `Preview` sub-handle
  returned by `session.preview()` would be justified only if paint shipped
  with `click()` and `locate_cursor()` (they share state). With paint alone,
  the sub-handle is ceremony — keep the verb on `RenderSession`.
- **Not an `OutputFormat`.** Canvas is a side-effecting paint into a JS
  object, not a serializable byte stream. Forcing it into the enum
  either leaks `wasm_bindgen` into `core` or makes `Artifact` dishonest.
- **Coalesce at the session, not at the format.** One compile feeds
  bytes (`render`), pixels (`paint`), and metadata (`pageSize`,
  `warnings`).
- **`Any` downcast over a generic capability registry.** Canvas is
  Typst-only and WASM-only; pushing it through a generic core trait would
  force every backend to implement or stub it and would drag `web-sys`
  toward `core`. The downcast is the standard escape hatch.
- **`layoutScale` and `densityScale` separated, both optional.** A
  single scalar conflated layout (how big on screen) with sharpness
  (how many backing pixels). The split mirrors how editor consumers
  think about it — `layoutScale` is a layout decision, `densityScale`
  is a sharpness decision they fold `devicePixelRatio` + zoom +
  `visualViewport.scale` into. Both default to 1 because the painter
  alone cannot know the consumer's DPR (e.g. SSR contexts, tests,
  off-screen previews); the cost of the silent default is one missed
  `densityScale` ⇒ blurry retina, the benefit is a usable
  `paint(ctx, page)` for the simple case.
- **Painter owns `canvas.width` / `canvas.height`; consumer owns
  `canvas.style.*`.** The alternative — pushing backing-store math onto every
  consumer ("size your canvas like X before calling paint") — makes
  `devicePixelRatio` and the rounding rule callable-side state, which
  means every consumer has to get them right. Folding the math into the
  painter eliminates a class of "blurry on retina" bugs and lets the
  painter clamp at the 16384-px browser limit centrally.
- **Hard 16384-px backing-store clamp.** Real browser limits vary
  (Chrome/Firefox ~32k, Safari 16k, lower on memory-constrained mobile);
  16384 is the floor that works everywhere. `PaintResult` reports the
  actual backing dimensions, so a consumer that cares can detect the
  clamp and surface "max zoom reached" UI.
- **Unpremultiplied RGBA on the wire.** `tiny_skia` produces premultiplied
  alpha; `ImageData` expects non-premultiplied. We unpremultiply pixel-by-
  pixel before constructing `ImageData`. One allocation per repaint;
  fine for typical edit cadence.
- **`warnings` accessor on `RenderSession`.** The session-level diagnostic
  attached at `Backend::open` time is otherwise invisible to canvas
  consumers (it was only surfaced via `render()`'s `RenderResult`).

## Crate layout

```
crates/
├── core/src/session.rs              extended  — Any + handle()
├── backends/typst/src/lib.rs        extended  — TypstSession is pub;
│                                                page_size_pt, render_rgba;
│                                                typst_session_of accessor
└── bindings/wasm/
    ├── Cargo.toml                   extended  — web-sys features
    │                                            (CanvasRenderingContext2d,
    │                                             HtmlCanvasElement,
    │                                             ImageData,
    │                                             OffscreenCanvas,
    │                                             OffscreenCanvasRenderingContext2d)
    └── src/engine.rs                extended  — paint, pageSize,
                                                  backendId, supportsCanvas,
                                                  warnings; CanvasCtx enum
                                                  dispatches OnScreen vs
                                                  OffScreen contexts (calls
                                                  typst_session_of directly;
                                                  no separate adapter file)
```

## Future work (not in V1)

- **Direct `CanvasRenderingContext2d` adapter.** V1 allocates an RGBA
  `Vec<u8>` per repaint. A direct path that hands tiny_skia's pixmap to
  the canvas (or a typed-array view backed by linear memory) would
  remove the allocation. Optimize only if profiling demands.
- **Click → editor and cursor → preview mapping.** Out of scope for the
  preview itself. If/when added, would slot in via the same
  `TypstSession` accessor by exposing `IdeWorld` + an `OriginMap` from
  MD→Typst conversion.

## Feature gate (implemented)

The whole canvas/render surface — the `Quillmark` engine, `RenderSession`,
`paint` / `pageSize`, `CanvasCtx`, the `web-sys` dependency, and
`quillmark_typst` — is gated behind the wasm crate's `render` feature
(default). The **core** build (`--no-default-features`,
`@quillmark/wasm/core`) drops all of it along with Typst, leaving
`Document` + `Quill` for load / validate / schema / seed / blueprint. This
realizes the once-deferred "opt out of the canvas dependency" — except the
opt-out is the whole render half, not just `web-sys`, and the win is Typst
(~8 MB), not the canvas glue. See [the split proposal](../proposals/wasm-bindings-split.md).
