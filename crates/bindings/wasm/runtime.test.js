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
import { describe, it, expect, beforeAll } from 'vitest'
import {
  Quill,
  Document,
  Engine,
  DocumentWriter,
  CardWriter,
  DocumentView,
  CardView,
  MAIN_CARD_ADDR,
  isQuillmarkError,
  exportMarkdown,
} from '@quillmark-wasm/runtime'
// Pin that the runtime's Quill IS the internal core build's class (re-export,
// not a parallel wrapper). This imports the internal core artifact directly —
// `pkg/core` is NOT a public package subpath, it is the build the root
// re-exports.
import { Quill as CoreQuill, Document as CoreDocument } from '../../../pkg/core/wasm.js'
import {
  makeQuill,
  makeSampleFormQuill,
  SAMPLE_FORM_MARKDOWN,
  expectEditCode,
} from './test-helpers.js'

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

/** Read a field value from a card's payloadItems list by key. */
const fieldOf = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

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

// The typed-writer sugar binds a quill to a document once, so writes are bare
// `set` / `setAll` / `reviseField` / `card(i).set` — the JS twin of Rust's
// `quill.writer(doc)`, forwarding to the underscored `_commit*` / `_reviseField`
// ABI on the raw `Document` class (hidden from the `.d.ts`).
describe('@quillmark/wasm/runtime — DocumentWriter / CardWriter (bind the quill once)', () => {
  const EDITOR_QUILL_YAML = `quill:
  name: editor_test
  version: "1.0"
  backend: typst
  description: Typed writer sugar test

main:
  fields:
    subject:
      type: richtext
      inline: true
    qty:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: richtext
`
  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'editor_test', plate: TEST_PLATE, quillYaml: EDITOR_QUILL_YAML }))
  const blankDoc = () => Document.fromMarkdown('~~~card-yaml\n$quill: editor_test\n~~~\n\nBody.')

  it('quill.writer(doc) is the front door and returns a DocumentWriter', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    expect(ed).toBeInstanceOf(DocumentWriter)
    // The factory is sugar over the constructor — same class, no wrapping.
    expect(new DocumentWriter(quill, blankDoc())).toBeInstanceOf(DocumentWriter)
  })

  it('set binds the quill once and strict-commits a schema field', () => {
    const ed = buildQuill().writer(blankDoc())
    ed.set('qty', '3') // schema field → strict coerce
    expect(fieldOf(ed.document.main, 'qty')).toBe(3)
  })

  it('set rejects an undeclared name as a typo, not a fallback', () => {
    const ed = buildQuill().writer(blankDoc())
    expectEditCode(() => ed.set('stray', 'x'), 'edit::unknown_field')
    expect(fieldOf(ed.document.main, 'stray')).toBeUndefined()
  })

  it('setAll aborts the whole batch on a typo, applying nothing', () => {
    const ed = buildQuill().writer(blankDoc())
    expectEditCode(() => ed.setAll({ qty: '5', titel: 'oops' }), 'edit::unknown_field')
    expect(fieldOf(ed.document.main, 'qty')).toBeUndefined()
    expect(fieldOf(ed.document.main, 'titel')).toBeUndefined()
  })

  it('setBody writes the main body from markdown, receipt-free', () => {
    const ed = buildQuill().writer(blankDoc())
    ed.setBody('New **body**.')
    expect(ed.document.getMarkdown()).toBe('New **body**.')
  })

  it('reviseField writes a richtext field typed, and returns a Delta', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    const delta = ed.reviseField('subject', 'Q3 **results**')
    expect(quill.view(ed.document).get('subject')).toBe('Q3 **results**')
    expect(delta).toBeTruthy() // the anchor-preserving receipt
  })

  it('reviseField rejects an undeclared name, and a non-inline result', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    expectEditCode(() => ed.reviseField('stray', 'x'), 'edit::unknown_field')
    // `subject` is richtext(inline): a multi-block result is refused, field intact.
    ed.reviseField('subject', 'kept')
    expectEditCode(() => ed.reviseField('subject', 'a\n\nb'), 'edit::field_richtext_not_inline')
    expect(quill.view(ed.document).get('subject')).toBe('kept')
  })

  it('addCard fuses make + typed commit + push, transactionally', () => {
    const ed = buildQuill().writer(blankDoc())
    // `body` here is the card's richtext FIELD; the third arg is the card body.
    ed.addCard('note', { body: 'Field **body**.' }, 'Card body text.')
    expect(ed.document.cards).toHaveLength(1)
    expect(ed.document.cards[0].kind).toBe('note')
    expect(exportMarkdown(fieldOf(ed.document.cards[0], 'body'))).toBe('Field **body**.')
    expect(exportMarkdown(ed.document.cards[0].body)).toBe('Card body text.')
    // A typo aborts the commit; the card never joins the document.
    expectEditCode(() => ed.addCard('note', { stray: 'x' }), 'edit::unknown_field')
    expect(ed.document.cards).toHaveLength(1)
  })

  it('removeCard drops the card and returns it', () => {
    const ed = buildQuill().writer(blankDoc())
    ed.addCard('note', { body: 'x' })
    const removed = ed.removeCard(0)
    expect(removed.kind).toBe('note')
    expect(ed.document.cards).toHaveLength(0)
  })

  it('card(i).set / card(i).setBody target the composable card', () => {
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: editor_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
    const ed = buildQuill().writer(doc)
    ed.card(0).set('body', 'Card **body**.')
    expect(exportMarkdown(fieldOf(doc.cards[0], 'body'))).toBe('Card **body**.')
    ed.card(0).setBody('Card body md.')
    expect(exportMarkdown(doc.cards[0].body)).toBe('Card body md.')
    // card(i).reviseField is the typed, anchor-preserving field write.
    const delta = ed.card(0).reviseField('body', 'Revised **field**.')
    expect(exportMarkdown(fieldOf(doc.cards[0], 'body'))).toBe('Revised **field**.')
    expect(delta).toBeTruthy()
    expectEditCode(() => ed.card(0).reviseField('stray', 'x'), 'edit::unknown_field')
  })

  it('a bad card index throws at write time, not at card()', () => {
    const ed = buildQuill().writer(blankDoc())
    const cardEd = ed.card(9) // lazy: constructing the CardWriter never throws
    expect(cardEd).toBeInstanceOf(CardWriter)
    expectEditCode(() => cardEd.set('body', 'x'), 'edit::index_out_of_range')
  })

  it('get reads raw values quill-free; getMarkdown is body-only (field half retired)', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    ed.set('qty', '3')
    ed.set('subject', 'Q3 **results**')
    ed.setBody('Main **body**.')
    // Transport reads stay quill-free on the Document.
    expect(ed.document.get('qty')).toBe(3)
    expect(ed.document.get('missing')).toBeUndefined()
    // getMarkdown is the body read; a field address throws — a field's markdown
    // reads through the schema-plane view (#978).
    expect(ed.document.getMarkdown()).toBe('Main **body**.')
    expect(() => ed.document.getMarkdown({ field: 'subject' })).toThrow(/body-only/)
    expect(quill.view(ed.document).get('subject')).toBe('Q3 **results**')
    // view.get carries schema authority: an unknown name throws (vs `undefined`
    // from the quill-free transport `Document.get` above).
    expectEditCode(() => quill.view(ed.document).get('missing'), 'edit::unknown_field')
  })
})

// The typed-reader sugar is the read twin of the writer above: bind the quill
// once and read each field by its declared type — a richtext field to markdown,
// every other type verbatim — with schema authority, so an unknown field name
// throws rather than reading back `undefined` off the quill-free `Document`.
describe('@quillmark/wasm/runtime — DocumentView / CardView (the schema-plane read)', () => {
  const VIEW_QUILL_YAML = `quill:
  name: view_test
  version: "1.0"
  backend: typst
  description: Typed reader sugar test

main:
  fields:
    subject:
      type: richtext
      inline: true
    note:
      type: plaintext
    qty:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: richtext
`
  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'view_test', plate: TEST_PLATE, quillYaml: VIEW_QUILL_YAML }))
  const seededDoc = (quill) => {
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nMain **body**.')
    const w = quill.writer(doc)
    w.set('subject', 'Q3 **results**')
    w.set('qty', '3')
    w.addCard('note', { body: 'A *card* field.' }, 'Card body.')
    return doc
  }

  it('quill.view(doc) is the front door and returns a DocumentView', () => {
    const quill = buildQuill()
    const v = quill.view(seededDoc(quill))
    expect(v).toBeInstanceOf(DocumentView)
    expect(new DocumentView(quill, seededDoc(quill))).toBeInstanceOf(DocumentView)
  })

  it('interprets by declared type: richtext → markdown, plaintext → literal, scalar → canonical', () => {
    const quill = buildQuill()
    const doc = seededDoc(quill)
    quill.writer(doc).set('note', 'a *literal* line') // marks verbatim under plaintext
    const v = quill.view(doc)
    expect(v.get('subject')).toBe('Q3 **results**') // richtext projects to markdown
    expect(v.get('note')).toBe('a *literal* line') // plaintext projects verbatim
    expect(v.get('qty')).toBe(3) // scalar returns canonical
  })

  it('absence returns undefined; an unknown name throws (schema authority)', () => {
    const quill = buildQuill()
    const v = quill.view(Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.'))
    expect(v.get('subject')).toBeUndefined() // absent, not a typo
    expectEditCode(() => v.get('nope'), 'edit::unknown_field') // typo, not absent
  })

  it('a richtext field holding a scalar throws FieldRichtextDecode', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    doc.storeField('subject', 3) // opaque write puts a bare number under richtext
    expectEditCode(() => quill.view(doc).get('subject'), 'edit::field_richtext_decode')
  })

  it('an absent field addr reads the body markdown, quill-free', () => {
    const quill = buildQuill()
    const v = quill.view(seededDoc(quill))
    expect(v.getBody()).toBe('Main **body**.')
    expect(v.get({})).toBe('Main **body**.') // {} = main body, equals getBody()
  })

  it('card(i).get reads a card field through its $kind schema', () => {
    const quill = buildQuill()
    const v = quill.view(seededDoc(quill))
    expect(v.card(0).kind).toBe('note')
    expect(v.card(0).get('body')).toBe('A *card* field.')
    expect(v.card(0).getBody()).toBe('Card body.')
    expectEditCode(() => v.card(0).get('nope'), 'edit::unknown_field')
  })

  it('a bad card index throws at read time, not at card()', () => {
    const quill = buildQuill()
    const cardView = quill.view(seededDoc(quill)).card(9)
    expect(cardView).toBeInstanceOf(CardView)
    expectEditCode(() => cardView.get('body'), 'edit::index_out_of_range')
  })
})

// MAIN_CARD_ADDR names the empty main-card address `{}` the card-scoped verbs
// take, so a main-card batch write reads as intent (`storeFields(MAIN_CARD_ADDR,
// fields)`) rather than as an anonymous `{}`. It IS `{}` (frozen), so it is a
// pure alias — `{}` and `undefined` stay equally valid.
describe('@quillmark/wasm/runtime — MAIN_CARD_ADDR (the named main-card address)', () => {
  it('is a frozen, empty card address — {} with a name', () => {
    expect(MAIN_CARD_ADDR).toEqual({})
    expect(Object.isFrozen(MAIN_CARD_ADDR)).toBe(true)
  })

  it('targets the main card on a card-scoped verb, identically to {}', () => {
    const named = new Document('editor_test')
    named.storeFields(MAIN_CARD_ADDR, { title: 'Hello', qty: 3 })
    expect(fieldOf(named.main, 'title')).toBe('Hello')
    expect(fieldOf(named.main, 'qty')).toBe(3)

    // Same effect as the bare empty-address spelling — a pure alias.
    const empty = new Document('editor_test')
    empty.storeFields({}, { title: 'Hello', qty: 3 })
    expect(named.main.payloadItems).toEqual(empty.main.payloadItems)
  })

  it('carries $ext onto the main card too', () => {
    const doc = new Document('editor_test')
    doc.storeExt(MAIN_CARD_ADDR, { editor: { pinned: true } })
    expect(doc.main.ext.editor.pinned).toBe(true)
  })
})

describe('@quillmark/wasm/runtime — Engine (hidden core→backend crossing)', () => {
  // Warm the lazy Typst-backend import + first Typst compile once, outside any
  // timed test. `Engine.render` dynamically `import()`s the backend wasm binary
  // on first render — a one-time cost (large module instantiation) that on a
  // cold CI runner alone can approach the per-test ceiling. Paying it here keeps
  // the individual render tests warm (sub-second, like the SVG case) so a tight
  // per-test `testTimeout` still catches a genuine hang. The hook carries its own
  // generous timeout for the cold load.
  beforeAll(async () => {
    await new Engine().render(makeRuntimeQuill(), Document.fromMarkdown(TEST_MARKDOWN), {
      format: 'pdf',
    })
  }, 120000)

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

  // GUARD for the class of bug where a method is declared in runtime.d.ts and
  // implemented in the backend build, but the hand-written canonical LiveSession
  // wrapper (runtime.js) forgets to forward it — `fieldAt`, issue #801. The
  // type-level drift test (runtime.types.test-d.ts) only checks structural type
  // compatibility, so a wrapper that TYPE-checks but has no matching JS method
  // sails through it and throws `X is not a function` at runtime. This calls
  // EVERY documented LiveSession member on a live canonical session, so a
  // dropped delegation surfaces here instead of only in a consumer.
  it('canonical LiveSession forwards every documented method to the inner session', async () => {
    // paint() downcasts its argument to a 2D context via wasm-bindgen's
    // `instanceof` check, so it needs these globals present (Node has no DOM).
    class FakeImageData {
      constructor(data, width, height) {
        this.data = data
        this.width = width
        this.height = height
      }
    }
    class FakeCanvasRenderingContext2D {
      constructor() {
        this.calls = []
        this.canvas = { width: 0, height: 0 }
      }
      putImageData(img, dx, dy) {
        this.calls.push({ width: img.width, height: img.height, dx, dy })
      }
    }
    globalThis.ImageData = FakeImageData
    globalThis.CanvasRenderingContext2D = FakeCanvasRenderingContext2D

    // A SINGLE-LINE $body, deliberately. `fieldAt` hit-tests per-glyph ink
    // boxes, so the probe point below (the region rect's centre) must land on
    // ink: a one-line body's region rect IS that line's contiguous glyph
    // boxes, so its centre is ink by construction. TEST_MARKDOWN's
    // heading+paragraph body has an inter-line gap at the union rect's
    // centre, where fieldAt correctly answers undefined.
    const SMOKE_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Smoke Test
author: Smoke Author
~~~

A single line of body ink.`

    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(SMOKE_MARKDOWN)
    const session = await engine.open(quill, doc)
    try {
      // Getters.
      expect(session.pageCount).toBeGreaterThan(0)
      expect(session.backendId).toBe('typst')
      expect(typeof session.supportsCanvas).toBe('boolean')
      expect(Array.isArray(session.warnings)).toBe(true)

      // render.
      expect(typeof session.render).toBe('function')
      expect(session.render({ format: 'svg' }).artifacts.length).toBeGreaterThan(0)

      // regions — the body markdown content field auto-tags one region, keyed
      // by the canonical DocPath `main.body`.
      expect(typeof session.regions).toBe('function')
      const regions = session.regions()
      const body = regions.find((r) => r.field === 'main.body')
      expect(body).toBeDefined()

      // pageSize.
      const size = session.pageSize(body.page)
      expect(size.widthPt).toBeGreaterThan(0)
      expect(size.heightPt).toBeGreaterThan(0)

      // fieldAt — the delegation that was missing (#801). Hit-test the centre
      // of the body region's rect ([x0, y0, x1, y1], bottom-left PDF points)
      // — guaranteed ink for the single-line body (see SMOKE_MARKDOWN above) —
      // and expect it to resolve back through the wrapper as its DocPath. Off
      // any field's ink (the page corner) the contract is undefined.
      expect(typeof session.fieldAt).toBe('function')
      const [x0, y0, x1, y1] = body.rect
      const hit = session.fieldAt(body.page, (x0 + x1) / 2, (y0 + y1) / 2)
      expect(hit).toBe('main.body')
      expect(session.fieldAt(body.page, 1, 1)).toBeUndefined()

      // fieldBoxes — the whole-field union helper. A single-line body has one
      // span-bearing segment, so its box unions to one rect covering that line.
      expect(typeof session.fieldBoxes).toBe('function')
      const boxes = session.fieldBoxes('main.body')
      expect(boxes.length).toBe(1)
      expect(boxes[0].field).toBe('main.body')
      expect(boxes[0].span).toBeDefined()
      // A field with no span-bearing region has no derived content box.
      expect(session.fieldBoxes('does_not_exist')).toEqual([])

      // positionAt — the fine-grained click direction, carrying the granularity
      // signal. A hit on the single line's ink is cluster-exact.
      expect(typeof session.positionAt).toBe('function')
      const chit = session.positionAt(body.page, (x0 + x1) / 2, (y0 + y1) / 2)
      expect(chit.field).toBe('main.body')
      expect(chit.granularity).toBe('cluster')

      // paint.
      expect(typeof session.paint).toBe('function')
      const ctx = new FakeCanvasRenderingContext2D()
      const paintResult = session.paint(ctx, body.page)
      expect(paintResult.pixelWidth).toBeGreaterThan(0)

      // apply — recompile in place.
      expect(typeof session.apply).toBe('function')
      const cs = session.apply(Document.fromMarkdown(SMOKE_MARKDOWN))
      expect(Array.isArray(cs.dirtyPages)).toBe(true)
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
