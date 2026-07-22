// Engine-free smoke check for the @quillmark/conformance package: the module
// loads, its data is well-formed, and the runner's plumbing (dispatch, the
// contractVersion gate, aggregate failure reporting) works. The real
// engine-backed run is the editor's `@quillmark/wasm` adapter and the engine's
// Rust harness (crates/conformance); this only guards the package itself.
//
// Run: `node conformance/smoke.mjs` (exits non-zero on failure).

import assert from 'node:assert/strict'
import { contractVersion, quills, fixtures, paths, deepEqual, runConformance } from './index.js'

// ── data shape ──────────────────────────────────────────────────────────────
assert.match(contractVersion, /^\d+\.\d+\.\d+$/, 'contractVersion is a semver string')
assert.ok(fixtures.length > 0 && paths.length > 0, 'suite is non-empty')

const names = [...fixtures.map((f) => f.name), ...paths.map((p) => p.name)]
assert.equal(new Set(names).size, names.length, 'fixture names are unique')

for (const fx of fixtures) {
  assert.ok(quills[fx.quill], `${fx.name}: references a known quill`)
  assert.equal(typeof fx.document, 'string', `${fx.name}: has a document`)
}
for (const p of paths) {
  assert.ok(typeof p.path === 'string' && Array.isArray(p.segs), `${p.name}: path + segs`)
}

// No fixture asserts message text — the freeze signal. The format has no message
// field, so this checks the invariant is not smuggled in via an unknown key.
const hasKey = (o, key) =>
  o != null && typeof o === 'object' && Object.entries(o).some(([k, v]) => k === key || hasKey(v, key))
assert.ok(!hasKey(fixtures, 'message'), 'no fixture asserts a diagnostic message')

// ── runner plumbing: the contractVersion gate reports a mismatch ─────────────
const stub = new Proxy(
  { contractVersion: () => 'x.y.z' },
  { get: (t, p) => (p in t ? t[p] : () => { throw new Error('unreached') }) },
)
assert.throws(() => runConformance(stub), /contractVersion/, 'a version mismatch is a failure')

assert.ok(deepEqual({ a: [1, 2] }, { a: [1, 2] }) && !deepEqual([1], [1, 2]))

console.log(`ok — ${fixtures.length} fixtures, ${paths.length} paths, contract ${contractVersion}`)
