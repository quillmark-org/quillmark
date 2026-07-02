/**
 * Canonical-API (`@quillmark/wasm/runtime`) integration tests.
 *
 * The runtime layer re-exports the core build's `Quill`/`Document` and adds an
 * `Engine` that hides the core→backend WASM-memory crossing. These tests prove,
 * end to end, that a CORE quill + document handed to `Engine` render correctly
 * — i.e. the engine clones them into the Typst backend's memory on demand
 * (`toTree`→`fromTree`, `toJson`→`fromJson`) without the caller ever seeing a
 * backend handle.
 *
 * Aliased to pkg/runtime/runtime.js in vitest.config.js.
 */
import { describe, it, expect } from 'vitest'
import { Quill, Document, Engine, isQuillmarkError } from '@quillmark-wasm/runtime'
// Pin that the runtime's Quill IS the internal core build's class (re-export,
// not a parallel wrapper). This imports the internal core artifact directly —
// `pkg/core` is NOT a public package subpath, it is the build the root
// re-exports.
import { Quill as CoreQuill, Document as CoreDocument } from '../../../pkg/core/wasm.js'
import { makeQuill, makeSampleFormQuill, SAMPLE_FORM_MARKDOWN } from './test-helpers.js'

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let body = data.at("$body")

= #title

#body`

const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is a test document.`

function makeRuntimeQuill() {
  return Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
}

describe('@quillmark/wasm/runtime — surface', () => {
  // IMPLEMENTATION PIN: the root re-exports the internal core build's classes
  // verbatim (never wraps). There is exactly one public entry point, so this is
  // an internal structural fact rather than a cross-entry-point contract. If it
  // fails, the re-export was replaced by a wrapper — a breaking change, not a
  // refactor. See runtime.js.
  it('re-exports the internal core build classes verbatim (no parallel wrappers)', () => {
    expect(Quill).toBe(CoreQuill)
    expect(Document).toBe(CoreDocument)
  })

  it('builds a canonical Quill with a backendId and a round-tripping tree', () => {
    const quill = makeRuntimeQuill()
    expect(quill.backendId).toBe('typst')

    // toTree is the inverse of fromTree — re-materializing reproduces an
    // equivalent quill (same backend, same files).
    const tree = quill.toTree()
    expect(tree).toBeInstanceOf(Map)
    expect(tree.has('Quill.yaml')).toBe(true)
    const rebuilt = Quill.fromTree(tree)
    expect(rebuilt.backendId).toBe('typst')
  })

  it('parses a Document via the re-exported core class', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.quillRef).toBe('test_quill')
  })

  // ERROR CONTRACT: every fallible method throws a real Error carrying a
  // non-empty `diagnostics` array (the QuillmarkError structural interface).
  // isQuillmarkError is the exported narrowing guard for it.
  it('throws satisfy isQuillmarkError with non-empty structured diagnostics', () => {
    let caught
    try {
      Document.fromMarkdown('~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: [unclosed\n~~~\n\nbody')
    } catch (e) {
      caught = e
    }
    expect(caught).toBeInstanceOf(Error)
    expect(isQuillmarkError(caught)).toBe(true)
    expect(caught.diagnostics.length).toBeGreaterThan(0)
    const d = caught.diagnostics[0]
    expect(typeof d.message).toBe('string')
    expect(d.severity).toBeDefined()
    // message derives from the diagnostics (first message or an aggregate)
    expect(caught.message.length).toBeGreaterThan(0)
  })

  it('isQuillmarkError rejects non-quillmark values', () => {
    expect(isQuillmarkError(new Error('plain'))).toBe(false) // no diagnostics
    expect(isQuillmarkError({ diagnostics: [] })).toBe(false) // not an Error
    expect(isQuillmarkError(undefined)).toBe(false)
    expect(isQuillmarkError('boom')).toBe(false)
    // structural acceptance: any Error carrying a diagnostics array narrows,
    // regardless of which build or WASM instance constructed it
    const foreign = Object.assign(new Error('x'), { diagnostics: [] })
    expect(isQuillmarkError(foreign)).toBe(true)
  })
})

describe('@quillmark/wasm/runtime — Engine (hidden core→backend crossing)', () => {
  it('renders a core Quill + Document to PDF without exposing a backend handle', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = await engine.render(quill, doc, { format: 'pdf' })
    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.outputFormat).toBe('pdf')
    expect(result.artifacts[0].bytes).toBeInstanceOf(Uint8Array)
    expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)

    // The caller's canonical handles survive the render (clones were transient
    // and freed inside the engine; the originals are untouched).
    expect(quill.backendId).toBe('typst')
    expect(doc.quillRef).toBe('test_quill')
  })

  it('renders to SVG and reports supported formats / canvas capability', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const svg = await engine.render(quill, doc, { format: 'svg' })
    expect(svg.outputFormat).toBe('svg')

    const formats = await engine.supportedFormats(quill)
    expect(formats).toContain('svg')
    expect(typeof (await engine.supportsCanvas(quill))).toBe('boolean')
  })

  it('session.regions() is always a non-null array', async () => {
    // Regions are a session-level query, not on the render result. The document
    // body is a markdown content field, so it auto-tags one schema-field region
    // keyed `$body`; the result is always an array, never undefined.
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = await engine.open(quill, doc)
    const regions = session.regions()
    expect(Array.isArray(regions)).toBe(true)
    expect(regions.some((r) => r.field === '$body')).toBe(true)
    session.free()
  })

  it('manifest-backed capability probes do NOT load the backend', async () => {
    // A descriptor-form counting loader: it carries the same manifest the
    // default registry uses, so probes answer from the manifest (no load),
    // while still counting any real binary load triggered by render.
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/backends/typst/wasm.js')
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Descriptor WITH a manifest → probes answer from the manifest, no load.
    const formats = await engine.supportedFormats(quill)
    expect(formats).toContain('pdf')
    expect(typeof (await engine.supportsCanvas(quill))).toBe('boolean')
    expect(loaded).toBe(0)

    // A real render still triggers exactly one load.
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded).toBe(1)
  })

  it('manifest formats cannot drift from the loaded backend (drift guard)', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // What the static manifest reports (no load).
    const manifestFormats = await engine.supportedFormats(quill)
    const manifestCanvas = await engine.supportsCanvas(quill)

    // Force the backend to actually load, then ask the real engine directly.
    await engine.render(quill, doc, { format: 'svg' })
    const mod = await import('../../../pkg/backends/typst/wasm.js')
    const backendEngine = new mod.Quillmark()
    const backendQuill = mod.Quill.fromTree(quill.toTree())
    try {
      const realFormats = backendEngine.supportedFormats(backendQuill)
      const realCanvas = backendEngine.supportsCanvas(backendQuill)
      // The manifest must match what the binary reports, both directions.
      expect([...manifestFormats].sort()).toEqual([...realFormats].sort())
      expect(manifestCanvas).toBe(realCanvas)
    } finally {
      backendQuill.free()
    }
  })

  it('pdfform manifest cannot drift from the loaded backend (drift guard)', async () => {
    // Same drift guard as typst, but for the pdfform backend: the static
    // `{ formats, canvas }` manifest in DEFAULT_BACKENDS must match what the
    // loaded pdfform binary actually reports.
    const engine = new Engine()
    const quill = Quill.fromTree(makeSampleFormQuill())
    expect(quill.backendId).toBe('pdfform')
    const doc = Document.fromMarkdown(SAMPLE_FORM_MARKDOWN)

    // What the static manifest reports (no load).
    const manifestFormats = await engine.supportedFormats(quill)
    const manifestCanvas = await engine.supportsCanvas(quill)
    expect([...manifestFormats].sort()).toEqual(['pdf', 'png', 'svg'])
    expect(manifestCanvas).toBe(true)

    // Force the pdfform backend to load, then ask the real engine directly.
    await engine.render(quill, doc, { format: 'pdf' })
    const mod = await import('../../../pkg/backends/pdfform/wasm.js')
    const backendEngine = new mod.Quillmark()
    const backendQuill = mod.Quill.fromTree(quill.toTree())
    try {
      const realFormats = backendEngine.supportedFormats(backendQuill)
      const realCanvas = backendEngine.supportsCanvas(backendQuill)
      expect([...manifestFormats].sort()).toEqual([...realFormats].sort())
      expect(manifestCanvas).toBe(realCanvas)
    } finally {
      backendQuill.free()
    }
  })

  it('throws at construction for a malformed backend descriptor (names the id)', () => {
    // A backend entry must be a descriptor `{ load, formats, canvas }`; a bare thunk is rejected.
    expect(() => new Engine({ backends: { typst: () => import('../../../pkg/backends/typst/wasm.js') } })).toThrow(
      /typst/
    )
    // Missing/invalid manifest fields also fail fast at construction.
    expect(
      () => new Engine({ backends: { mybackend: { load: () => Promise.resolve({}), canvas: true } } })
    ).toThrow(/mybackend/)
    expect(
      () =>
        new Engine({
          backends: { mybackend: { load: () => Promise.resolve({}), formats: ['pdf'], canvas: 'yes' } }
        })
    ).toThrow(/mybackend/)
  })

  // A loader that wraps the real backend module so `Quill.fromTree` calls are
  // counted (and still delegate to the real implementation). Used to prove the
  // per-Engine quill-clone cache materializes the backend quill once per
  // canonical instance instead of per render/open call.
  function fromTreeCountingEngine(options) {
    let fromTreeCalls = 0
    const engine = new Engine({
      ...options,
      backends: {
        typst: {
          load: async () => {
            const real = await import('../../../pkg/backends/typst/wasm.js')
            const wrappedQuill = new Proxy(real.Quill, {
              get(target, prop, receiver) {
                if (prop === 'fromTree') {
                  return (...args) => {
                    fromTreeCalls++
                    return target.fromTree(...args)
                  }
                }
                return Reflect.get(target, prop, receiver)
              }
            })
            return new Proxy(real, {
              get(target, prop, receiver) {
                if (prop === 'Quill') return wrappedQuill
                return Reflect.get(target, prop, receiver)
              }
            })
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    return { engine, fromTreeCalls: () => fromTreeCalls }
  }

  it('caches the backend quill clone: rendering twice materializes it once', async () => {
    const { engine, fromTreeCalls } = fromTreeCountingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await engine.render(quill, doc, { format: 'svg' })
    await engine.render(quill, doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(1)
  })

  it('caches per canonical instance: two different quills → two materializations', async () => {
    const { engine, fromTreeCalls } = fromTreeCountingEngine()
    const quillA = makeRuntimeQuill()
    const quillB = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await engine.render(quillA, doc, { format: 'svg' })
    await engine.render(quillB, doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(2)
  })

  it('opens an iterative session, renders pages, and frees it', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = await engine.open(quill, doc)
    try {
      expect(session.pageCount).toBeGreaterThan(0)
      expect(session.backendId).toBe('typst')
      const page = session.render({ format: 'svg' })
      expect(page.artifacts.length).toBeGreaterThan(0)
    } finally {
      session.free()
    }
  })

  it('renders repeatedly from the same quill (clone-on-demand, no shared handle)', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const a = await engine.render(quill, doc, { format: 'svg' })
    const b = await engine.render(quill, doc, { format: 'svg' })
    expect(a.artifacts.length).toBeGreaterThan(0)
    expect(b.artifacts.length).toBeGreaterThan(0)
  })

  it('throws a clear error for an unregistered backend', async () => {
    const engine = new Engine()
    // A quill whose declared backend has no loader.
    const yaml = `quill:
  name: mystery
  version: "1.0.0"
  backend: doesnotexist
  description: no backend registered
main:
  fields:
    title:
      type: string
      example: x
`
    const quill = Quill.fromTree(new Map([['Quill.yaml', new TextEncoder().encode(yaml)]]))
    const doc = quill.seedDocument()
    await expect(engine.render(quill, doc)).rejects.toThrow(/no backend registered/)
  })

  it('accepts a custom backend descriptor override', async () => {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/backends/typst/wasm.js')
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded).toBe(1)
  })

  // A counting descriptor loader for the lazy-load / coalescing invariants below.
  function countingEngine() {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/backends/typst/wasm.js')
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    return { engine, loaded: () => loaded }
  }

  it('does NOT load the backend for sync core work — only on first render (lazy)', async () => {
    const { engine, loaded } = countingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Sync core surface (schema / validate / seed) touches no backend.
    expect(quill.schema).toBeDefined()
    quill.validate(doc)
    quill.seedDocument().free?.()
    expect(loaded()).toBe(0)

    // First render triggers exactly one backend load.
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded()).toBe(1)
  })

  it('coalesces concurrent first renders into a single backend load', async () => {
    const { engine, loaded } = countingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await Promise.all([
      engine.render(quill, doc, { format: 'svg' }),
      engine.render(quill, doc, { format: 'svg' }),
      engine.render(quill, doc, { format: 'svg' })
    ])
    expect(loaded()).toBe(1)
  })

  it('caller may free() its handles as soon as render/open returns (pre-await snapshot)', async () => {
    // Both caller handles are snapshotted before the first await inside
    // render/open (the backend load — a real suspension point on first call),
    // so a synchronous free() right after the call cannot race the clone.
    // Regression pin for the "null pointer passed to rust" panic (#782 §3):
    // each engine below is fresh, so its first call has the load pending when
    // free() runs.
    const renderEngine = new Engine()
    const renderQuill = makeRuntimeQuill()
    const renderDoc = Document.fromMarkdown(TEST_MARKDOWN)
    const pendingRender = renderEngine.render(renderQuill, renderDoc, { format: 'svg' })
    renderDoc.free()
    renderQuill.free()
    const result = await pendingRender
    expect(result.artifacts.length).toBeGreaterThan(0)

    const openEngine = new Engine()
    const openQuill = makeRuntimeQuill()
    const openDoc = Document.fromMarkdown(TEST_MARKDOWN)
    const pendingOpen = openEngine.open(openQuill, openDoc)
    openDoc.free()
    openQuill.free()
    const session = await pendingOpen
    try {
      expect(session.pageCount).toBeGreaterThan(0)
    } finally {
      session.free()
    }
  })

  it('propagates a clone-construction failure (doc clone), leaving the quill clone cached', async () => {
    // Exercises the teardown path when the doc clone (Document.fromJson) throws:
    // the quill clone is already materialized and cached (NOT freed here — that
    // is the T3 caching contract), only the per-call doc clone is freed in the
    // finally. We can only assert the error surfaces (cache/leak state is not
    // observable from JS), but this pins the throw path #withClones guards.
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const badDoc = { backendId: 'typst', toJson: () => '{"not":"a valid storage DTO"}' }
    await expect(engine.render(quill, badDoc)).rejects.toThrow()
  })
})
