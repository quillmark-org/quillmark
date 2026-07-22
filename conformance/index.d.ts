// Type surface for @quillmark/conformance — the frozen contract fixtures and
// the runner over an adapter you supply. See prose/canon/CONFORMANCE.md.

/** One segment of a parsed `DocPath` — mirrors `@quillmark/wasm`'s `DocPathSeg`
 * (inlined so this package carries no type dependency on the engine). */
export type DocPathSeg =
  | { seg: 'main' }
  | { seg: 'card'; kind: string | null; index: number }
  | { seg: 'field'; name: string }
  | { seg: 'index'; index: number }
  | { seg: 'body' }

/** The engine↔consumer contract version the fixtures were frozen against —
 * semver'd over {diagnostic taxonomy, DocPath grammar, fieldStates shape}. Assert
 * your engine's `contractVersion()` equals it before trusting the set. */
export declare const contractVersion: string

/** Named quills the fixtures build against, each a `path -> UTF-8 text` file
 * tree. A conformance quill is `Quill.yaml`-only (the suite never renders). */
export declare const quills: Record<string, Record<string, string>>

/** The operation-script fixtures. */
export declare const fixtures: Fixture[]

/** The `DocPath` grammar fixtures. */
export declare const paths: PathFixture[]

// ── fixture format ──────────────────────────────────────────────────────────

/** A diagnostic assertion — code, path, severity; never message text. */
export interface ExpectDiag {
  severity: 'error' | 'warning'
  code: string
  path?: string
}

/** A mutator-failure assertion on a step. */
export interface ExpectError {
  code: string
  path?: string
}

/** One resolved row: the `source` rung (always) and, when deterministic, the
 * exact `value`. */
export interface ExpectFieldState {
  source: 'authored' | 'default' | 'zero'
  value?: unknown
}

/** One card's expected rows: an optional key-`order` assertion (the
 * declaration-order contract) and a per-field `{source, value?}` map. */
export interface ExpectCard {
  order?: string[]
  fields: Record<string, ExpectFieldState>
}

/** A composable card's expected rows, matched to the actual card by `index`;
 * `kind` is `null` for an unknown-kind card. */
export interface ExpectCardAt extends ExpectCard {
  index: number
  kind?: string | null
}

/** The resolved-value expectations for a fixture. */
export interface ExpectFieldStates {
  main?: ExpectCard
  cards?: ExpectCardAt[]
}

/** One operation-script step, keyed by `op` (the WASM `Document` verb name).
 * `card` absent targets the main card. An `error` asserts the op fails with that
 * `{code, path}`; its absence asserts success. */
export interface Step {
  op: 'storeField' | 'storeFill' | 'removeField' | 'commitField' | 'insertCard'
  card?: number
  field?: string
  value?: unknown
  kind?: string
  index?: number
  body?: string
  error?: ExpectError
}

/** One state fixture: parse `document` against `quill`, replay `steps`, then
 * assert `validate` and `fieldStates`. */
export interface Fixture {
  name: string
  description?: string
  quill: string
  document: string
  steps?: Step[]
  validate?: ExpectDiag[]
  fieldStates?: ExpectFieldStates
}

/** A grammar fixture: a `DocPath` string and the segments it parses to. `plate`
 * carries the plate-space geometry form the engine translates it from — an
 * engine-side seam a consumer does not run (it reads already-translated
 * addresses). */
export interface PathFixture {
  name: string
  path: string
  segs: DocPathSeg[]
  plate?: { addr: string; cardKinds: (string | null)[] }
}

// ── adapter ─────────────────────────────────────────────────────────────────

/** A diagnostic as the engine surfaces it — the shape `validate()` returns and a
 * thrown mutator error carries on `.diagnostics[0]`. */
export interface Diagnostic {
  severity: 'error' | 'warning'
  code?: string
  path?: string
  message?: string
}

/** The integration layer the runner drives. A `@quillmark/wasm` adapter maps
 * each verb to the identically-named `Document` method; `Quill` / `Doc` are
 * whatever handle types that layer uses. A mutator verb throws the engine's
 * diagnostic-bearing error on failure. */
export interface ConformanceAdapter<Quill = unknown, Doc = unknown> {
  /** The engine's reported contract version (`contractVersion()`). */
  contractVersion(): string
  /** Build a quill from a `path -> text` file tree. */
  buildQuill(files: Record<string, string>): Quill
  /** Parse a document from Quillmark markdown. */
  parseDocument(markdown: string): Doc
  storeField(doc: Doc, card: number | null, field: string, value: unknown): void
  storeFill(doc: Doc, card: number | null, field: string, value: unknown): void
  removeField(doc: Doc, card: number | null, field: string): void
  commitField(quill: Quill, doc: Doc, card: number | null, field: string, value: unknown): void
  insertCard(doc: Doc, kind: string, index: number | null, body: string | null): void
  validate(quill: Quill, doc: Doc): Diagnostic[]
  fieldStates(quill: Quill, doc: Doc): {
    main: { fields: Record<string, { value: unknown; source: string }> }
    cards: { kind: string | null; index: number; fields: Record<string, { value: unknown; source: string }> }[]
  }
  parseDocPath(path: string): DocPathSeg[]
  formatDocPath(segs: DocPathSeg[]): string
  /** Extract `{code, path}` from a thrown mutator error. Defaults to reading
   * `err.diagnostics[0]` (the WASM error shape); override for another surface. */
  errorDiag?(err: unknown): { code?: string; path?: string }
}

/** Structural equality — primitives, arrays (order-sensitive), plain objects
 * (key-order-independent). */
export declare function deepEqual(a: unknown, b: unknown): boolean

/** Run one state fixture; throws on mismatch. Pass a prebuilt `quill` to reuse
 * one across fixtures; omit it to build the fixture's own. */
export declare function runFixture(adapter: ConformanceAdapter, fx: Fixture, quill?: unknown): void

/** Run one grammar fixture; throws on mismatch. */
export declare function runPath(adapter: ConformanceAdapter, fx: PathFixture): void

/** Run the whole suite: assert the engine's `contractVersion`, then every
 * fixture. Returns `{ total }` on success; throws an aggregate error naming
 * every failure otherwise. */
export declare function runConformance(adapter: ConformanceAdapter): { total: number }
