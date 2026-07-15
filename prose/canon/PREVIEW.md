# Live Preview (WASM)

> **Implementation**: `crates/core/src/`, `crates/backends/typst/src/`, `crates/backends/pdfform/src/`, `crates/bindings/wasm/src/`

## TL;DR

The preview surface is two verbs: `render(quill, doc, opts)` — stateless
one-shot bytes for CLI / server / export — and `open(quill, doc)` →
**`LiveSession`**, a persistent, incremental compiler that owns preview. Reads
(`render`, `paint`, `pageSize`, `regions`, `fieldAt`, `positionAt`, `locate`)
serve the session's current compile; `apply(doc)` recompiles in place and
returns a `ChangeSet` naming the dirty pages. `paint` writes a rasterized page directly into a
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
pub trait SessionHandle: Send + Sync + 'static {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;

    // Edit seam — default Err = "apply unsupported".
    fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> { ... }

    // Canvas seam — default None = "no painter".
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> { None }
    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> { None }

    // Warnings seam — the current compile's non-fatal diagnostics; default empty.
    fn warnings(&self) -> &[Diagnostic] { &[] }
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
serving the last-good document and its `warnings`, and the session recovers on
the next successful apply. On `Ok` reads serve the new compile — `warnings`
included — and the returned `ChangeSet { page_count, dirty_pages }` names the
pages whose rendered content changed (including added pages; removed pages are
implied by `page_count`). A preview repaints `dirty ∩ visible` and nothing
else — that repaint bound, not compile speed, is the throughput lever.

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
and the preview consumer (ours — preview is WASM-only by non-goal) executes
serially. There is no separate
frozen snapshot type and no change-generation counter — with a single owned
consumer there is no cross-edit reader to protect. If a long-lived read-only
viewer ever needs to shed the retained world, a `freeze()` that drops it and
keeps the pageable document is a *mode* to add, not a second type.

`apply` is the only edit verb — a whole-document recompile. Anchoring a caret
or selection across edits is the **editor's** job: its own transaction mapping
(a ProseMirror / CodeMirror `StepMap`) carries positions through local edits, so
the session holds no change log, no revision stamp, and no per-field delta path
(#886 removed them; `FieldRegion` / `CorpusHit` carry no `revision`). Geometry
(`regions`, `positionAt`, `locate`) is read against the current compile and
re-read after each committed `apply`. `positionAt` (point → corpus position) and
`locate` (corpus position → caret rect) are exact inverses over that compile —
that pair *is* the bidirectional preview↔editor cursor bridge, and it needs no
forward-mapping because the editor owns the live position it feeds in.

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

### Painter owns the canvas

`paint` writes the whole backing store with `put_image_data`, which bypasses
the 2D context transform, `globalAlpha`, and clip. The painter therefore owns
the entire canvas: **give each visible page its own `` element.** You
cannot paint two pages into one canvas, paint into a sub-rect, or push a page
through a context transform — the raster is complete precisely so you never
need to composite, and the write ignores context state so you could not if you
tried.

Because every `paint` re-rasterizes from scratch (no per-page raster cache on
the session — a deliberate omission, § Decisions), keep a page's canvas alive
while it stays near the viewport rather than pooling one canvas across pages: an
idle canvas retains its pixels for free, whereas reusing a canvas on scroll
re-runs a full render. This keeps memory bounded to *visible + margin* without
paying re-rasterization on every scroll reversal.

Field geometry is primarily a **session-level query**, `LiveSession::regions()`
(see the region type in `crates/core/src/region.rs`): the interactive-preview
path holds a session and reads geometry off the current compile with no render
— re-read it after each committed `apply`. A one-shot byte render carries the
same sidecar only on request (`RenderOptions::regions` → `RenderResult::regions`),
for consumers without a live session — static overlays over an exported SVG,
PDF post-processing, CI coverage probes. The sidecar always describes the
whole document: page indices are document-space even under a `pages` subset
render. Each region carries per-field geometry keyed on the **quill schema
field path** — the address the editor uses — plus, for content ink, the
**corpus span** it covers (§ Segments and the striped union). Navigation is
four queries, two coarse and two fine:

- `regions()` answers *field → rectangles* (scroll to / highlight a field),
  one box per content **segment** it draws. `fieldBoxes(field)` derives the
  whole-field highlight from it — one union rect per page over the field's
  `span`-bearing segments — so consumers do not reimplement the union.
- `fieldAt(page, x, y)` answers *point → field* (click → focus in the editor),
  hit-testing the compiled document directly so **every** placement resolves,
  not just the ones `regions()` surfaces.
- `positionAt(page, x, y)` answers *point → corpus position* — the field
  *and* a USV offset into its `RichText`, cluster-exact, for placing a caret
  or mapping a selection into the content model.
- `locate(field, pos)` answers *corpus position → caret rect* — the reverse of
  `positionAt`, the box to draw a caret at.

Three producers: **content fields** (a richtext body, a `richtext[]` element,
a card's content field) are tracked by the spans their glyphs carry — the
backend evaluates each value at its own generated call site and records the
site's byte window, so the rendered ink resolves back to its field through
*any* placement context, including a package that rebuilds the content (a
`show`-rule pass that buffers and re-emits paragraphs): the origin rides the
glyph, not a marker a rebuild could drop. **Direct scalar references** — each
`data.<field>` / `data.at("field")` expression in the plate is its own
tracked site; a scalar shown in header and footer surfaces both sites, and a
reference wrapped in an expression (`#upper(data.subject)`) attributes the
whole expression's ink to the field when it is the only reference inside it.
Not tracked: expressions mixing several fields (`data.from + ", " + rank` has
no single owner), values laundered through intermediate bindings, and card
scalars read from the per-card loop variable (one shared expression site
carries no per-instance identity — bind a widget for those). **Form-field widgets** carry the path explicitly
(pdfform from the form mapping, a Typst `form-field` from its `field:`
argument, validated against schema address tables baked into the generated
helper — cards carry their canonical prefix as `$path`, so plates compose
card addresses without reimplementing the kind+ordinal grammar) and surface a
region only when they bind one — a widget with no schema field is a backend
artifact, not a routable field.

### Segments and the striped union

A content field is not one box. The backend records a per-**segment** source
map (a segment is one paragraph, heading, or whole code fence — the corpus's
`continues`-joined line run), and `regions()` returns one region per
`(segment, page)`, each carrying `span: [start, end)` — the USV range of the
field's `RichText` that box covers. A scalar reference site and a widget carry
no `span` (`undefined`): geometry with no corpus address.

The whole-field highlight is **derived, not emitted**: per `(field, page)`,
union the `span`-bearing segment rects. That is the point of #829 — the union
is *striped*, leaving inter-paragraph whitespace uncovered, where the old
single box painted over it. Emitting a field-level union from the *backend*
would reintroduce the lie the disjointness invariant exists to prevent, so the
union stays out of `regions()`. But the derivation itself is subtle (which
rects carry spans, first-placement-only, widget-vs-content), so a **convenience
owns it** rather than every consumer: `fieldBoxes(field)` (on `LiveSession`,
core `field_boxes(regions, field)` for the one-shot sidecar) folds the
span-filter + per-page union, leaving `regions()` the low-level disjoint truth.
It is content-only — a field placed solely as a scalar reference or a bound
widget carries no `span` and yields nothing, its box being a single `regions()`
rect. Equivalent to the union a consumer would write by hand:

```ts
const boxes = session.fieldBoxes(field);        // one union rect per page

// …which is exactly this, per page, now owned by the helper:
const box = regions()
  .filter(r => r.field === field && r.page === page && r.span)
  .reduce(unionRect, undefined);
```

Each `(segment, page)` key surfaces its **first placement** — one region per
page it touches, so highlighting covers continuation pages (page marginals
between one page's body and the next's do not end a placement; a same-page
interruption does) — not every placement: span data cannot distinguish
package chrome interrupting one placement from a second placement of the same
value, and a spanning union would claim the ink between them. A field's own
ink *between* its segments (brackets, container-open syntax — usually inkless)
is transparent: it neither accrues a box nor breaks a run. `field` is still
not unique — segment fragments, page fragments, several scalar sites, or
content plus a bound widget each surface independently; consumers group by
`field`. Later placements stay reachable through `fieldAt` / `positionAt`,
where a concrete point identifies one drawn item unambiguously. A blank field
draws nothing and surfaces no region. Geometry only, never a value, and never
needed to complete the picture.

`positionAt` reads the same map the other way: the hit glyph's resolved node
range plus `glyph.span.1` gives an exact generated byte, which inverts through
the owning run's escape scan to a cluster-exact corpus offset. It is
**cluster-exact, not sub-character** — a hit inside a char that escaped to
several bytes floors to that cluster's first char — and degrades to the
containing segment's start on origin-less ink: a list marker or numbering
(detached-span decoration, attributable to no field — like clicking page
chrome, it resolves to nothing) and, inside a multi-line code fence, every
line sharing one resolved node wider than any per-line run, so per-line
precision collapses to the fence's corpus start (segment-level correctness
kept). Which of the two happened rides the hit as `granularity`
(`'cluster'` when an owning run resolved the offset, `'segment'` when it floored
to the segment start), so a caret UI trusts a `cluster` offset for the caret
and treats a `segment` one as a segment selection rather than guessing from the
value. `locate` forward-maps a corpus offset to a generated byte and returns
the covering glyph's box.

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
  regions(): FieldRegion[];             // field → rects (one per segment); session query, no render
  fieldBoxes(field: string): FieldRegion[];  // derived whole-field box: one union rect per page (content only)
  fieldAt(page: number, x: number, y: number): string | undefined;
                                        // point → field; PDF pt, bottom-left
  positionAt(page: number, x: number, y: number): CorpusHit | undefined;
                                        // point → { field, pos }; cluster-exact USV offset
  locate(field: string, pos: number): FieldRegion | undefined;
                                        // corpus pos → caret rect
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
  clamped: boolean;       // MAX_BACKING_DIMENSION forced densityScale down
  effectiveDensityScale: number;  // densityScale actually applied (== requested unless clamped)
}

interface FieldRegion {
  field: string;          // quill schema field path, not a widget name
  page: number;           // 0-based
  rect: [number, number, number, number];   // [x0,y0,x1,y1] PDF pt, bottom-left
  span?: [number, number];// USV [start,end) of the covered corpus; absent for scalar/widget
}

interface CorpusHit {
  field: string;
  pos: number;            // USV offset into the field's RichText (cluster floor)
  granularity?: 'cluster' | 'segment';  // was pos cluster-exact or floored to the segment start?
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
backing dimensions. `clamped` and `effectiveDensityScale` carry the fact and
the applied density on the result, so the consumer reads the clamp off the
return value rather than reconstructing it from
`pixelWidth < round(layoutWidth × densityScale)`. A clamped page renders soft
at the same `canvas.style` size; compare `effectiveDensityScale` to the
requested `densityScale` to know by how much.

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
| `pkg/backends/pdfform/` (`pdfform`)       | pdfform  | yes    | pre-flatten + hayro raster/SVG/PNG; `web-sys` canvas painter |

The pdfform backend always links its hayro raster seam, so it renders PDF, SVG,
and PNG (`supports_canvas() == true`). The wasm `pdfform` feature pulls in
`web-sys` unconditionally, so the pdfform build also ships the generic canvas
*painter* (`page_size` / `paint`, dispatching through the core `SessionHandle`
seam) — there is no painterless pdfform variant. `build-wasm.sh` builds the
three artifacts (core, typst, pdfform) sequentially; `runtime/runtime.js` maps
each backend id to its build with a `{ formats, canvas }` manifest, drift-guarded
by `runtime.test.js`.

## Non-goals

- Native (CLI / Python) exposure. Capability is WASM-only.
- Text selection, find-in-page, accessibility. Canvas has none of these by
  design — if you need them, keep an SVG/PDF export path alongside.
- Click handling in the painter. The painter is a dumb blit; it maps no
  clicks itself. Click→field lives on the **session** (`fieldAt`, hit-testing
  the compiled document) — a consumer converts the canvas click to PDF-pt
  page coordinates (the inverse of the regions overlay transform) and asks
  the session, keeping the painter free of interaction state — see
  [SCHEMAS.md](SCHEMAS.md).

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
- **No session raster cache — re-rasterize per `paint`.** Caching the last
  raster per `(page, renderScale)` and blitting on scroll-back would skip
  re-rasterizing unchanged pages (`ChangeSet` already names dirty pages to
  invalidate), but it stays unbuilt: the surface ships ahead of its first
  consumer, the megabyte-scale per-page buffers reintroduce the unbounded
  memory the viewport-bounded design set out to avoid, and any `renderScale`
  change (DPR / zoom) rotates the key and voids the cache. Consumer-side canvas
  liveness (keep the visible page's canvas alive rather than pooling) covers the
  common scroll case without that trade-off; a real consumer's profile is what
  should justify the cache and its eviction policy, not speculation.
- **Method on `LiveSession`, not a sub-handle.** Even with click resolution
  shipped (`fieldAt`), it shares no state with `paint` beyond the compile the
  whole session already owns — a `Preview` sub-handle grouping them is
  ceremony.
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
- **`warnings` accessor on `LiveSession`.** The current compile's non-fatal
  diagnostics (e.g. Typst font fallback) — set at open, refreshed by each
  committed `apply`, swapped transactionally with the compile. Without the
  accessor they are invisible to canvas consumers (only surfaced via
  `render()`'s `RenderResult`).
- **`regions()` render-free on the session; opt-in on one-shot renders.** The
  invariants are that geometry never composites (the raster is complete
  without it) and that the edit loop reads it without producing bytes — a
  paint-only consumer must never run a throwaway byte render to harvest the
  sidecar. Session exclusivity was never the invariant: there is exactly one
  producer (the frame scan over the current compile), so `RenderOptions::regions`
  attaches the same entries to `RenderResult` for consumers with no session in
  hand (static SVG overlays, PDF post-processing, CI coverage probes — and the
  native bindings, which expose no session surface at all). Off by default:
  exports pay no introspection cost, and best-effort geometry stays a request,
  not a promise attached to every artifact.

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
