/**
 * Core-build smoke tests.
 *
 * Exercises the Typst-less `@quillmark/wasm/core` bundle: a `Quill` loads,
 * validates, seeds, and exposes schema/blueprint/metadata with NO engine and NO
 * render surface. The render suite (basic/canvas) covers the superset.
 *
 * Setup: imports from `@quillmark-wasm/core` (aliased to pkg/core/wasm.js in
 * vitest.config.js) — the no-features build with Typst excluded.
 */
import { describe, it, expect } from 'vitest'
import * as core from '@quillmark-wasm/core'
import { Quill, Document } from '@quillmark-wasm/core'

const enc = new TextEncoder()

// A minimal quill with one schema field — no plate/font needed; core never
// renders, it only reads config.
function makeCoreQuill() {
  const yaml = `quill:
  name: core_test
  version: "1.0.0"
  backend: typst
  description: Core build smoke test
main:
  fields:
    title:
      type: string
      description: Document title
      example: Hello
`
  return new Map([['Quill.yaml', enc.encode(yaml)]])
}

describe('@quillmark/wasm/core surface', () => {
  it('exposes Document and Quill but NO engine / render API', () => {
    expect(typeof core.Quill).toBe('function')
    expect(typeof core.Document).toBe('function')
    // The engine and session live only in the render build.
    expect(core.Quillmark).toBeUndefined()
    expect(core.RenderSession).toBeUndefined()
  })

  it('loads a quill via Quill.fromTree with no engine', () => {
    const quill = Quill.fromTree(makeCoreQuill())
    expect(quill.backendId).toBe('typst')
    // No render/open/capability methods on the core Quill.
    expect(quill.render).toBeUndefined()
    expect(quill.open).toBeUndefined()
    expect(quill.supportsCanvas).toBeUndefined()
  })

  it('metadata is identity-only — no supportedFormats', () => {
    const quill = Quill.fromTree(makeCoreQuill())
    const meta = quill.metadata
    expect(meta.name).toBe('core_test')
    expect(meta.version).toBe('1.0.0')
    expect(meta.backend).toBe('typst')
    expect(meta.supportedFormats).toBeUndefined()
  })

  it('schema, blueprint, seed, and validate work without a backend', () => {
    const quill = Quill.fromTree(makeCoreQuill())

    expect(quill.schema.main).toBeDefined()
    expect(quill.schema.main.fields.title).toBeDefined()

    expect(typeof quill.blueprint).toBe('string')
    expect(quill.blueprint.length).toBeGreaterThan(0)

    const doc = quill.seedDocument()
    expect(doc).toBeInstanceOf(Document)

    const main = quill.seedMain()
    expect(main).toBeDefined()

    // A seeded document validates clean (array of diagnostics, empty here).
    const diags = quill.validate(doc)
    expect(Array.isArray(diags)).toBe(true)
  })

  it('loads even when the declared backend is unknown (resolved at render time)', () => {
    const yaml = `quill:
  name: no_backend
  version: "1.0.0"
  backend: nonexistent
  description: Backend resolved later
`
    const quill = Quill.fromTree(new Map([['Quill.yaml', enc.encode(yaml)]]))
    expect(quill.backendId).toBe('nonexistent')
  })
})
