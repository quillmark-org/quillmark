// @quillmark/wasm/runtime — canonical consumer API.
//
// `Quill`/`Document` are re-exported verbatim from the core build (their full
// surface, no drift). Render-side types are re-exported TYPE-ONLY, so they add
// no runtime import edge to a backend build. `Engine` is the render dispatcher
// that hides the cross-WASM-memory seam.

// CANONICAL INVARIANT: `/runtime` re-exports `/core`'s `Quill`/`Document`
// verbatim — they are the SAME classes, never wrappers. A `/core` handle and a
// `/runtime` handle are therefore one type; no convert/adopt API exists or is
// needed. Replacing this with a wrapper is a breaking design change (consumers
// would then need an explicit `adopt`), not a refactor. See runtime.js.
export { Quill, Document, init } from '../core/wasm';

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
} from '../core/wasm';

// Render-side types — TYPE-ONLY (erased; no backend binary is pulled eagerly).
export type {
	RenderResult,
	RenderOptions,
	Artifact,
	OutputFormat,
	PageSize,
	PaintOptions,
	PaintResult
} from '../render/wasm';

import type { Quill, Document } from '../core/wasm';
import type {
	RenderResult,
	RenderOptions,
	OutputFormat,
	Diagnostic,
	PageSize,
	PaintOptions,
	PaintResult
} from '../render/wasm';

/** A backend loader: returns the dynamically-imported backend build module. */
export type BackendLoader = () => Promise<unknown>;

export interface EngineOptions {
	/**
	 * Extra or overriding backend loaders, merged over the built-ins. Keys are
	 * backend ids (as declared by `Quill.yaml`'s `backend:` and reported by
	 * `Quill.backendId`). The default registry maps `"typst"` to the bundled
	 * Typst build.
	 */
	backends?: Record<string, BackendLoader>;
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

	/** Output formats `quill`'s backend can emit (resolves the backend; compiles nothing). */
	supportedFormats(quill: Quill): Promise<OutputFormat[]>;

	/** Whether `quill`'s backend can paint sessions to a canvas. */
	supportsCanvas(quill: Quill): Promise<boolean>;
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
