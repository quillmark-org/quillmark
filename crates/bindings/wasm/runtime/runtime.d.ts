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

/** The kind and payload of a {@link FieldRegion}. Discriminated on `type`. */
export type FieldRegionKind =
	| { type: 'field'; fieldType: string; value?: string };

/**
 * A form-field region: geometry and bound value from a stamped AcroForm.
 * Emitted by backends that stamp form fields (`pdfform`; Typst signature
 * overlay). Consumers use `rect` for *overlay* layout (e.g. positioning an
 * input box over a canvas) and `kind.value` to read the bound value.
 *
 * Regions are NOT needed to make a canvas paint complete: `RenderSession.paint`
 * already bakes every field value into the raster (see {@link RenderSession}).
 * They exist for interactive overlays drawn on top of that raster.
 *
 * COORDINATE TRANSFORM. `rect` is in PDF points with a **bottom-left** origin;
 * a canvas is **top-left** origin in device pixels. To place an overlay from a
 * region onto a canvas painted at `renderScale` (= `layoutScale × densityScale`)
 * for a page `pageHeightPt` tall (from {@link PageSize}.heightPt):
 *
 * ```js
 * const [x0, y0, x1, y1] = region.rect;       // PDF pt, bottom-left origin
 * const left   = x0 * renderScale;
 * const right  = x1 * renderScale;
 * const top    = (pageHeightPt - y1) * renderScale;  // flip Y
 * const bottom = (pageHeightPt - y0) * renderScale;  // y_canvas = (pageHeightPt - y_pdf) × renderScale
 * ```
 */
export interface FieldRegion {
	/** Fully-qualified field name (matches the AcroForm widget `/T`). */
	name: string;
	/** 0-based page index. */
	page: number;
	/** `[x0, y0, x1, y1]` in PDF points (1/72″), bottom-left origin. */
	rect: [number, number, number, number];
	kind: FieldRegionKind;
}

/** Canonical contract every backend build must satisfy. Result of one render. */
export interface RenderResult {
	artifacts: Artifact[];
	warnings: Diagnostic[];
	outputFormat: OutputFormat;
	renderTimeMs: number;
	/**
	 * Form-field regions from stamped AcroForm backends. Always an array —
	 * empty for backends / formats that produce no field geometry. The only
	 * path to field values in non-interactive flat output.
	 */
	regions: FieldRegion[];
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
 * that, so field values appear in the raster on their own. The
 * {@link FieldRegion} sidecar carries field geometry for interactive overlays
 * drawn on top of the raster; it is never needed to complete the picture.
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
