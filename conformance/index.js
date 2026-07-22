// @quillmark/conformance — the frozen engine↔consumer contract fixtures, and a
// runner that replays them against an integration layer you supply (the
// "adapter"). The engine repo runs the identical `conformance.json` against
// `quillmark-core` (crates/conformance); a consumer runs it here against
// `@quillmark/wasm`. One frozen set, both repos green.
//
// A fixture asserts a diagnostic's `code`, `path`, and `severity` — never its
// `message`. See prose/canon/CONFORMANCE.md and ./index.d.ts.

import suite from './conformance.json' with { type: 'json' }

export const { contractVersion, quills, fixtures, paths } = suite

// ── value comparison ────────────────────────────────────────────────────────

/** Structural equality — primitives, arrays (order-sensitive), plain objects
 * (key-order-independent). The one comparison every value/segment assertion
 * shares. */
export function deepEqual(a, b) {
  if (a === b) return true
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false
  if (Array.isArray(a) !== Array.isArray(b)) return false
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false
    return a.every((x, i) => deepEqual(x, b[i]))
  }
  const ka = Object.keys(a)
  const kb = Object.keys(b)
  if (ka.length !== kb.length) return false
  return ka.every((k) => Object.prototype.hasOwnProperty.call(b, k) && deepEqual(a[k], b[k]))
}

function show(v) {
  return typeof v === 'string' ? v : JSON.stringify(v)
}

class ConformanceError extends Error {}

function fail(fixture, message) {
  throw new ConformanceError(`${fixture}: ${message}`)
}

// ── operation dispatch ──────────────────────────────────────────────────────

/** Apply one step through the adapter. `card` is a card index or null (main).
 * Returns nothing; a mutator failure throws (the adapter surfaces the engine's
 * diagnostic-bearing error). */
function applyStep(adapter, quill, doc, step) {
  const card = step.card ?? null
  switch (step.op) {
    case 'storeField': return adapter.storeField(doc, card, step.field, step.value)
    case 'storeFill': return adapter.storeFill(doc, card, step.field, step.value)
    case 'removeField': return adapter.removeField(doc, card, step.field)
    case 'commitField': return adapter.commitField(quill, doc, card, step.field, step.value)
    case 'insertCard': return adapter.insertCard(doc, step.kind, step.index ?? null, step.body ?? null)
    default: throw new ConformanceError(`unknown op \`${step.op}\``)
  }
}

/** The `{ code, path }` of a thrown mutator error. The WASM binding attaches a
 * `.diagnostics` array (ERROR.md); an adapter over a different surface overrides
 * `adapter.errorDiag`. */
function errorDiag(adapter, err) {
  if (adapter.errorDiag) return adapter.errorDiag(err)
  const d = err?.diagnostics?.[0] ?? {}
  return { code: d.code, path: d.path }
}

// ── expectation checks ──────────────────────────────────────────────────────

function checkSteps(adapter, quill, doc, fx) {
  fx.steps?.forEach((step, i) => {
    const at = `${fx.name}: step ${i} (\`${step.op}\`)`
    let error = null
    try {
      applyStep(adapter, quill, doc, step)
    } catch (e) {
      error = e
    }
    if (step.error) {
      if (!error) fail(at, `expected error ${step.error.code}, got success`)
      const got = errorDiag(adapter, error)
      if (got.code !== step.error.code) fail(at, `error code: expected ${step.error.code}, got ${got.code}`)
      const wantPath = step.error.path ?? undefined
      const gotPath = got.path ?? undefined
      if (gotPath !== wantPath) fail(at, `error path: expected ${show(wantPath)}, got ${show(gotPath)}`)
    } else if (error) {
      throw error
    }
  })
}

function checkCard(actual, expect, at) {
  const fields = actual?.fields ?? {}
  if (expect.order) {
    const keys = Object.keys(fields)
    if (!deepEqual(keys, expect.order)) {
      fail(at, `field order (declaration-order contract): expected ${show(expect.order)}, got ${show(keys)}`)
    }
  }
  for (const [name, exp] of Object.entries(expect.fields ?? {})) {
    const row = fields[name]
    if (!row) fail(at, `missing row \`${name}\``)
    if (row.source !== exp.source) fail(at, `\`${name}\` source: expected ${exp.source}, got ${row.source}`)
    if ('value' in exp && !deepEqual(row.value, exp.value)) {
      fail(at, `\`${name}\` value: expected ${show(exp.value)}, got ${show(row.value)}`)
    }
  }
}

function checkValidate(adapter, quill, doc, fx) {
  const key = (d) => `${d.severity}|${d.code}|${d.path ?? ''}`
  const actual = adapter.validate(quill, doc).map(key).sort()
  const want = fx.validate.map(key).sort()
  if (!deepEqual(actual, want)) {
    fail(fx.name, `validate() diagnostics: expected ${show(want)}, got ${show(actual)}`)
  }
}

function checkFieldStates(adapter, quill, doc, fx) {
  const states = adapter.fieldStates(quill, doc)
  const expect = fx.fieldStates
  if (expect.main) checkCard(states.main, expect.main, `${fx.name}: main`)
  for (const exp of expect.cards ?? []) {
    const card = states.cards.find((c) => c.index === exp.index)
    if (!card) fail(fx.name, `no card at index ${exp.index}`)
    if ('kind' in exp && !deepEqual(card.kind, exp.kind)) {
      fail(fx.name, `card[${exp.index}] kind: expected ${show(exp.kind)}, got ${show(card.kind)}`)
    }
    checkCard(card, exp, `${fx.name}: card[${exp.index}]`)
  }
}

// ── public runner ───────────────────────────────────────────────────────────

/** Run one state fixture end to end; throws a `ConformanceError` on mismatch.
 * Pass a prebuilt `quill` to reuse one across fixtures; omit it to build the
 * fixture's own. */
export function runFixture(adapter, fx, quill = adapter.buildQuill(quills[fx.quill])) {
  const doc = adapter.parseDocument(fx.document)
  checkSteps(adapter, quill, doc, fx)
  if (fx.validate) checkValidate(adapter, quill, doc, fx)
  if (fx.fieldStates) checkFieldStates(adapter, quill, doc, fx)
}

/** Run one grammar fixture: `parseDocPath` matches the expected segments and
 * `formatDocPath` round-trips. The plate-space translation is engine-side (a
 * consumer reads already-translated addresses), so it is not asserted here. */
export function runPath(adapter, fx) {
  const segs = adapter.parseDocPath(fx.path)
  if (!deepEqual(segs, fx.segs)) {
    fail(fx.name, `parseDocPath: expected ${show(fx.segs)}, got ${show(segs)}`)
  }
  const round = adapter.formatDocPath(fx.segs)
  if (round !== fx.path) fail(fx.name, `formatDocPath round-trip: expected ${fx.path}, got ${round}`)
}

/** Run the whole suite. Asserts the adapter's engine reports the frozen
 * `contractVersion`, then every fixture. Returns `{ total }` on success; throws
 * an aggregate error naming every failure otherwise. */
export function runConformance(adapter) {
  const failures = []
  const engineVersion = adapter.contractVersion()
  if (engineVersion !== contractVersion) {
    failures.push(`contractVersion: engine reports ${engineVersion}, fixtures frozen at ${contractVersion}`)
  }
  // Build each distinct quill once — many fixtures share one.
  const built = {}
  for (const fx of fixtures) {
    try {
      built[fx.quill] ??= adapter.buildQuill(quills[fx.quill])
      runFixture(adapter, fx, built[fx.quill])
    } catch (e) {
      failures.push(e.message)
    }
  }
  for (const fx of paths) {
    try {
      runPath(adapter, fx)
    } catch (e) {
      failures.push(e.message)
    }
  }
  if (failures.length) {
    throw new ConformanceError(`${failures.length} conformance failure(s):\n${failures.join('\n')}`)
  }
  return { total: fixtures.length + paths.length }
}
