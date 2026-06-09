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

// CANONICAL INVARIANT: `/runtime` re-exports `/core`'s `Quill`/`Document`
// verbatim — they are the SAME classes, never wrappers. A `/core` handle and a
// `/runtime` handle are therefore one type; no convert/adopt API exists or is
// needed. Replacing this with a wrapper is a breaking design change (consumers
// would then need an explicit `adopt`), not a refactor. See runtime.js.
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

/** Canonical contract every backend build must satisfy. Result of one render. */
export interface RenderResult {
	artifacts: Artifact[];
	warnings: Diagnostic[];
	outputFormat: OutputFormat;
	renderTimeMs: number;
}

/** Canonical contract every backend build must satisfy. The emittable formats. */
export type OutputFormat = 'pdf' | 'svg' | 'txt' | 'png';

/** Canonical contract every backend build must satisfy. Page geometry in pt. */
export interface PageSize {
	widthPt: number;
	heightPt: number;
}

/** Canonical contract every backend build must satisfy. Inputs to `paint`. */
export interface PaintOptions {
	layoutScale?: number;
	densityScale?: number;
}

/** Canonical contract every backend build must satisfy. Output of `paint`. */
export interface PaintResult {
	layoutWidth: number;
	layoutHeight: number;
	pixelWidth: number;
	pixelHeight: number;
}

/** A backend loader: returns the dynamically-imported backend build module. */
export type BackendLoader = () => Promise<unknown>;

/**
 * Descriptor form of a backend registry entry. `load` is the lazy thunk; the
 * optional `formats`/`canvas` are a STATIC capability manifest. When present,
 * `Engine.supportedFormats`/`Engine.supportsCanvas` answer from them directly —
 * FREE: no backend binary is loaded and no quill is cloned into backend memory.
 * Omit them (or pass a bare `BackendLoader` thunk) and those probes fall back to
 * loading the binary and cloning the quill to ask the backend.
 */
export interface BackendDescriptor {
	load: BackendLoader;
	formats?: OutputFormat[];
	canvas?: boolean;
}

export interface EngineOptions {
	/**
	 * Extra or overriding backend loaders, merged over the built-ins. Keys are
	 * backend ids (as declared by `Quill.yaml`'s `backend:` and reported by
	 * `Quill.backendId`). Each value is either a bare `BackendLoader` thunk or a
	 * `BackendDescriptor`. Use the descriptor form with `formats`/`canvas` to make
	 * capability probes free (no binary load). The default registry maps
	 * `"typst"` to the bundled Typst build in descriptor form.
	 */
	backends?: Record<string, BackendLoader | BackendDescriptor>;
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

	/** Open an iterative render session (canvas preview / per-page paint). */
	open(quill: Quill, doc: Document): Promise<RenderSession>;

	/**
	 * Output formats `quill`'s backend can emit. A cheap pre-render probe: for a
	 * descriptor-form backend with a `formats` manifest (the default Typst entry)
	 * it answers WITHOUT loading the backend binary or cloning the quill; for a
	 * bare-thunk backend it loads + clones to ask the backend. Async either way.
	 */
	supportedFormats(quill: Quill): Promise<OutputFormat[]>;

	/**
	 * Whether `quill`'s backend can paint sessions to a canvas. Same cheap-probe
	 * contract as `supportedFormats`: free for descriptor-form backends carrying a
	 * `canvas` manifest, load+clone fallback for bare-thunk backends.
	 */
	supportsCanvas(quill: Quill): Promise<boolean>;

	/**
	 * Drop and free this engine's cached backend-memory clone of `quill` (across
	 * every backend).
	 *
	 * CACHING CONTRACT: to avoid re-serializing + re-validating a quill bundle on
	 * every `render`/`open`, the `Engine` caches the backend-memory clone of each
	 * `Quill` instance and assumes that instance's contents NEVER change after
	 * construction. Honour this by mutating-by-replacing-the-instance. If you must
	 * republish changed contents under the same `Quill` instance, call
	 * `invalidate(quill)` afterwards so the next render re-materializes it.
	 */
	invalidate(quill: Quill): void;

	/**
	 * Drop and free every cached backend-memory quill clone in this engine, for
	 * every backend. Coarse counterpart to `invalidate` — see its caching
	 * contract.
	 */
	invalidateAll(): void;
}

/** Iterative render session over a compiled snapshot. `free()` when done. */
export declare class RenderSession {
	private constructor();
	readonly pageCount: number;
	readonly backendId: string;
	readonly supportsCanvas: boolean;
	readonly warnings: Diagnostic[];
	render(options?: RenderOptions): RenderResult;
	pageSize(page: number): PageSize;
	paint(
		ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
		page: number,
		options?: PaintOptions
	): PaintResult;
	free(): void;
}
