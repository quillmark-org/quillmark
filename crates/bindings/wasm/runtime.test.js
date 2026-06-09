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
import { Quill, Document, Engine } from '@quillmark-wasm/runtime'
// Cross-check that the runtime's Quill IS the core build's class (re-export,
// not a parallel wrapper).
import { Quill as CoreQuill, Document as CoreDocument } from '@quillmark-wasm/core'
import { makeQuill } from './test-helpers.js'

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
  // CANONICAL INVARIANT GUARD: `/runtime` re-exports `/core`'s classes verbatim
  // (never wraps). This identity is what lets a `/core` handle pass straight to
  // `Engine` with no convert/adopt. If this fails, the re-export was replaced by
  // a wrapper — a breaking design change, not a refactor. See runtime.js.
  it('re-exports the core build classes verbatim (no parallel wrappers)', () => {
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

  it('bare-thunk custom loaders still work end to end (probe falls back to load)', async () => {
    let loaded = 0
    const engine = new Engine({
      backends: {
        // BARE THUNK form (no manifest) — must still resolve and render.
        typst: () => {
          loaded++
          return import('../../../pkg/backends/typst/wasm.js')
        }
      }
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Probe with no manifest must load the backend to answer (load+clone path).
    const formats = await engine.supportedFormats(quill)
    expect(formats).toContain('pdf')
    expect(loaded).toBe(1)

    // And rendering works against the same bare-thunk backend.
    const result = await engine.render(quill, doc, { format: 'svg' })
    expect(result.outputFormat).toBe('svg')
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

  it('invalidate(quill) re-materializes the clone on the next render', async () => {
    const { engine, fromTreeCalls } = fromTreeCountingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await engine.render(quill, doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(1)
    engine.invalidate(quill)
    await engine.render(quill, doc, { format: 'svg' })
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

  it('accepts a custom backend loader override', async () => {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: () => {
          loaded++
          return import('../../../pkg/backends/typst/wasm.js')
        }
      }
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded).toBe(1)
  })

  // A counting loader for the lazy-load / coalescing invariants below.
  function countingEngine() {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: () => {
          loaded++
          return import('../../../pkg/backends/typst/wasm.js')
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
