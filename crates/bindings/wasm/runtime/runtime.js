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
	 * `quill` under `backendId`. On a cache miss the clone is built via
	 * `toTree`→`fromTree` and stored in the per-backend `WeakMap` keyed on the
	 * canonical `Quill` instance, so a later call with the same instance reuses
	 * it (the hot-path fix: no re-serialize / re-copy / re-validate per call).
	 * @param {any} mod the backend build module
	 * @param {string} backendId
	 * @param {{ toTree(): Map<string, Uint8Array> }} quill
	 * @returns {any} the backend-memory quill clone
	 */
	#cachedQuillClone(mod, backendId, quill) {
		let perQuill = this.#quillClones.get(backendId);
		if (!perQuill) {
			perQuill = new WeakMap();
			this.#quillClones.set(backendId, perQuill);
		}
		let backendQuill = perQuill.get(quill);
		if (!backendQuill) {
			backendQuill = mod.Quill.fromTree(quill.toTree());
			perQuill.set(quill, backendQuill);
		}
		return backendQuill;
	}

	/**
	 * Materialize the backend-memory clones for `quill` + `doc` in `backendId`'s
	 * memory and run `fn` against the backend engine. Only `render`/`open` call
	 * this, so `doc` is always present.
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
		const { mod, engine } = await this.#resolveBackend(backendId);
		// The quill clone is cached (see #cachedQuillClone); only the per-call doc
		// clone is transient. Bring the doc clone + `fn` under one try so the doc
		// clone is freed even if a later step throws (e.g. `Document.fromJson`
		// rejecting a cross-version DTO). The cached quill clone is intentionally
		// NOT freed here. `fn` MUST be synchronous — the doc clone is freed as soon
		// as it returns, so an async `fn` would have it freed mid-flight.
		const backendQuill = this.#cachedQuillClone(mod, backendId, quill);
		let backendDoc = null;
		try {
			backendDoc = mod.Document.fromJson(doc.toJson());
			return fn({ mod, engine, quill: backendQuill, doc: backendDoc });
		} finally {
			backendDoc?.free();
		}
	}

	/**
	 * Render `doc` against `quill` in one shot, returning a `RenderResult`.
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
	 * Open an iterative render session (for canvas preview / per-page paint).
	 * The session is a self-contained compiled snapshot, so the transient quill
	 * and document clones are freed before this returns; the caller owns the
	 * returned session and must `.free()` it.
	 * @experimental Ships ahead of its first production consumer (the designed
	 * canvas live-preview path); the session/paint surface may change in any
	 * 0.x release. `render()` is the stable path.
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
 * Thin wrapper over a backend's iterative render session. Holds the compiled
 * snapshot; the quill/document it was opened from have already been freed.
 *
 * `paint` writes a COMPLETE page raster — all content visible, no caller-side
 * compositing — for every backend that supports canvas (Typst rasterizes
 * natively; pdfform rasterizes its pre-flattened page). See `runtime.d.ts`.
 */
export class RenderSession {
	/** @param {{ pageCount: number, backendId: string, supportsCanvas: boolean, warnings: any[], render: Function, regions: Function, pageSize: Function, paint: Function, free: Function }} inner backend-build RenderSession (typst or pdfform) */
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

/**
 * Throw unless `s` is a positive, finite scale. Mirrors `paint`'s scale
 * validation so device-pixel overlay geometry can't be computed against a bad
 * `renderScale` (NaN/0/∞ would yield nonsense boxes).
 * @param {number} s
 */
function assertRenderScale(s) {
	if (!(Number.isFinite(s) && s > 0)) {
		throw new Error('RegionMap: renderScale must be a positive, finite number.');
	}
}

/**
 * Per-page projection of `RenderSession.regions()` into draw-ready overlay
 * geometry and a click hit-test — the canonical region coordinate transform
 * (Y-flip, bottom-left→top-left origin, pt↔device-px), encoded once so consumers
 * don't re-derive it. Pure data: no WASM, no DOM, no session reference. See
 * `runtime.d.ts` for the consumer recipe and the coordinate contract.
 */
export class RegionMap {
	/** @type {number} */ #page;
	/** @type {import('./runtime.d.ts').PageSize} */ #pageSize;
	/** @type {import('./runtime.d.ts').FieldRegion[]} this page's regions, in order */ #regions;
	/** @type {Map<string, import('./runtime.d.ts').FieldRegion>} field path → region; first wins on a dup path */ #byField;

	/**
	 * Internal. Use {@link RegionMap.from}. `regions` is already filtered to `page`.
	 * @param {number} page
	 * @param {import('./runtime.d.ts').PageSize} pageSize
	 * @param {import('./runtime.d.ts').FieldRegion[]} regions
	 */
	constructor(page, pageSize, regions) {
		this.#page = page;
		this.#pageSize = pageSize;
		this.#regions = regions;
		this.#byField = new Map();
		for (const r of regions) if (!this.#byField.has(r.field)) this.#byField.set(r.field, r);
	}

	/**
	 * Build the map for `page` from a session's full region list and that page's
	 * size. Regions not on `page` are dropped.
	 * @param {import('./runtime.d.ts').FieldRegion[]} regions
	 * @param {import('./runtime.d.ts').PageSize} pageSize
	 * @param {number} page
	 * @returns {RegionMap}
	 */
	static from(regions, pageSize, page) {
		const w = pageSize?.widthPt;
		const h = pageSize?.heightPt;
		if (!(Number.isFinite(w) && w > 0 && Number.isFinite(h) && h > 0)) {
			throw new Error('RegionMap: pageSize must have positive, finite widthPt and heightPt.');
		}
		return new RegionMap(
			page,
			pageSize,
			regions.filter((r) => r.page === page)
		);
	}

	get page() {
		return this.#page;
	}
	get pageSize() {
		return this.#pageSize;
	}
	get fields() {
		return this.#regions.map((r) => r.field);
	}

	/**
	 * Percent-of-page overlay box for one region — top-left origin, Y flipped.
	 * @param {import('./runtime.d.ts').FieldRegion} region
	 * @returns {import('./runtime.d.ts').OverlayBox}
	 */
	#percent({ rect: [x0, y0, x1, y1] }) {
		const { widthPt: w, heightPt: h } = this.#pageSize;
		return {
			left: (x0 / w) * 100,
			top: ((h - y1) / h) * 100, // flip Y: bottom-left PDF origin → top-left page origin
			width: ((x1 - x0) / w) * 100,
			height: ((y1 - y0) / h) * 100
		};
	}

	/**
	 * Device-pixel overlay box for one region at `s` — top-left origin, Y flipped.
	 * @param {import('./runtime.d.ts').FieldRegion} region
	 * @param {number} s renderScale
	 * @returns {import('./runtime.d.ts').OverlayBox}
	 */
	#device({ rect: [x0, y0, x1, y1] }, s) {
		const h = this.#pageSize.heightPt;
		return {
			left: x0 * s,
			top: (h - y1) * s, // flip Y
			width: (x1 - x0) * s,
			height: (y1 - y0) * s
		};
	}

	/** @param {string} field */
	region(field) {
		return this.#byField.get(field);
	}

	/**
	 * @param {number} xPercent
	 * @param {number} yPercent
	 * @returns {import('./runtime.d.ts').FieldRegion | undefined}
	 */
	at(xPercent, yPercent) {
		let best;
		let bestArea = Infinity;
		for (const r of this.#regions) {
			const b = this.#percent(r);
			// Inclusive on all edges; a non-finite coord fails every comparison → no match.
			if (
				xPercent >= b.left &&
				xPercent <= b.left + b.width &&
				yPercent >= b.top &&
				yPercent <= b.top + b.height
			) {
				const area = b.width * b.height;
				if (area < bestArea) {
					best = r;
					bestArea = area;
				}
			}
		}
		return best;
	}

	/** @param {string} field */
	overlayPercent(field) {
		const r = this.#byField.get(field);
		return r ? this.#percent(r) : undefined;
	}

	/**
	 * @param {string} field
	 * @param {number} renderScale
	 */
	overlayDevice(field, renderScale) {
		assertRenderScale(renderScale);
		const r = this.#byField.get(field);
		return r ? this.#device(r, renderScale) : undefined;
	}

	/** @returns {import('./runtime.d.ts').FieldOverlay[]} */
	overlaysPercent() {
		return this.#regions.map((r) => ({ field: r.field, box: this.#percent(r) }));
	}

	/**
	 * @param {number} renderScale
	 * @returns {import('./runtime.d.ts').FieldOverlay[]}
	 */
	overlaysDevice(renderScale) {
		assertRenderScale(renderScale);
		return this.#regions.map((r) => ({ field: r.field, box: this.#device(r, renderScale) }));
	}
}
