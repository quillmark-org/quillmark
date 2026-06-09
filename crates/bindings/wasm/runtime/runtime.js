/* @ts-self-types="./runtime.d.ts" */
//
// @quillmark/wasm/runtime — the canonical consumer API.
//
// Consumers import `Quill`, `Document`, and `Engine` from here and never touch
// the build-specific subpaths. The package ships multiple WASM binaries with
// SEPARATE linear memories — a Typst-less `core` build (small, eager) that is
// the canonical home of `Quill`/`Document`, and one backend build per backend
// (`render` = Typst today; more later) that carries an engine. A handle from
// one memory cannot be used by another. This module hides that seam:
//
//   - `Quill` and `Document` ARE the core build's classes, re-exported. They
//     hold the canonical data and the full sync surface (schema / validate /
//     seed / mutate / toJson / toTree). No backend is loaded to use them, so
//     the editor/validation path never pays for a multi-MB backend binary.
//
//   - `Engine` is the render dispatcher. It routes on `quill.backendId`, lazily
//     imports that backend's build (so a consumer that never renders never
//     loads it), clones the canonical `Quill`/`Document` into the backend's
//     memory ON DEMAND as data (`toTree`→`fromTree`, `toJson`→`fromJson`),
//     renders, and frees the transient clones. The backend handles never escape.
//
// The cross-memory crossing is therefore invisible: a consumer hands canonical
// `Quill`/`Document` to `engine.render(...)` and gets a `RenderResult` back.

// ── CANONICAL INVARIANT: re-export `/core`, never wrap ──────────────────────
// `/runtime`'s `Quill`/`Document` ARE the core build's classes, re-exported
// verbatim — NOT subclasses or wrappers. This is load-bearing, not incidental:
//
//   * A `/core` Quill and a `/runtime` Quill are the SAME class and the SAME
//     object — identical at runtime and to TypeScript. So a core handle passes
//     to `Engine` with no convert/adopt step, and a render-less consumer
//     (quiver, an SSR service) can stay on `/core` while its handles flow into
//     a `/runtime` `Engine` unchanged. There is deliberately NO core→runtime
//     conversion API, because there is nothing to convert.
//   * The ONLY boundary that needs crossing is core→backend (a separate WASM
//     memory), and `Engine` does that internally as data (`toTree`/`toJson`).
//
// Do NOT replace this with a wrapper class. If a future requirement forces
// per-instance wrapper handles, the identity above breaks and every consumer
// then needs an explicit `adopt(coreHandle)` — so that is a deliberate,
// breaking design change, not a refactor. Keep `Engine` duck-typed on
// `.toTree()`/`.backendId`/`.toJson()` (it is) so it tolerates handles from any
// `/core` instance regardless. The `runtime.test.js` "re-exports … verbatim"
// case (`Quill === CoreQuill`) is the executable guard for this invariant.
export { Quill, Document, init } from '../core/wasm.js';

// Backend builds are NEVER statically imported here — that would pull a
// multi-MB binary into the eager graph and defeat lazy loading. Each entry is a
// thunk returning a dynamic `import()`, so a backend's chunk is fetched only
// when something actually renders against that backend.
const DEFAULT_BACKENDS = {
	typst: () => import('../render/wasm.js')
};

/**
 * Render dispatcher over the canonical `Quill`/`Document`. One `Engine`
 * instance can drive every backend; it resolves the right backend build from
 * each quill's declared `backendId` and loads it lazily on first use.
 */
export class Engine {
	/** backendId → Promise<backend module>, memoized so each build loads once. */
	#modules = new Map();
	/** backendId → that backend's engine instance (the WASM backend registry). */
	#engines = new Map();
	/** backendId → () => Promise<module>. */
	#loaders;

	/**
	 * @param {{ backends?: Record<string, () => Promise<unknown>> }} [options]
	 *   Extra or overriding backend loaders, merged over the built-ins. The
	 *   default registry maps `"typst"` to the bundled Typst build.
	 */
	constructor(options) {
		this.#loaders = { ...DEFAULT_BACKENDS, ...(options?.backends ?? {}) };
	}

	/**
	 * Resolve (and lazily load) the backend module + its engine for `backendId`.
	 * @param {string} backendId
	 * @returns {Promise<{ mod: any, engine: any }>}
	 */
	async #resolveBackend(backendId) {
		const loader = this.#loaders[backendId];
		if (!loader) {
			throw new Error(
				`Engine: no backend registered for '${backendId}'. ` +
					`Known backends: ${Object.keys(this.#loaders).join(', ') || '(none)'}.`
			);
		}

		let modPromise = this.#modules.get(backendId);
		if (!modPromise) {
			// Set the promise synchronously (before any await) so concurrent first
			// renders share ONE import. Self-heal on failure so a transient load
			// error doesn't poison every later attempt.
			modPromise = Promise.resolve()
				.then(loader)
				.catch((err) => {
					this.#modules.delete(backendId);
					throw err;
				});
			this.#modules.set(backendId, modPromise);
		}
		const mod = await modPromise;

		let engine = this.#engines.get(backendId);
		if (!engine) {
			engine = new mod.Quillmark();
			this.#engines.set(backendId, engine);
		}
		return { mod, engine };
	}

	/**
	 * Materialize a transient clone of `quill` + `doc` in `backendId`'s memory,
	 * run `fn` against the backend engine, and free the clones — even on throw.
	 * `doc` is optional (capability probes need only the quill).
	 * @param {string} backendId
	 * @param {{ toTree(): Map<string, Uint8Array> }} quill
	 * @param {{ toJson(): string } | null} doc
	 * @param {(ctx: { mod: any, engine: any, quill: any, doc: any }) => any} fn
	 */
	async #withClones(backendId, quill, doc, fn) {
		const { mod, engine } = await this.#resolveBackend(backendId);
		// Allocate the quill clone, then bring BOTH the doc clone and `fn` under
		// one try so every backend allocation is freed even if a later step throws
		// (e.g. `Document.fromJson` rejecting a cross-version DTO). `fn` MUST be
		// synchronous — the clones are freed as soon as it returns, so an async
		// `fn` would have them freed mid-flight.
		const backendQuill = mod.Quill.fromTree(quill.toTree());
		let backendDoc = null;
		try {
			backendDoc = doc ? mod.Document.fromJson(doc.toJson()) : null;
			return fn({ mod, engine, quill: backendQuill, doc: backendDoc });
		} finally {
			// Free independently: a throwing `backendDoc.free()` must not skip
			// `backendQuill.free()`.
			try {
				backendDoc?.free();
			} finally {
				backendQuill.free();
			}
		}
	}

	/**
	 * Render `doc` against `quill` in one shot, returning a `RenderResult`.
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @param {object} [options] render options (`{ format, ppi, pages, producer }`)
	 * @returns {Promise<import('../render/wasm').RenderResult>}
	 */
	async render(quill, doc, options) {
		return this.#withClones(quill.backendId, quill, doc, ({ engine, quill: q, doc: d }) =>
			engine.render(q, d, options ?? undefined)
		);
	}

	/**
	 * Open an iterative render session (for canvas preview / per-page paint).
	 * The session is a self-contained compiled snapshot, so the transient quill
	 * and document clones are freed before this returns; the caller owns the
	 * returned session and must `.free()` it.
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @returns {Promise<RenderSession>}
	 */
	async open(quill, doc) {
		return this.#withClones(
			quill.backendId,
			quill,
			doc,
			({ engine, quill: q, doc: d }) => new RenderSession(engine.open(q, d))
		);
	}

	/**
	 * The output formats `quill`'s backend can emit. Resolves the backend but
	 * compiles nothing.
	 * @param {Quill} quill
	 * @returns {Promise<import('../render/wasm').OutputFormat[]>}
	 */
	async supportedFormats(quill) {
		return this.#withClones(quill.backendId, quill, null, ({ engine, quill: q }) =>
			engine.supportedFormats(q)
		);
	}

	/**
	 * Whether `quill`'s backend can paint sessions to a canvas.
	 * @param {Quill} quill
	 * @returns {Promise<boolean>}
	 */
	async supportsCanvas(quill) {
		return this.#withClones(quill.backendId, quill, null, ({ engine, quill: q }) =>
			engine.supportsCanvas(q)
		);
	}
}

/**
 * Thin wrapper over a backend's iterative render session. Holds the compiled
 * snapshot; the quill/document it was opened from have already been freed.
 */
export class RenderSession {
	/** @param {import('../render/wasm').RenderSession} inner */
	constructor(inner) {
		this.#inner = inner;
	}
	#inner;

	get pageCount() {
		return this.#inner.pageCount;
	}
	get backendId() {
		return this.#inner.backendId;
	}
	get supportsCanvas() {
		return this.#inner.supportsCanvas;
	}
	get warnings() {
		return this.#inner.warnings;
	}

	/** @param {object} [options] */
	render(options) {
		return this.#inner.render(options ?? undefined);
	}

	/** @param {number} page */
	pageSize(page) {
		return this.#inner.pageSize(page);
	}

	/**
	 * @param {CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D} ctx
	 * @param {number} page
	 * @param {object} [options]
	 */
	paint(ctx, page, options) {
		return this.#inner.paint(ctx, page, options);
	}

	free() {
		this.#inner.free();
	}
}
