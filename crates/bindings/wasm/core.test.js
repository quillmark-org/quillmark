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

// Card field accessor (mirrors the payloadItems shape used across the suite).
const field = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

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
    expect(core.LiveSession).toBeUndefined()
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

  it('seedCard layers a $seed overlay over the schema example', () => {
    const yaml = `quill:
  name: seed_core
  version: "1.0.0"
  backend: typst
  description: Seed overlay smoke test
main:
  fields:
    title:
      type: string
      example: T
card_kinds:
  note:
    fields:
      author:
        type: string
        example: A. Author
`
    const quill = Quill.fromTree(new Map([['Quill.yaml', enc.encode(yaml)]]))

    const doc = Document.fromMarkdown(
      '~~~\n$quill: seed_core@1.0.0\n$kind: main\n$seed:\n  note:\n    author: Custom Author\n~~~\n',
    )

    // The per-kind overlay is read off main.seed[kind] (undefined for unknown).
    const overlay = doc.main.seed?.note
    expect(overlay.author).toBe('Custom Author')
    expect(doc.main.seed?.missing).toBeUndefined()

    // seedCard layers it over the example (overlay › example); omitting the
    // overlay yields the bare schema example.
    expect(field(quill.seedCard('note', overlay), 'author')).toBe('Custom Author')
    expect(field(quill.seedCard('note'), 'author')).toBe('A. Author')

    // storeSeedNamespace writes an overlay; main.seed reads it back; remove clears.
    const doc2 = Document.fromMarkdown('~~~\n$quill: seed_core@1.0.0\n$kind: main\n~~~\n')
    doc2.storeSeedNamespace('note', { author: 'Written' })
    expect(doc2.main.seed?.note.author).toBe('Written')
    doc2.removeSeedNamespace('note')
    expect(doc2.main.seed?.note).toBeUndefined()
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

// The core bundle's reason to exist is the editor: the full Document mutation +
// persistence surface must work with Typst absent, not merely be present.
describe('@quillmark/wasm/core Document editing (Typst-free)', () => {
  it('builds, mutates, and round-trips a document with no engine', () => {
    const doc = Document.fromMarkdown(`~~~
$quill: core_test
$kind: main
title: Draft
~~~

# Body`)

    // Edit the main card and append a composable card.
    doc.storeField('title', 'Final')
    doc.insertCard(Document.makeCard('note', { author: 'Alice' }, 'A note.'))
    expect(doc.cardCount).toBe(1)
    expect(doc.cards[0].kind).toBe('note')

    doc.storeField({ card: 0, field: 'author' }, 'Bob')
    // Keyed card read (#953) — mirrors the write, no payloadItems walk. Agrees
    // with the hand-rolled projection it replaces.
    expect(doc.getStored({ card: 0, field: 'author' })).toBe('Bob')
    expect(doc.getStored({ card: 0, field: 'author' })).toBe(field(doc.cards[0], 'author'))

    // Storage DTO round-trips losslessly — the editor's persistence path.
    const restored = Document.fromJson(doc.toJson())
    expect(restored.equals(doc)).toBe(true)

    // Removal works back down to empty.
    doc.removeCard(0)
    expect(doc.cardCount).toBe(0)
  })

  it('keyed card reads mirror the card write verbs (#953)', () => {
    const doc = Document.fromMarkdown(`~~~
$quill: core_test
$kind: main
title: Draft
~~~

# Body`)
    doc.insertCard(Document.makeCard('note', { author: 'Alice' }, 'A note body.'))

    // getStored — value keyed by name; undefined when the field is absent.
    expect(doc.getStored({ card: 0, field: 'author' })).toBe('Alice')
    expect(doc.getStored({ card: 0, field: 'missing' })).toBeUndefined()

    // getMarkdown is the card body read (card address); a field address throws.
    // A field's markdown reads through the schema-plane view,
    // quill.reader(doc).card(i).get(name) (#978).
    expect(doc.getMarkdown({ card: 0 })).toContain('A note body.')
    expect(() => doc.getMarkdown({ card: 0, field: 'author' })).toThrow(/body-only/)

    // An out-of-range index is a boundary error — it throws, the way the card
    // write verbs do, rather than reading back as undefined/"".
    expect(() => doc.getStored({ card: 1, field: 'author' })).toThrow()
    expect(() => doc.getMarkdown({ card: 1 })).toThrow()

    // getStored still reads the raw value verbatim (transport) — including a scalar a
    // storeField wrote under a would-be richtext field.
    doc.storeField({ card: 0, field: 'qty' }, 3)
    expect(doc.getStored({ card: 0, field: 'qty' })).toBe(3)
  })

  it('single-card, $id, and seed-overlay reads (#956)', () => {
    const doc = Document.fromMarkdown(`~~~
$quill: core_test
$kind: main
title: Draft
~~~

# Body`)
    doc.insertCard({ kind: 'note', id: 'dup', body: 'A' })
    doc.insertCard({ kind: 'note', id: 'other', body: 'B' })
    doc.insertCard({ kind: 'note', id: 'dup', body: 'C' })

    // card(i) reads one whole card without materializing the cards array.
    expect(doc.card(1).kind).toBe('note')
    expect(doc.card(1).id).toBe('other')
    expect(() => doc.card(3)).toThrow() // out of range is a boundary error

    // cardIndexById resolves the durable $id address; non-unique → first match.
    expect(doc.cardIndexById('dup')).toBe(0)
    expect(doc.cardIndexById('other')).toBe(1)
    expect(doc.cardIndexById('missing')).toBeUndefined()

    // seedOverlay reads one $seed[kind] entry off the main card cheaply, the
    // overlay you feed straight into quill.seedCard(kind, overlay).
    doc.storeSeedNamespace('note', { author: 'Seeded' })
    expect(doc.seedOverlay('note')).toEqual({ author: 'Seeded' })
    expect(doc.seedOverlay('absent')).toBeUndefined()
  })
})
