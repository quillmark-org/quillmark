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
}

/**
 * A rendered field region: the quill schema field address (`field`) plus its
 * geometry (`rect`) on the page. Emitted by backends that place schema fields
 * (`pdfform` AcroForm widgets; Typst form-fields). Only fields with a schema
 * address produce a region — a backend-only widget produces none, and the
 * backend widget name never appears.
 *
 * Use it to map between a place on the page and a field in the editor: click a
 * rendered field → focus `field` in the editor, or highlight the rect for the
 * focused field. Geometry only — `RenderSession.paint` already bakes every
 * value into the raster (see {@link RenderSession}), so a region is never a
 * compositing input.
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
}

/**
 * A positioned overlay box for one field. `left`/`top` are measured from the
 * page's LEFT/TOP edges — the Y axis is already flipped from the region's
 * bottom-left PDF origin — so the box drops straight into a CSS
 * `position:absolute` element or a `fillRect` with no further transform. Units
 * depend on the projection that produced it: {@link RegionMap.overlayPercent}
 * emits percentages of the page (for a CSS overlay on a `width:100%` canvas);
 * {@link RegionMap.overlayDevice} emits device pixels at a `renderScale` (for
 * painting into a raster).
 */
export interface OverlayBox {
	left: number;
	top: number;
	width: number;
	height: number;
}

/** A field path paired with its {@link OverlayBox} — one hotspot to draw. */
export interface FieldOverlay {
	field: string;
	box: OverlayBox;
}

/** Canonical contract every backend build must satisfy. Result of one render. */
export interface RenderResult {
	artifacts: Artifact[];
	warnings: Diagnostic[];
	outputFormat: OutputFormat;
	renderTimeMs: number;
}

/** Canonical contract every backend build must satisfy. The emittable formats. */
export type OutputFormat = 'pdf' | 'svg' | 'txt' | 'png';

/**
 * Canonical contract every backend build must satisfy. Page geometry in pt.
 * @experimental Part of the iterative-session/canvas surface — see {@link RenderSession}.
 */
export interface PageSize {
	widthPt: number;
	heightPt: number;
}

/**
 * Canonical contract every backend build must satisfy. Inputs to `paint`.
 * @experimental Part of the iterative-session/canvas surface — see {@link RenderSession}.
 */
export interface PaintOptions {
	layoutScale?: number;
	densityScale?: number;
}

/**
 * Canonical contract every backend build must satisfy. Output of `paint`.
 * @experimental Part of the iterative-session/canvas surface — see {@link RenderSession}.
 */
export interface PaintResult {
	layoutWidth: number;
	layoutHeight: number;
	pixelWidth: number;
	pixelHeight: number;
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

	/** Render `doc` against `quill` in one shot. */
	render(quill: Quill, doc: Document, options?: RenderOptions): Promise<RenderResult>;

	/**
	 * Open an iterative render session (canvas preview / per-page paint).
	 * @experimental Ships ahead of its first production consumer (the designed
	 * canvas live-preview path — see `prose/canon/PREVIEW.md`). The session/paint
	 * surface may change in any 0.x release; `render()` is the stable path.
	 */
	open(quill: Quill, doc: Document): Promise<RenderSession>;

	/**
	 * Output formats `quill`'s backend can emit. An ALWAYS-free pre-render probe:
	 * it answers from the descriptor's required `formats` manifest WITHOUT loading
	 * the backend binary or cloning the quill. Async for API stability.
	 */
	supportedFormats(quill: Quill): Promise<OutputFormat[]>;

	/**
	 * Whether `quill`'s backend can paint sessions to a canvas. Same always-free
	 * probe as `supportedFormats`: answered from the descriptor's required
	 * `canvas` manifest, no binary load and no quill clone. Both the Typst and
	 * pdfform backends report `true`; each paints a complete page raster (see
	 * {@link RenderSession.paint}).
	 * @experimental Probes the experimental session/canvas surface — see {@link RenderSession}.
	 */
	supportsCanvas(quill: Quill): Promise<boolean>;
}

/**
 * Iterative render session over a compiled snapshot. `free()` when done.
 *
 * CANVAS PAINT IS COMPLETE. {@link RenderSession.paint} writes a complete page
 * raster — every piece of page content is already visible in the painted
 * pixels, with NO compositing required by the caller. Both backends that
 * support canvas satisfy this: Typst rasterizes its laid-out page natively;
 * pdfform pre-flattens bound field values into the page content and rasterizes
 * that, so field values appear in the raster on their own.
 * {@link RenderSession.regions} carries schema-field geometry for interactive
 * overlays / cross-navigation drawn on top of the raster; it is never needed to
 * complete the picture.
 *
 * @experimental The whole session/canvas-paint surface (`Engine.open`,
 * `RenderSession`, `PaintOptions`, `PaintResult`, `PageSize`) ships ahead of
 * its first production consumer and may change shape in any 0.x release.
 * The stable render path is `Engine.render`.
 */
export declare class RenderSession {
	private constructor();
	readonly pageCount: number;
	readonly backendId: string;
	readonly supportsCanvas: boolean;
	readonly warnings: Diagnostic[];
	render(options?: RenderOptions): RenderResult;
	/**
	 * Schema-field geometry for this compiled session — one {@link FieldRegion}
	 * per schema-bound field, keyed on its quill schema field path. A
	 * session-level query: no render, no byte artifact. Read it to place field
	 * overlays / cross-navigation over a `paint`-ed canvas. Empty for backends
	 * that place no schema fields.
	 */
	regions(): FieldRegion[];
	/** Page geometry in points (1/72″). Report-only; the painter sizes the canvas. */
	pageSize(page: number): PageSize;
	/**
	 * Paint `page` into a 2D canvas context, sizing the backing store itself
	 * (it owns `canvas.width`/`height`; the caller owns `canvas.style.*`). The
	 * painted raster is COMPLETE — all page content visible, no caller-side
	 * compositing (Typst rasterizes natively; pdfform rasterizes its
	 * pre-flattened page). Effective rasterization scale is
	 * `layoutScale × densityScale`, clamped so neither backing dimension exceeds
	 * 16384 px — detect a clamp via {@link PaintResult.pixelWidth}.
	 */
	paint(
		ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
		page: number,
		options?: PaintOptions
	): PaintResult;
	free(): void;
}

/**
 * Per-page projection of {@link RenderSession.regions} into draw-ready overlay
 * geometry and a click hit-test — the canonical answer to the region coordinate
 * transform (Y-flip, bottom-left→top-left origin, pt↔device-px), so a consumer
 * never re-derives it and gets the flip wrong. Pure data: no WASM, no DOM, no
 * session reference, so a `render()`-only consumer never pulls it in. Build one
 * per page; it is cheap (a filter plus arithmetic).
 *
 * ```js
 * const map = RegionMap.from(session.regions(), session.pageSize(page), page);
 *
 * for (const { field, box } of map.overlaysPercent()) {        // CSS overlay,
 *   const el = document.createElement('div');                  // width:100% canvas
 *   el.style.cssText =
 *     `position:absolute;left:${box.left}%;top:${box.top}%;` +
 *     `width:${box.width}%;height:${box.height}%`;
 *   el.dataset.field = field;                                  // click → focus editor field
 * }
 *
 * const r = canvas.getBoundingClientRect();                    // hit-test a click
 * const hit = map.at(((e.clientX - r.left) / r.width) * 100,
 *                    ((e.clientY - r.top) / r.height) * 100);
 * if (hit) focusEditorField(hit.field);
 * ```
 *
 * @experimental Part of the experimental session/canvas surface — see
 * {@link RenderSession}. May change shape in any 0.x release.
 */
export declare class RegionMap {
	private constructor();
	/**
	 * Build the map for `page` from a session's full {@link RenderSession.regions}
	 * list and that page's {@link RenderSession.pageSize}. Regions not on `page`
	 * are dropped. Throws if `pageSize` is not positive and finite on both axes.
	 */
	static from(regions: FieldRegion[], pageSize: PageSize, page: number): RegionMap;
	/** The 0-based page this map projects. */
	readonly page: number;
	/** The page size every projection divides by, as passed to {@link from}. */
	readonly pageSize: PageSize;
	/** Field paths on this page, in {@link RenderSession.regions} order. */
	readonly fields: string[];
	/** The raw region for `field`, or `undefined` if it is not on this page. */
	region(field: string): FieldRegion | undefined;
	/**
	 * The field at a point given in page percent (0–100, top-left origin) — the
	 * unit {@link overlayPercent} emits and the natural unit of a click on a
	 * `width:100%` canvas. Returns the SMALLEST region containing the point (the
	 * most specific field when boxes nest), or `undefined` if none do. A
	 * non-finite coordinate matches nothing.
	 */
	at(xPercent: number, yPercent: number): FieldRegion | undefined;
	/** {@link OverlayBox} for `field` in page percent, or `undefined` if absent. */
	overlayPercent(field: string): OverlayBox | undefined;
	/**
	 * {@link OverlayBox} for `field` in device pixels at `renderScale`
	 * (= `layoutScale × densityScale` from the matching {@link RenderSession.paint}),
	 * or `undefined` if absent. Throws if `renderScale` is not positive and finite.
	 */
	overlayDevice(field: string, renderScale: number): OverlayBox | undefined;
	/** Every field's percent overlay — for drawing all hotspots at once. */
	overlaysPercent(): FieldOverlay[];
	/** Every field's device-pixel overlay at `renderScale`. */
	overlaysDevice(renderScale: number): FieldOverlay[];
}
