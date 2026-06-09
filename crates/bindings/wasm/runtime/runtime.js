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
//     a `Quill` instance's contents are assumed never to change after
//     construction — mutate by replacing the instance, or call `engine.invalidate
//     (quill)` / `engine.invalidateAll()` to evict and free a stale clone.
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
// DESCRIPTOR: `load` is a thunk returning a dynamic `import()` (a backend's
// chunk is fetched only when something actually renders against that backend),
// and `formats`/`canvas` are the STATIC capability manifest so the cheap probes
// (`supportedFormats`/`supportsCanvas`) answer without loading the binary or
// cloning the quill. The manifest values are verified against the backend's
// Rust source (`crates/backends/typst/src/lib.rs` `SUPPORTED_FORMATS` and
// `supports_canvas`) and pinned by the `runtime.test.js` drift-guard test, which
// renders once and asserts the loaded backend reports the same list.
const DEFAULT_BACKENDS = {
	typst: {
		load: () => import('../backends/typst/wasm.js'),
		formats: ['pdf', 'svg', 'png'], // crates/backends/typst/src/lib.rs SUPPORTED_FORMATS
		canvas: true // crates/backends/typst/src/lib.rs supports_canvas() == true
	}
};

/**
 * Normalize a backend registry entry into a descriptor. Accepts BOTH forms:
 *   - bare thunk: `() => import(...)` — a loader with NO static manifest. The
 *     cheap probes cannot answer from it, so they fall back to the load+clone
 *     `#withClones` route (see `supportedFormats`/`supportsCanvas`).
 *   - descriptor: `{ load, formats?, canvas? }` — `load` is the thunk; when
 *     `formats`/`canvas` are present, the probes answer from them with no load
 *     and no clone.
 * @param {(() => Promise<unknown>) | { load: () => Promise<unknown>, formats?: string[], canvas?: boolean }} entry
 * @returns {{ load: () => Promise<unknown>, formats?: string[], canvas?: boolean }}
 */
function normalizeBackend(entry) {
	return typeof entry === 'function' ? { load: entry } : entry;
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
	/** backendId → descriptor `{ load, formats?, canvas? }`. */
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
	 * @param {{ backends?: Record<string, (() => Promise<unknown>) | { load: () => Promise<unknown>, formats?: string[], canvas?: boolean }> }} [options]
	 *   Extra or overriding backend loaders, merged over the built-ins. Each
	 *   entry is either a bare thunk (`() => import(...)`, no static manifest) or
	 *   a descriptor (`{ load, formats?, canvas? }`). Descriptor-form entries make
	 *   `supportedFormats`/`supportsCanvas` free — no binary load, no quill clone.
	 *   The default registry maps `"typst"` to the bundled Typst build (descriptor
	 *   form). Bare thunks keep working but their probes fall back to load+clone.
	 */
	constructor(options) {
		const merged = { ...DEFAULT_BACKENDS, ...(options?.backends ?? {}) };
		/** @type {Record<string, { load: () => Promise<unknown>, formats?: string[], canvas?: boolean }>} */
		const loaders = {};
		for (const [id, entry] of Object.entries(merged)) {
			loaders[id] = normalizeBackend(entry);
		}
		this.#loaders = loaders;
	}

	/**
	 * Look up the registered descriptor for `backendId`, throwing the canonical
	 * "no backend registered" error if none. Pure — touches no binary.
	 * @param {string} backendId
	 * @returns {{ load: () => Promise<unknown>, formats?: string[], canvas?: boolean }}
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
	 * memory and run `fn` against the backend engine. `doc` is optional
	 * (capability probes need only the quill).
	 *
	 * Clone lifetimes differ by design: the `doc` clone is TRANSIENT — freed in
	 * the `finally` of every call. The `quill` clone is CACHED per (engine,
	 * backend, canonical quill instance) and is NOT freed here; it is dropped
	 * with the canonical quill (WeakMap collection → wasm-bindgen weak-ref free)
	 * or explicitly via `invalidate`/`invalidateAll`. A cache miss materializes
	 * it once; subsequent calls reuse it.
	 * @param {string} backendId
	 * @param {{ toTree(): Map<string, Uint8Array> }} quill
	 * @param {{ toJson(): string } | null} doc
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
			backendDoc = doc ? mod.Document.fromJson(doc.toJson()) : null;
			return fn({ mod, engine, quill: backendQuill, doc: backendDoc });
		} finally {
			backendDoc?.free();
		}
	}

	/**
	 * Drop the cached backend-memory clone(s) of `quill` across ALL backends,
	 * freeing each immediately. Escape hatch for the "same `Quill` instance,
	 * republished contents" staleness the caching contract otherwise forbids:
	 * the `Engine` assumes a `Quill` instance never changes after construction,
	 * so mutate-by-replacing-the-instance, or call this after an in-place change.
	 * @param {Quill} quill
	 */
	invalidate(quill) {
		for (const perQuill of this.#quillClones.values()) {
			const backendQuill = perQuill.get(quill);
			if (backendQuill) {
				perQuill.delete(quill);
				backendQuill.free();
			}
		}
	}

	/**
	 * Drop and free every cached backend-memory quill clone in this engine, for
	 * every backend. Coarse counterpart to `invalidate`.
	 */
	invalidateAll() {
		// WeakMaps aren't enumerable, so we can't iterate the cached clones to free
		// them; replacing each map drops our strong refs so the entries (and their
		// backend handles, via wasm-bindgen weak-refs) become collectable.
		this.#quillClones = new Map();
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
	 * The output formats `quill`'s backend can emit. A cheap, non-failing
	 * pre-render probe: it depends only on `quill.backendId`.
	 *
	 * For a descriptor-form backend with a `formats` manifest (the default Typst
	 * entry), this answers directly — NO binary load and NO quill clone. For a
	 * bare-thunk backend (no manifest), it falls back to the load+clone
	 * `#withClones` route, because the backend FFI is `supportedFormats(&Quill)`
	 * (see `crates/bindings/wasm/src/engine.rs`) and so requires a backend-memory
	 * quill to ask. Stays `async` either way; the manifest path just never awaits
	 * a real load.
	 * @param {Quill} quill
	 * @returns {Promise<import('./runtime.js').OutputFormat[]>}
	 */
	async supportedFormats(quill) {
		const descriptor = this.#descriptorFor(quill.backendId);
		if (descriptor.formats) {
			// Defensive copy so callers can't mutate the shared manifest.
			return descriptor.formats.slice();
		}
		return this.#withClones(quill.backendId, quill, null, ({ engine, quill: q }) =>
			engine.supportedFormats(q)
		);
	}

	/**
	 * Whether `quill`'s backend can paint sessions to a canvas. Cheap probe: see
	 * `supportedFormats` — answers from the descriptor's `canvas` manifest when
	 * present (no load, no clone), else falls back to `#withClones` for the same
	 * FFI-shape reason (`supportsCanvas(&Quill)`).
	 * @param {Quill} quill
	 * @returns {Promise<boolean>}
	 */
	async supportsCanvas(quill) {
		const descriptor = this.#descriptorFor(quill.backendId);
		if (descriptor.canvas !== undefined) {
			return descriptor.canvas;
		}
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
	/** @param {import('../backends/typst/wasm').RenderSession} inner */
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
