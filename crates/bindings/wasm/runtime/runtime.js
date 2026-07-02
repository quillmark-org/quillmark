/* @ts-self-types="./runtime.d.ts" */
//
// @quillmark/wasm/runtime — the canonical consumer API.
//
// Consumers import `Quill`, `Document`, and `Engine` from here and never touch
// the build-specific subpaths. The package ships multiple WASM binaries with
// SEPARATE linear memories — a Typst-less `core` build (small, eager) that is
// the canonical home of `Quill`/`Document`, and one private backend binary per
// backend (`backends/typst/` today; more later) that carries an engine. A
// handle from one memory cannot be used by another. This module hides that seam
// and is exposed at the package root (`@quillmark/wasm`):
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
//     renders, and the backend handles never escape.
//
//     CLONE LIFETIMES (not all transient): the per-call `Document` clone IS
//     transient — built and freed inside each call, because documents are small
//     and mutate freely. The `Quill` clone is CACHED instead: re-cloning a quill
//     per call means re-serializing its whole file tree Rust→JS, copying it into
//     backend memory, and re-parsing + re-validating the bundle every time — and
//     quills are validated, effectively-immutable bundles. So each `Engine`
//     memoizes the backend-memory quill per (engine, backendId, canonical quill
//     instance) in a `WeakMap` keyed on the canonical `Quill`: when the consumer
//     drops the core quill the cache entry becomes collectable and wasm-bindgen
//     weak-refs (`--weak-refs`) free the backend handle. The CONTRACT this buys:
//     a `Quill` instance's contents never change after construction — mutate by
//     replacing the instance (the clone is dropped with it via WeakMap +
//     weak-refs).
//
// The cross-memory crossing is therefore invisible: a consumer hands canonical
// `Quill`/`Document` to `engine.render(...)` and gets a `RenderResult` back.

// ── CANONICAL INVARIANT: re-export the core build, never wrap ───────────────
// The root re-exports the core build's `Quill`/`Document` classes verbatim —
// NOT subclasses or wrappers. There is exactly ONE public entry point (this
// module), so this identity is a structural fact: `Quill`/`Document` ARE the
// core classes, and the only boundary that needs crossing is core→backend (a
// separate WASM memory), which `Engine` does internally as data
// (`toTree`/`toJson`).
//
// Do NOT replace this with a wrapper class — that breaks the identity and turns
// a structural fact into a converted type (a breaking design change, not a
// refactor). Keep `Engine` duck-typed on `.toTree()`/`.backendId`/`.toJson()`
// (it is) so it tolerates handles from any core instance. The `runtime.test.js`
// "re-exports the internal core build classes verbatim" case
// (`Quill === CoreQuill`) is the executable guard for this invariant.
export { Quill, Document, init } from '../core/wasm.js';

/**
 * Narrow an unknown caught value to a `QuillmarkError` — the error every
 * fallible method in this package throws: a real `Error` with a non-empty
 * `diagnostics` array attached (same entry shape as `RenderResult.warnings`).
 *
 * Structural by necessity AND by design: the WASM layer constructs a plain
 * `Error` and attaches the property (there is no error class to `instanceof`),
 * and a structural check works on errors from any build or WASM instance in
 * the page — consistent with the duck-typed handling of handles elsewhere in
 * this layer.
 *
 * @param {unknown} e
 * @returns {e is Error & { diagnostics: import('../core/wasm.js').Diagnostic[] }}
 */
export function isQuillmarkError(e) {
	return e instanceof Error && Array.isArray(/** @type {any} */ (e).diagnostics);
}

// Backend builds are NEVER statically imported here — that would pull a
// multi-MB binary into the eager graph and defeat lazy loading. Each entry is a
// DESCRIPTOR: `load` is a thunk returning a dynamic `import()` (a backend's
// chunk is fetched only when something actually renders against that backend),
// and `formats`/`canvas` are the REQUIRED static capability manifest so the
// cheap probes (`supportedFormats`/`supportsCanvas`) ALWAYS answer without
// loading the binary or cloning the quill. The manifest values are verified
// against each backend's Rust source (`crates/backends/<id>/src/lib.rs`
// `SUPPORTED_FORMATS`) and pinned by the `runtime.test.js` drift-guard test,
// which renders once and asserts the loaded backend reports the same list.
// `canvas` mirrors `quillmark_core::formats_support_canvas`: true iff the
// format list includes a visual-page format (`svg` or `png`).
const DEFAULT_BACKENDS = {
	typst: {
		load: () => import('../backends/typst/wasm.js'),
		formats: ['pdf', 'svg', 'png'], // crates/backends/typst/src/lib.rs SUPPORTED_FORMATS
		canvas: true // has svg/png → formats_support_canvas == true
	},
	pdfform: {
		load: () => import('../backends/pdfform/wasm.js'),
		// crates/backends/pdfform/src/lib.rs SUPPORTED_FORMATS == [Pdf, Svg, Png]
		formats: ['pdf', 'svg', 'png'],
		canvas: true // has svg/png → formats_support_canvas == true
	}
};

/**
 * Validate a backend registry descriptor, throwing a clear error naming the
 * backend id on any malformed entry. Descriptors are the ONLY accepted form:
 * `{ load, formats, canvas }` with a callable `load`, a `formats` array, and a
 * boolean `canvas`. Failing at construction (not deep inside a render) keeps the
 * capability probes free — they can answer from the manifest unconditionally.
 * @param {string} id
 * @param {unknown} entry
 * @returns {{ load: () => Promise<unknown>, formats: string[], canvas: boolean }}
 */
function validateBackend(id, entry) {
	if (!entry || typeof entry !== 'object') {
		throw new Error(
			`Engine: backend '${id}' must be a descriptor { load, formats, canvas }.`
		);
	}
	const { load, formats, canvas } = /** @type {any} */ (entry);
	if (typeof load !== 'function') {
		throw new Error(`Engine: backend '${id}' descriptor needs a callable 'load'.`);
	}
	if (!Array.isArray(formats)) {
		throw new Error(`Engine: backend '${id}' descriptor needs a 'formats' array.`);
	}
	if (typeof canvas !== 'boolean') {
		throw new Error(`Engine: backend '${id}' descriptor needs a boolean 'canvas'.`);
	}
	return { load, formats, canvas };
}

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
	/** backendId → descriptor `{ load, formats, canvas }`. */
	#loaders;
	/**
	 * backendId → WeakMap<canonical Quill, backend-memory Quill clone>. Caches
	 * the expensive quill materialization per (engine, backend, canonical quill
	 * instance). WeakMap so dropping the canonical quill makes its clone
	 * collectable; the backend handle is then freed by wasm-bindgen weak-refs.
	 * @type {Map<string, WeakMap<object, any>>}
	 */
	#quillClones = new Map();

	/**
	 * @param {{ backends?: Record<string, { load: () => Promise<unknown>, formats: string[], canvas: boolean }> }} [options]
	 *   Extra or overriding backend descriptors, merged over the built-ins. Each
	 *   entry is a descriptor (`{ load, formats, canvas }`) with `formats` and
	 *   `canvas` REQUIRED — that static manifest is what makes
	 *   `supportedFormats`/`supportsCanvas` always free (no binary load, no quill
	 *   clone). Malformed entries throw here, at construction. The default
	 *   registry maps `"typst"` to the bundled Typst build.
	 */
	constructor(options) {
		const merged = { ...DEFAULT_BACKENDS, ...(options?.backends ?? {}) };
		/** @type {Record<string, { load: () => Promise<unknown>, formats: string[], canvas: boolean }>} */
		const loaders = {};
		for (const [id, entry] of Object.entries(merged)) {
			loaders[id] = validateBackend(id, entry);
		}
		this.#loaders = loaders;
	}

	/**
	 * Look up the registered descriptor for `backendId`, throwing the canonical
	 * "no backend registered" error if none. Pure — touches no binary.
	 * @param {string} backendId
	 * @returns {{ load: () => Promise<unknown>, formats: string[], canvas: boolean }}
	 */
	#descriptorFor(backendId) {
		const descriptor = this.#loaders[backendId];
		if (!descriptor) {
			throw new Error(
				`Engine: no backend registered for '${backendId}'. ` +
					`Known backends: ${Object.keys(this.#loaders).join(', ') || '(none)'}.`
			);
		}
		return descriptor;
	}

	/**
	 * Resolve (and lazily load) the backend module + its engine for `backendId`.
	 * @param {string} backendId
	 * @returns {Promise<{ mod: any, engine: any }>}
	 */
	async #resolveBackend(backendId) {
		const descriptor = this.#descriptorFor(backendId);

		let modPromise = this.#modules.get(backendId);
		if (!modPromise) {
			// Set the promise synchronously (before any await) so concurrent first
			// renders share ONE import. Self-heal on failure so a transient load
			// error doesn't poison every later attempt.
			modPromise = Promise.resolve()
				.then(descriptor.load)
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
	 * Get (or materialize-and-cache) the backend-memory `Quill` clone for
	 * `quill` under `backendId`. On a cache miss the clone is built from `tree`
	 * — the caller's pre-await `toTree()` snapshot; the canonical handle may be
	 * freed by now — and stored in the per-backend `WeakMap` keyed on the
	 * canonical `Quill` instance, so a later call with the same instance reuses
	 * it (the hot-path fix: no re-serialize / re-copy / re-validate per call).
	 * @param {any} mod the backend build module
	 * @param {string} backendId
	 * @param {object} quill the canonical instance (cache key only)
	 * @param {Map<string, Uint8Array> | null} tree pre-await snapshot; `null` on a cache hit
	 * @returns {any} the backend-memory quill clone
	 */
	#cachedQuillClone(mod, backendId, quill, tree) {
		let perQuill = this.#quillClones.get(backendId);
		if (!perQuill) {
			perQuill = new WeakMap();
			this.#quillClones.set(backendId, perQuill);
		}
		let backendQuill = perQuill.get(quill);
		if (!backendQuill) {
			backendQuill = mod.Quill.fromTree(tree);
			perQuill.set(quill, backendQuill);
		}
		return backendQuill;
	}

	/**
	 * Materialize the backend-memory clones for `quill` + `doc` in `backendId`'s
	 * memory and run `fn` against the backend engine. Only `render`/`open` call
	 * this, so `doc` is always present.
	 *
	 * OWNERSHIP WINDOW: both caller handles are snapshotted (`doc.toJson()`, and
	 * `quill.toTree()` on a clone-cache miss) BEFORE the first await. The backend
	 * load below is a real suspension point — a multi-MB `import()` on first
	 * render — so reading the handles after it would race a caller that
	 * `free()`s them as soon as this call returns its promise ("null pointer
	 * passed to rust"). The snapshot makes that natural calling pattern correct.
	 *
	 * Clone lifetimes differ by design: the `doc` clone is TRANSIENT — freed in
	 * the `finally` of every call. The `quill` clone is CACHED per (engine,
	 * backend, canonical quill instance) and is NOT freed here; a `Quill`
	 * instance's contents never change after construction, so it is dropped with
	 * the canonical quill (WeakMap collection → wasm-bindgen weak-ref free) when
	 * the consumer replaces the instance. A cache miss materializes it once;
	 * subsequent calls reuse it.
	 * @param {string} backendId
	 * @param {{ toTree(): Map<string, Uint8Array> }} quill
	 * @param {{ toJson(): string }} doc
	 * @param {(ctx: { mod: any, engine: any, quill: any, doc: any }) => any} fn
	 */
	async #withClones(backendId, quill, doc, fn) {
		const docJson = doc.toJson();
		const quillTree = this.#quillClones.get(backendId)?.has(quill) ? null : quill.toTree();
		const { mod, engine } = await this.#resolveBackend(backendId);
		// The quill clone is cached (see #cachedQuillClone); only the per-call doc
		// clone is transient. Bring the doc clone + `fn` under one try so the doc
		// clone is freed even if a later step throws (e.g. `Document.fromJson`
		// rejecting a cross-version DTO). The cached quill clone is intentionally
		// NOT freed here. `fn` MUST be synchronous — the doc clone is freed as soon
		// as it returns, so an async `fn` would have it freed mid-flight.
		const backendQuill = this.#cachedQuillClone(mod, backendId, quill, quillTree);
		let backendDoc = null;
		try {
			backendDoc = mod.Document.fromJson(docJson);
			return fn({ mod, engine, quill: backendQuill, doc: backendDoc });
		} finally {
			backendDoc?.free();
		}
	}

	/**
	 * Render `doc` against `quill` in one shot, returning a `RenderResult`.
	 * Both handles are read synchronously before the first await, so the caller
	 * may `free()` them as soon as this call returns.
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @param {object} [options] render options (`{ format, ppi, pages, producer }`)
	 * @returns {Promise<import('./runtime.js').RenderResult>}
	 */
	async render(quill, doc, options) {
		return this.#withClones(quill.backendId, quill, doc, ({ engine, quill: q, doc: d }) =>
			engine.render(q, d, options ?? undefined)
		);
	}

	/**
	 * Open a live render session (canvas preview / per-page paint / `apply`).
	 * The session is self-contained (it retains what it needs for `apply`), so
	 * the transient quill and document clones are freed before this returns;
	 * the caller owns the returned session and must `.free()` it. The `quill`
	 * and `doc` handles are read synchronously before the first await, so the
	 * caller may `free()` them as soon as this call returns.
	 * @experimental Ships ahead of its first production consumer (the designed
	 * canvas live-preview path); the session/paint surface may change in any
	 * 0.x release. `render()` is the stable path.
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @returns {Promise<LiveSession>}
	 */
	async open(quill, doc) {
		return this.#withClones(
			quill.backendId,
			quill,
			doc,
			({ mod, engine, quill: q, doc: d }) => new LiveSession(engine.open(q, d), mod)
		);
	}

	/**
	 * The output formats `quill`'s backend can emit. A cheap, non-failing,
	 * ALWAYS-free pre-render probe: it answers from the descriptor's required
	 * `formats` manifest — NO binary load and NO quill clone — depending only on
	 * `quill.backendId`. Stays `async` for API stability (it never awaits a load).
	 * @param {Quill} quill
	 * @returns {Promise<import('./runtime.js').OutputFormat[]>}
	 */
	async supportedFormats(quill) {
		const descriptor = this.#descriptorFor(quill.backendId);
		// Defensive copy so callers can't mutate the shared manifest.
		return descriptor.formats.slice();
	}

	/**
	 * Whether `quill`'s backend can paint sessions to a canvas. Same ALWAYS-free
	 * probe as `supportedFormats`: answered from the descriptor's required
	 * `canvas` manifest, no load and no clone.
	 * @param {Quill} quill
	 * @returns {Promise<boolean>}
	 */
	async supportsCanvas(quill) {
		const descriptor = this.#descriptorFor(quill.backendId);
		return descriptor.canvas;
	}
}

/**
 * Thin wrapper over a backend's live render session. Reads serve the current
 * compile; `apply(doc)` recompiles in place (transactional: on throw, reads
 * keep serving the last-good compile). The quill/document clones it was
 * opened from have already been freed — the session retains what `apply`
 * needs.
 *
 * `paint` writes a COMPLETE page raster — all content visible, no caller-side
 * compositing — for every backend that supports canvas (Typst rasterizes
 * natively; pdfform rasterizes its pre-flattened page). See `runtime.d.ts`.
 */
export class LiveSession {
	/**
	 * @param {{ pageCount: number, backendId: string, supportsCanvas: boolean, warnings: any[], apply: Function, render: Function, regions: Function, pageSize: Function, paint: Function, free: Function }} inner backend-build LiveSession (typst or pdfform)
	 * @param {{ Document: { fromJson(json: string): any } }} mod the session's backend build, used to materialize `apply` documents in its linear memory
	 */
	constructor(inner, mod) {
		this.#inner = inner;
		this.#mod = mod;
	}
	#inner;
	#mod;

	/**
	 * Recompile the session against `doc` — the edit verb of a live preview.
	 * Transactional: on throw every read keeps serving the last-good compile.
	 * On success reads serve the new compile; repaint `dirtyPages ∩ visible`.
	 * @param {Document} doc
	 * @returns {import('./runtime.d.ts').ChangeSet}
	 */
	apply(doc) {
		let backendDoc = null;
		try {
			backendDoc = this.#mod.Document.fromJson(doc.toJson());
			return this.#inner.apply(backendDoc);
		} finally {
			backendDoc?.free();
		}
	}

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

	/**
	 * Schema-field geometry for this compiled session — one region per
	 * schema-bound field, keyed on its quill schema field path. A session-level
	 * query (no render); read it to place field overlays / cross-navigation over
	 * a `paint`-ed canvas.
	 * @returns {import('./runtime.d.ts').FieldRegion[]}
	 */
	regions() {
		return this.#inner.regions();
	}

	/** @param {number} page */
	pageSize(page) {
		return this.#inner.pageSize(page);
	}

	/**
	 * Paint `page` into a 2D canvas context. The painted raster is COMPLETE —
	 * all page content visible, no caller-side compositing — for both the Typst
	 * and pdfform backends. See `runtime.d.ts` for the DPR/clamp math and the
	 * region-overlay coordinate transform.
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
