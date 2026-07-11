// @quillmark/wasm/runtime — canonical consumer API.
//
// `Quill`/`Document` are re-exported verbatim from the core build (their full
// surface, no drift). Render-side types (`RenderResult`, `RenderOptions`,
// `Artifact`, `OutputFormat`, `PageSize`, `PaintOptions`, `PaintResult`) are
// defined HERE as the canonical, backend-neutral render contract — NOT sourced
// from any one private backend build. A type-level drift guard
// (`runtime.types.test-d.ts`, via `npm run typecheck`) asserts they stay
// mutually assignable with the Typst backend's generated declarations. `Engine`
// is the render dispatcher that hides the cross-WASM-memory seam.

// CANONICAL INVARIANT: the root re-exports the core build's `Quill`/`Document`
// verbatim — they are the SAME classes, never wrappers. There is exactly one
// public entry point, so this is a structural fact. Replacing the re-export
// with a wrapper is a breaking design change, not a refactor. See runtime.js.
export { Quill, Document, init } from '../core/wasm.js';

// Core-build types consumers read off `Quill`/`Document`.
export type {
	Card,
	PayloadItem,
	Diagnostic,
	Location,
	Severity,
	QuillSchema,
	QuillFieldSchema,
	QuillCardSchema,
	QuillCardBody,
	QuillFieldUi,
	QuillCardUi,
	QuillMetadata
} from '../core/wasm.js';

// ── Error contract ──────────────────────────────────────────────────────────

/**
 * The error every fallible method in this package throws — parse
 * (`Document.fromMarkdown`), document mutation, validation
 * (`Quill.fromTree`, `quill.validate`), and rendering (`engine.render`,
 * `engine.open`, `session.render`).
 *
 * This is a STRUCTURAL interface, not a class: the WASM layer throws a real
 * `Error` and attaches `diagnostics` to it, so there is no constructor to
 * `instanceof` against — narrow with {@link isQuillmarkError}. `diagnostics`
 * is always non-empty; `message` is the first diagnostic's message (or an
 * `"N error(s): …"` aggregate for multi-diagnostic failures), so iterate
 * `diagnostics` for per-error detail. The shape is identical to
 * `RenderResult.warnings` entries.
 */
export interface QuillmarkError extends Error {
	diagnostics: Diagnostic[];
}

/**
 * Narrow an unknown caught value to {@link QuillmarkError}. Structural
 * (`Error` carrying a `diagnostics` array), so it works on errors from any
 * build or WASM instance in the page — consistent with the package's
 * duck-typed handling of handles.
 */
export declare function isQuillmarkError(e: unknown): e is QuillmarkError;

// ── Canonical render-side types ─────────────────────────────────────────────
// These are the BACKEND-NEUTRAL render contract of the plural-backend API. They
// are defined HERE (not re-exported from one private backend) because no single
// backend build owns the canonical API's types. Every backend build MUST satisfy
// these shapes; that they match the Typst backend's generated declarations is
// enforced by the type-level drift guard `crates/bindings/wasm/runtime.types.test-d.ts`
// (run via `npm run typecheck`), so these and the generated
// `pkg/backends/typst/wasm.d.ts` cannot silently diverge.

import type { Quill, Document } from '../core/wasm.js';
import type { Diagnostic } from '../core/wasm.js';

/** Canonical contract every backend build must satisfy. One emitted output. */
export interface Artifact {
	format: OutputFormat;
	bytes: Uint8Array;
	mimeType: string;
}

/** Canonical contract every backend build must satisfy. Options for one render. */
export interface RenderOptions {
	format?: OutputFormat;
	ppi?: number;
	pages?: number[];
	producer?: string;
	/**
	 * Populate {@link RenderResult.regions} with the schema-field geometry
	 * sidecar (the same entries {@link LiveSession.regions} serves), for
	 * consumers without a live session — e.g. overlays over a one-shot SVG
	 * export. Defaults to `false`: exports pay no introspection cost.
	 */
	regions?: boolean;
}

/**
 * How precisely a {@link CorpusHit.pos} resolved — the marker a caret UI reads
 * to decide whether to trust the offset. Never sub-cluster: `'cluster'` is the
 * finest, `'segment'` the floor it degrades to on origin-less ink.
 *
 * - `'cluster'` — `pos` is the first corpus char of the cluster under the point
 *   (an escaped/CJK/shaping cluster floors to its first char). Place the caret
 *   at `pos` directly.
 * - `'segment'` — the point hit origin-less ink (list markers, numbering, a
 *   multi-line code fence's interior), so `pos` degraded to the containing
 *   segment's start. Treat `pos` as the selected segment, not a caret.
 */
export type HitGranularity = 'cluster' | 'segment';

/** A click resolved to a field and USV offset into its RichText. */
export interface CorpusHit {
	field: string;
	pos: number;
	/**
	 * Whether {@link pos} is cluster-exact or floored to the segment start
	 * ({@link HitGranularity}). Absent when the backend does not report it.
	 */
	granularity?: HitGranularity;
}

/**
 * A rendered field region: the quill schema field address (`field`) plus its
 * geometry (`rect`) on the page. Emitted by backends that place schema fields
 * (`pdfform` AcroForm widgets; Typst form-fields and span-tracked content —
 * richtext bodies, `richtext[]` elements, card content fields, direct scalar
 * references). Only fields with a schema address produce a region — a
 * backend-only widget produces none, and the backend widget name never
 * appears.
 *
 * Use it to scroll to / highlight the focused field's rect; for the click
 * direction use {@link LiveSession.fieldAt}, which resolves a point on *any*
 * placement, not just the first one surfaced here. Geometry only —
 * `LiveSession.paint` already bakes every value into the raster (see
 * {@link LiveSession}), so a region is never a compositing input.
 *
 * COORDINATE TRANSFORM. `rect` is in PDF points with a **bottom-left** origin.
 *
 * For an **HTML/CSS overlay** on a `width:100%` canvas, position hotspots as
 * percentages of the page — they track the displayed size across DPI and pane
 * resize for free, and only the Y axis flips:
 *
 * ```js
 * const [x0, y0, x1, y1] = region.rect;            // PDF pt, bottom-left origin
 * const left   = (x0 / pageWidthPt) * 100;         // % of page (from PageSize.widthPt)
 * const top    = (1 - y1 / pageHeightPt) * 100;    // % — flip Y (from PageSize.heightPt)
 * const width  = ((x1 - x0) / pageWidthPt) * 100;
 * const height = ((y1 - y0) / pageHeightPt) * 100;
 * ```
 *
 * For painting **into a raster** at `renderScale` (= `layoutScale × densityScale`),
 * use the device-pixel form instead:
 *
 * ```js
 * const left   = x0 * renderScale;
 * const top    = (pageHeightPt - y1) * renderScale;  // flip Y
 * ```
 */
export interface FieldRegion {
	/** Quill schema field path (e.g. `"signature_block"`), not a backend widget name. */
	field: string;
	/** 0-based page index. */
	page: number;
	/** `[x0, y0, x1, y1]` in PDF points (1/72″), bottom-left origin. */
	rect: [number, number, number, number];
	/**
	 * The corpus slice this box covers — USV `[start, end)` into the field's
	 * `RichText` for content ink (one segment), absent for a scalar reference
	 * site or widget. Consumers key segment highlights on it;
	 * {@link LiveSession.fieldBoxes} unions same-page segments for the
	 * whole-field box.
	 */
	span?: [number, number];
}

/** Canonical contract every backend build must satisfy. Result of one render. */
export interface RenderResult {
	artifacts: Artifact[];
	warnings: Diagnostic[];
	outputFormat: OutputFormat;
	renderTimeMs: number;
	/**
	 * Schema-field geometry sidecar — populated only when
	 * {@link RenderOptions.regions} requested it; empty otherwise. The same
	 * entries {@link LiveSession.regions} serves, for consumers without a live
	 * session. Page indices are document-space even under a `pages` subset
	 * render.
	 */
	regions: FieldRegion[];
}

/** Canonical contract every backend build must satisfy. The emittable formats. */
export type OutputFormat = 'pdf' | 'svg' | 'txt' | 'png';

/**
 * Canonical contract every backend build must satisfy. Page geometry in pt.
 * @experimental Part of the iterative-session/canvas surface — see {@link LiveSession}.
 */
export interface PageSize {
	widthPt: number;
	heightPt: number;
}

/**
 * Canonical contract every backend build must satisfy. Inputs to `paint`.
 * @experimental Part of the iterative-session/canvas surface — see {@link LiveSession}.
 */
export interface PaintOptions {
	layoutScale?: number;
	densityScale?: number;
}

/**
 * Canonical contract every backend build must satisfy. Output of `paint`.
 * @experimental Part of the iterative-session/canvas surface — see {@link LiveSession}.
 */
export interface PaintResult {
	layoutWidth: number;      // canvas.style.width target; independent of densityScale
	layoutHeight: number;
	pixelWidth: number;       // canvas.width the painter wrote (clamped at 16384)
	pixelHeight: number;
	/**
	 * True when `MAX_BACKING_DIMENSION` forced `densityScale` down: the page is
	 * painted at fewer device pixels than requested and renders soft at the same
	 * `canvas.style` size. Reads the clamp off the return value instead of the
	 * `pixelWidth < round(layoutWidth × densityScale)` derivation.
	 */
	clamped: boolean;
	/**
	 * The `densityScale` actually applied — equal to the requested value unless
	 * `clamped`, then reduced proportionally. `layoutScale × effectiveDensityScale`
	 * is the scale the backing store was rasterized at.
	 */
	effectiveDensityScale: number;
}

/**
 * Canonical contract every backend build must satisfy. Output of
 * {@link LiveSession.apply}: `dirtyPages` lists the pages whose rendered
 * content differs from the previous compile, including added pages; removed
 * pages are implied by `pageCount`. Repaint `dirty ∩ visible`.
 * @experimental Part of the live-session/canvas surface — see {@link LiveSession}.
 */
export interface ChangeSet {
	pageCount: number;
	dirtyPages: number[];
}

/**
 * A backend registry entry. `load` is the lazy thunk returning the dynamically-
 * imported backend build module; `formats`/`canvas` are the REQUIRED static
 * capability manifest. That manifest is what makes
 * `Engine.supportedFormats`/`Engine.supportsCanvas` always FREE: they answer
 * from it directly — no backend binary is loaded and no quill is cloned into
 * backend memory. A malformed descriptor throws at `new Engine(...)`.
 */
export interface BackendDescriptor {
	load: () => Promise<unknown>;
	formats: OutputFormat[];
	canvas: boolean;
}

export interface EngineOptions {
	/**
	 * Extra or overriding backend descriptors, merged over the built-ins. Keys are
	 * backend ids (as declared by `Quill.yaml`'s `backend:` and reported by
	 * `Quill.backendId`). Each value is a `BackendDescriptor` — `formats`/`canvas`
	 * are required, so capability probes are ALWAYS free (no binary load, no quill
	 * clone). Malformed entries throw at construction. The default registry maps
	 * `"typst"` to the bundled Typst build.
	 */
	backends?: Record<string, BackendDescriptor>;
}

/**
 * Render dispatcher over the canonical `Quill`/`Document`. Routes on
 * `quill.backendId`, lazily loads that backend build, clones the quill and
 * document into the backend's WASM memory on demand, renders, and frees the
 * clones. The cross-memory crossing is invisible to callers.
 */
export declare class Engine {
	constructor(options?: EngineOptions);

	/**
	 * Render `doc` against `quill` in one shot. Both handles are read
	 * synchronously before the first await, so the caller may `free()` them as
	 * soon as this call returns.
	 */
	render(quill: Quill, doc: Document, options?: RenderOptions): Promise<RenderResult>;

	/**
	 * Open a live render session (canvas preview / per-page paint / `apply`).
	 * The `quill` and `doc` handles are read synchronously before the first
	 * await, so the caller may `free()` them as soon as this call returns; the
	 * caller owns the returned session and must `.free()` it.
	 * @experimental Ships ahead of its first production consumer (the designed
	 * canvas live-preview path — see `prose/canon/PREVIEW.md`). The session/paint
	 * surface may change in any 0.x release; `render()` is the stable path.
	 */
	open(quill: Quill, doc: Document): Promise<LiveSession>;

	/**
	 * Output formats `quill`'s backend can emit. An ALWAYS-free pre-render probe:
	 * it answers from the descriptor's required `formats` manifest WITHOUT loading
	 * the backend binary or cloning the quill. Async for API stability.
	 */
	supportedFormats(quill: Quill): Promise<OutputFormat[]>;

	/**
	 * Whether `quill`'s BACKEND can paint sessions to a canvas — a pre-session
	 * ESTIMATE, not a fact about any particular compile. Same always-free probe
	 * as `supportedFormats`: answered from the descriptor's required `canvas`
	 * manifest, no binary load and no quill clone. Both the Typst and pdfform
	 * backends report `true` here unconditionally; each paints a complete page
	 * raster (see {@link LiveSession.paint}) — but a specific compile can still
	 * refuse to paint (e.g. a 0-page document), so this can answer `true` while
	 * the resulting {@link LiveSession.supportsCanvas} answers `false`. Gate
	 * mounting a canvas UI on this; gate the actual `paint` call on the session's
	 * getter once `open()` has run.
	 * @experimental Probes the experimental session/canvas surface — see {@link LiveSession}.
	 */
	supportsCanvas(quill: Quill): Promise<boolean>;
}

/**
 * Iterative render session over a compiled snapshot. `free()` when done.
 *
 * CANVAS PAINT IS COMPLETE. {@link LiveSession.paint} writes a complete page
 * raster — every piece of page content is already visible in the painted
 * pixels, with NO compositing required by the caller. Both backends that
 * support canvas satisfy this: Typst rasterizes its laid-out page natively;
 * pdfform pre-flattens bound field values into the page content and rasterizes
 * that, so field values appear in the raster on their own.
 * {@link LiveSession.regions} carries schema-field geometry for interactive
 * overlays / cross-navigation drawn on top of the raster; it is never needed to
 * complete the picture.
 *
 * @experimental The whole session/canvas-paint surface (`Engine.open`,
 * `LiveSession`, `PaintOptions`, `PaintResult`, `PageSize`) ships ahead of
 * its first production consumer and may change shape in any 0.x release.
 * The stable render path is `Engine.render`.
 */
export declare class LiveSession {
	private constructor();
	readonly pageCount: number;
	readonly backendId: string;
	/**
	 * `true` iff `paint`/`pageSize` will succeed for THIS compile — the
	 * authoritative answer, derived from the session's canvas seam, so it can
	 * never disagree with what `paint` actually does. This can be `false` even
	 * when {@link Engine.supportsCanvas} answered `true` for the same `quill`
	 * (that probe is a pre-session backend estimate; e.g. a canvas-capable
	 * backend compiled to a 0-page document has nothing to paint). Re-check
	 * this getter after `open()` rather than relying on the engine hint alone.
	 */
	readonly supportsCanvas: boolean;
	readonly warnings: Diagnostic[];
	/**
	 * Recompile the session against `doc` — the edit verb of a live preview.
	 * Transactional: on throw every read (`render`, `paint`, `pageSize`,
	 * `regions`) keeps serving the last-good compile, and the session recovers
	 * on the next successful `apply`. On success reads serve the new compile;
	 * repaint `dirtyPages ∩ visible`.
	 */
	apply(doc: Document): ChangeSet;
	render(options?: RenderOptions): RenderResult;
	/**
	 * Schema-field geometry for this compiled session, keyed on quill schema
	 * field path. A session-level query: no render, no byte artifact. Read it
	 * to scroll to / highlight the focused field over a `paint`-ed canvas;
	 * the click direction is {@link fieldAt}. Empty for backends that place
	 * no schema fields.
	 *
	 * `field` is **not** unique: a content field surfaces its **first
	 * placement** as one {@link FieldRegion} per page that placement touches
	 * (so a highlight covers continuation pages); a scalar referenced at
	 * several plate sites surfaces each site; tracked content plus a
	 * `field:`-bound widget yields both, widget ordered first. Group by
	 * `field` — every entry routes to that field. Later placements of one
	 * content value are not enumerated; {@link fieldAt} still resolves
	 * clicks on them.
	 */
	regions(): FieldRegion[];
	/**
	 * The whole-field highlight boxes for `field` — one union rect per page,
	 * over the field's `span`-bearing content segments (the "highlight the
	 * focused field" quantity). Owns the union {@link regions} leaves derived
	 * (span-filter + per-page union), keeping `regions()` the low-level disjoint
	 * truth, so a consumer stops reimplementing it. **Content only** — a field
	 * placed solely as a scalar reference or a bound widget carries no `span`
	 * and returns `[]`; its box is a single {@link regions} rect. Reflects the
	 * current compile, like `regions()`.
	 */
	fieldBoxes(field: string): FieldRegion[];
	/**
	 * The schema field whose content is under a point on `page` — the forward
	 * (click → field) direction: hit-test a click against the compiled
	 * document and get back the field address to focus in the editor, or
	 * `undefined` off any field's ink. `x`/`y` are PDF points with a
	 * **bottom-left** origin, the same space as {@link FieldRegion.rect} —
	 * from a canvas click, invert the overlay transform documented there:
	 * `x = clickPx.x / renderScale`,
	 * `y = pageHeightPt - clickPx.y / renderScale`. Unlike {@link regions},
	 * *every* placement answers, not just the first.
	 */
	fieldAt(page: number, x: number, y: number): string | undefined;
	/**
	 * Fine-grained click → corpus position (caret placement). Same PDF-point
	 * space as {@link fieldAt}; `undefined` off all content ink.
	 */
	positionAt(page: number, x: number, y: number): CorpusHit | undefined;
	/** Corpus position → caret rect — reverse of {@link positionAt}. */
	locate(field: string, pos: number): FieldRegion | undefined;
	/** Page geometry in points (1/72″). Report-only; the painter sizes the canvas. */
	pageSize(page: number): PageSize;
	/**
	 * Paint `page` into a 2D canvas context, sizing the backing store itself
	 * (it owns `canvas.width`/`height`; the caller owns `canvas.style.*`). The
	 * painted raster is COMPLETE — all page content visible, no caller-side
	 * compositing (Typst rasterizes natively; pdfform rasterizes its
	 * pre-flattened page). Effective rasterization scale is
	 * `layoutScale × densityScale`, clamped so neither backing dimension exceeds
	 * 16384 px — {@link PaintResult.clamped} reports the clamp and
	 * {@link PaintResult.effectiveDensityScale} the density actually applied.
	 *
	 * The write is a whole-backing-store `putImageData`, which bypasses the 2D
	 * context transform, `globalAlpha`, and clip: the painter owns the entire
	 * canvas, so give each visible page its own `` element. You cannot
	 * paint two pages into one canvas, paint into a sub-rect, or apply a context
	 * transform through this call — the raster is complete precisely so you never
	 * need to. Keep the per-page canvases alive while their pages stay near the
	 * viewport: each `paint` re-rasterizes from scratch, so reusing (pooling) a
	 * canvas across pages on scroll re-runs a full render, whereas an idle canvas
	 * retains its pixels for free.
	 */
	paint(
		ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
		page: number,
		options?: PaintOptions
	): PaintResult;
	free(): void;
}
