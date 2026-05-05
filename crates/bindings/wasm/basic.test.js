/**
 * Smoke tests for quillmark-wasm — Document API (Phase 2)
 *
 * These tests cover the canonical flow:
 *   engine.quill(tree) → Document.fromMarkdown(markdown) → quill.render(doc, opts)
 *
 * Setup: Tests use the bundler build via @quillmark-wasm alias (see vitest.config.js)
 */

import { describe, it, expect } from 'vitest'
import { Quillmark, Document } from '@quillmark-wasm'
import { makeQuill } from './test-helpers.js'

const TEST_MARKDOWN = `---
QUILL: test_quill
title: Test Document
author: Test Author
---

# Hello World

This is a test document.`

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let body = data.BODY

= #title

#body`

describe('Document.fromMarkdown', () => {
  it('should parse markdown with YAML frontmatter', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(doc).toBeDefined()
    expect(doc.quillRef).toBe('test_quill')
  })

  it('should expose typed frontmatter (no QUILL/BODY/CARDS)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(doc.main.frontmatter).toBeDefined()
    expect(doc.main.frontmatter instanceof Object).toBe(true)
    expect(doc.main.frontmatter.title).toBe('Test Document')
    expect(doc.main.frontmatter.author).toBe('Test Author')
    // QUILL, BODY, CARDS must NOT appear in frontmatter
    expect(doc.main.frontmatter.QUILL).toBeUndefined()
    expect(doc.main.frontmatter.BODY).toBeUndefined()
    expect(doc.main.frontmatter.CARDS).toBeUndefined()
  })

  it('should expose body as a string', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(typeof doc.main.body).toBe('string')
    expect(doc.main.body).toContain('Hello World')
  })

  it('should expose cards as an array', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(Array.isArray(doc.cards)).toBe(true)
    expect(doc.cards.length).toBe(0)
  })

  it('should expose card fields and body', () => {
    const md = `---
QUILL: test_quill
---

Global body.

---
CARD: note
foo: bar
---

Card body.
`
    const doc = Document.fromMarkdown(md)

    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].tag).toBe('note')
    expect(doc.cards[0].frontmatter.foo).toBe('bar')
    expect(doc.cards[0].body).toContain('Card body.')
  })

  it('should expose warnings array', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(Array.isArray(doc.warnings)).toBe(true)
    expect(doc.warnings.length).toBe(0)
  })

  it('should throw on invalid YAML frontmatter', () => {
    const badMarkdown = `---
title: Test
QUILL: test_quill
this is not valid yaml
---

# Content`

    expect(() => {
      Document.fromMarkdown(badMarkdown)
    }).toThrow()
  })

  it('should throw when QUILL field is absent', () => {
    const markdownWithoutQuill = `---
title: Default Test
author: Test Author
---

# Hello Default

This document has no QUILL tag.`

    expect(() => {
      Document.fromMarkdown(markdownWithoutQuill)
    }).toThrow()
  })

  it('attaches err.diagnostics as a non-empty array on thrown errors', () => {
    // Thrown errors normalise to a flat { message, diagnostics[] } shape
    // regardless of whether the underlying failure produced one diagnostic
    // or many.
    try {
      Document.fromMarkdown('')
      throw new Error('fromMarkdown should have thrown')
    } catch (err) {
      expect(Array.isArray(err.diagnostics)).toBe(true)
      expect(err.diagnostics.length).toBeGreaterThanOrEqual(1)
      expect(err.diagnostics[0]).toHaveProperty('message')
      expect(err.diagnostics[0]).toHaveProperty('severity')
      expect(err.message).toMatch(/Empty markdown input/)
    }
  })
})

// ---------------------------------------------------------------------------
// Document.toMarkdown — emitter integration tests (Phase 4c)
// ---------------------------------------------------------------------------

describe('Document.toMarkdown — fromMarkdown → mutate → emit → re-parse', () => {
  it('general round-trip: mutated document survives emit → re-parse', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const originalCardCount = doc.cards.length  // 0 for TEST_MARKDOWN

    // Mutate
    doc.setField('title', 'New Title')
    doc.pushCard({ tag: 'note', fields: { author: 'Alice' }, body: 'Hello' })
    doc.replaceBody('Updated body')

    // Emit
    const emitted = doc.toMarkdown()
    expect(typeof emitted).toBe('string')
    expect(emitted.length).toBeGreaterThan(0)

    // Re-parse and assert structure survives.
    //
    // Note on trailing newlines: the global body is followed by a card fence,
    // so the wire format inserts a line terminator + F2 blank line between
    // them (`Updated body\n\n---`). On re-parse the F2 blank is stripped but
    // the terminator stays, so `doc2.main.body === 'Updated body\n'`. The card
    // body is at EOF and has no F2 separator, so it survives byte-for-byte.
    const doc2 = Document.fromMarkdown(emitted)
    expect(doc2.main.frontmatter.title).toBe('New Title')
    expect(doc2.main.body).toBe('Updated body\n')
    expect(doc2.cards.length).toBe(originalCardCount + 1)
    expect(doc2.cards[0].tag).toBe('note')
    expect(doc2.cards[0].frontmatter.author).toBe('Alice')
    expect(doc2.cards[0].body).toBe('Hello')
  })

  it('ambiguous-string survival: YAML-keyword values are preserved as strings', () => {
    // "on", "off", "yes", "no", "true", "false", "null" are all YAML booleans/null
    // in permissive parsers. The emitter must double-quote them so they survive
    // as strings through a re-parse.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('flag_on', 'on')
    doc.setField('flag_off', 'off')
    doc.setField('flag_yes', 'yes')
    doc.setField('flag_no', 'no')
    doc.setField('str_true', 'true')
    doc.setField('str_false', 'false')
    doc.setField('str_null', 'null')
    doc.setField('octal_str', '01234')
    doc.setField('date_str', '2024-01-15')

    const emitted = doc.toMarkdown()
    const doc2 = Document.fromMarkdown(emitted)

    // Every value must survive as a string, not be re-interpreted as bool/null/number
    expect(doc2.main.frontmatter.flag_on).toBe('on')
    expect(doc2.main.frontmatter.flag_off).toBe('off')
    expect(doc2.main.frontmatter.flag_yes).toBe('yes')
    expect(doc2.main.frontmatter.flag_no).toBe('no')
    expect(doc2.main.frontmatter.str_true).toBe('true')
    expect(doc2.main.frontmatter.str_false).toBe('false')
    expect(doc2.main.frontmatter.str_null).toBe('null')
    expect(doc2.main.frontmatter.octal_str).toBe('01234')
    expect(doc2.main.frontmatter.date_str).toBe('2024-01-15')
  })
})

describe('Quillmark.quill', () => {
  it('should return a render-ready Quill', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    expect(quill).toBeDefined()
  })

  it('should accept a plain object tree (Record<string, Uint8Array>)', () => {
    const engine = new Quillmark()
    const mapTree = makeQuill({ name: 'test_quill', plate: TEST_PLATE })
    const objectTree = Object.fromEntries(mapTree)

    const fromMap = engine.quill(mapTree)
    const fromObject = engine.quill(objectTree)

    expect(fromMap.backendId).toBe(fromObject.backendId)

    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const r1 = fromMap.render(doc, { format: 'svg' })
    const r2 = fromObject.render(doc, { format: 'svg' })
    expect(r1.artifacts.length).toBe(r2.artifacts.length)
  })

  it('should reject non-object trees with a clear error', () => {
    const engine = new Quillmark()
    expect(() => engine.quill(42)).toThrow()
    expect(() => engine.quill('string')).toThrow()
    expect(() => engine.quill(null)).toThrow()
  })

  it('should render markdown to PDF via quill.render(doc) with default opts', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = quill.render(doc)

    expect(result).toBeDefined()
    expect(result.artifacts).toBeDefined()
    expect(result.artifacts.length).toBeGreaterThan(0)
    // The declared TS type is Uint8Array — assert the runtime matches so
    // consumers don't need to defensively coerce `new Uint8Array(bytes)`.
    expect(result.artifacts[0].bytes).toBeInstanceOf(Uint8Array)
    expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)
    expect(result.artifacts[0].mimeType).toBe('application/pdf')
  })

  it('should render markdown to PDF via quill.render(doc, opts)', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = quill.render(doc, { format: 'pdf' })

    expect(result).toBeDefined()
    expect(result.artifacts).toBeDefined()
    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)
    expect(result.artifacts[0].mimeType).toBe('application/pdf')
  })

  it('should render markdown to SVG via quill.render(doc)', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = quill.render(doc, { format: 'svg' })

    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.artifacts[0].mimeType).toBe('image/svg+xml')
  })

  it('should allow rendering the same Document multiple times', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const pdf = quill.render(doc, { format: 'pdf' })
    const svg = quill.render(doc, { format: 'svg' })

    expect(pdf.artifacts[0].mimeType).toBe('application/pdf')
    expect(svg.artifacts[0].mimeType).toBe('image/svg+xml')
  })

  it('should emit a quill::ref_mismatch warning when Document QUILL differs from quill name', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))

    // Document declares a different quill name
    const otherMarkdown = `---
QUILL: other_quill
title: Mismatch Test
---

# Content`
    const doc = Document.fromMarkdown(otherMarkdown)
    const result = quill.render(doc, { format: 'pdf' })

    expect(result.warnings.length).toBe(1)
    expect(result.warnings[0].code).toBe('quill::ref_mismatch')
    expect(result.artifacts.length).toBeGreaterThan(0)
  })
})

// ---------------------------------------------------------------------------
// Document editor surface (Phase 3)
// ---------------------------------------------------------------------------

describe('Document editor surface — setField / removeField', () => {
  it('setField inserts a new frontmatter field', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('subtitle', 'A subtitle')
    expect(doc.main.frontmatter.subtitle).toBe('A subtitle')
  })

  it('setField updates an existing field', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('title', 'Updated')
    expect(doc.main.frontmatter.title).toBe('Updated')
  })

  it('setField throws EditError::ReservedName for BODY', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('BODY', 'x')).toThrow(/ReservedName/)
  })

  it('setField throws EditError::ReservedName for CARDS', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('CARDS', [])).toThrow(/ReservedName/)
  })

  it('setField throws EditError::ReservedName for QUILL', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('QUILL', 'x')).toThrow(/ReservedName/)
  })

  it('setField throws EditError::ReservedName for CARD', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('CARD', 'x')).toThrow(/ReservedName/)
  })

  it('setField throws EditError::InvalidFieldName for uppercase name', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('Title', 'x')).toThrow(/InvalidFieldName/)
  })

  it('removeField returns the removed value', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const removed = doc.removeField('title')
    expect(removed).toBe('Test Document')
    expect(doc.main.frontmatter.title).toBeUndefined()
  })

  it('removeField returns undefined when field absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.removeField('nonexistent')).toBeUndefined()
  })

  it('removeField throws EditError::ReservedName for QUILL', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.removeField('QUILL')).toThrow(/ReservedName/)
  })

  it('removeField throws EditError::ReservedName for BODY/CARDS/CARD', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const reserved of ['BODY', 'CARDS', 'CARD']) {
      expect(() => doc.removeField(reserved)).toThrow(/ReservedName/)
    }
  })

  it('removeField throws EditError::InvalidFieldName for uppercase name', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.removeField('Title')).toThrow(/InvalidFieldName/)
  })
})

describe('Document editor surface — setQuillRef / replaceBody', () => {
  it('setQuillRef changes the quillRef', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setQuillRef('new_quill')
    expect(doc.quillRef).toBe('new_quill')
  })

  it('setQuillRef throws on invalid reference', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setQuillRef('INVALID QUILL REF WITH SPACES')).toThrow()
  })

  it('replaceBody changes the body', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.replaceBody('Brand new body.')
    expect(doc.main.body).toBe('Brand new body.')
  })
})

describe('Document editor surface — card mutations', () => {
  const MD_WITH_CARDS = `---
QUILL: test_quill
---

Body.

---
CARD: note
foo: bar
---

Card one.

---
CARD: summary
---

Card two.
`

  it('pushCard appends a card', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.pushCard({ tag: 'note', fields: {}, body: 'My card.' })
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].tag).toBe('note')
    expect(doc.cards[0].body).toBe('My card.')
  })

  it('pushCard throws on invalid tag', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.pushCard({ tag: 'BadTag' })).toThrow(/InvalidTagName/)
  })

  it('insertCard inserts at specified index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.insertCard(0, { tag: 'intro' })
    expect(doc.cards[0].tag).toBe('intro')
    expect(doc.cards[1].tag).toBe('note')
  })

  it('insertCard throws IndexOutOfRange when index > len', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expect(() => doc.insertCard(5, { tag: 'note' })).toThrow(/IndexOutOfRange/)
  })

  it('removeCard removes and returns the card', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    const removed = doc.removeCard(0)
    expect(removed).toBeDefined()
    expect(removed.tag).toBe('note')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].tag).toBe('summary')
  })

  it('removeCard returns undefined when out of range', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.removeCard(0)).toBeUndefined()
  })

  it('moveCard swaps positions correctly', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(1, 0) // summary → front
    expect(doc.cards[0].tag).toBe('summary')
    expect(doc.cards[1].tag).toBe('note')
  })

  it('moveCard no-op when from == to', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(0, 0)
    expect(doc.cards[0].tag).toBe('note')
  })

  it('moveCard throws IndexOutOfRange on out-of-range index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // 2 cards
    expect(() => doc.moveCard(5, 0)).toThrow(/IndexOutOfRange/)
  })

  it('setCardTag renames the tag in place', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.setCardTag(0, 'annotation')
    expect(doc.cards[0].tag).toBe('annotation')
    // Frontmatter preserved across rename.
    expect(doc.cards[0].frontmatter).toBeDefined()
  })

  it('setCardTag throws InvalidTagName for empty/uppercase/dashed tags', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    for (const bad of ['', 'BadTag', 'with-dash']) {
      expect(() => doc.setCardTag(0, bad)).toThrow(/InvalidTagName/)
    }
  })

  it('setCardTag throws IndexOutOfRange when index >= len', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // 2 cards
    expect(() => doc.setCardTag(5, 'annotation')).toThrow(/IndexOutOfRange/)
  })

  it('cardCount reports composable card count without allocating', () => {
    const empty = Document.fromMarkdown(TEST_MARKDOWN)
    expect(empty.cardCount).toBe(0)

    const two = Document.fromMarkdown(MD_WITH_CARDS)
    expect(two.cardCount).toBe(2)
    two.pushCard({ tag: 'extra' })
    expect(two.cardCount).toBe(3)
    two.removeCard(0)
    expect(two.cardCount).toBe(2)
  })
})

describe('Document.equals', () => {
  it('returns true for identical documents', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    expect(a.equals(b)).toBe(true)
  })

  it('returns true for clones', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = a.clone()
    expect(a.equals(b)).toBe(true)
  })

  it('returns false after a frontmatter mutation', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    b.setField('title', 'Different')
    expect(a.equals(b)).toBe(false)
  })

  it('returns false after a body mutation', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    b.replaceBody('Different body')
    expect(a.equals(b)).toBe(false)
  })

  it('returns false after pushing a card', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    b.pushCard({ tag: 'note' })
    expect(a.equals(b)).toBe(false)
  })

  it('survives round-trip through toMarkdown / fromMarkdown', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(a.toMarkdown())
    expect(a.equals(b)).toBe(true)
  })
})

describe('Document editor surface — updateCardField / updateCardBody', () => {
  const MD_WITH_CARD = `---
QUILL: test_quill
---

Body.

---
CARD: note
foo: bar
---

Card body.
`

  it('updateCardField sets a field on a card', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.updateCardField(0, 'content', 'hello')
    expect(doc.cards[0].frontmatter.content).toBe('hello')
  })

  it('updateCardField throws EditError::ReservedName for BODY', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    expect(() => doc.updateCardField(0, 'BODY', 'x')).toThrow(/ReservedName/)
  })

  it('updateCardField throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expect(() => doc.updateCardField(0, 'title', 'x')).toThrow(/IndexOutOfRange/)
  })

  it('removeCardField returns the removed value and deletes the key', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    const removed = doc.removeCardField(0, 'foo')
    expect(removed).toBe('bar')
    expect('foo' in doc.cards[0].frontmatter).toBe(false)
  })

  it('removeCardField returns undefined when field absent', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    expect(doc.removeCardField(0, 'nonexistent')).toBeUndefined()
  })

  it('removeCardField throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expect(() => doc.removeCardField(0, 'foo')).toThrow(/IndexOutOfRange/)
  })

  it('updateCardBody replaces card body', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.updateCardBody(0, 'New card body.')
    expect(doc.cards[0].body).toBe('New card body.')
  })

  it('updateCardBody throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expect(() => doc.updateCardBody(0, 'x')).toThrow(/IndexOutOfRange/)
  })
})

describe('Document editor surface — parse→mutate→read round-trip', () => {
  it('mutated document reflects changes in subsequent reads', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Mutate
    doc.setField('author', 'Bob')
    doc.replaceBody('New body text.')
    doc.pushCard({ tag: 'note', body: 'Card content.' })
    doc.setQuillRef('updated_quill')

    // Assert state
    expect(doc.main.frontmatter.author).toBe('Bob')
    expect(doc.main.body).toBe('New body text.')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].tag).toBe('note')
    expect(doc.cards[0].body).toBe('Card content.')
    expect(doc.quillRef).toBe('updated_quill')

    // Original title still present
    expect(doc.main.frontmatter.title).toBe('Test Document')

    // Warnings untouched
    expect(Array.isArray(doc.warnings)).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// open + session.render
// ---------------------------------------------------------------------------

describe('quill.open + session.render', () => {
  it('should support open + session.render with pageCount', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = quill.open(doc)
    expect(typeof session.pageCount).toBe('number')
    expect(session.pageCount).toBeGreaterThan(0)

    const defaultFmt = session.render()
    expect(defaultFmt.artifacts.length).toBeGreaterThan(0)
    expect(defaultFmt.artifacts[0].mimeType).toBe('application/pdf')

    const allPages = session.render({ format: 'svg' })
    expect(allPages.artifacts.length).toBe(session.pageCount)
    expect(allPages.artifacts[0].mimeType).toBe('image/svg+xml')

    const subset = session.render({ format: 'png', ppi: 80, pages: [0, 0] })
    expect(subset.artifacts.length).toBe(2)
    expect(subset.artifacts[0].mimeType).toBe('image/png')
  })

  it('should throw on out-of-bounds page indices', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = quill.open(doc)
    const oob = session.pageCount + 10

    expect(() => {
      session.render({ format: 'png', ppi: 80, pages: [0, oob] })
    }).toThrow(/out of bounds/)
  })

  it('should error when requesting page selection with PDF', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = quill.open(doc)

    expect(() => {
      session.render({ format: 'pdf', pages: [0] })
    }).toThrow()
  })
})

describe('quill.metadata', () => {
  const META_QUILL_YAML = `quill:
  name: meta_test_quill
  version: "0.2.1"
  backend: typst
  plate_file: plate.typ
  description: Metadata test

main:
  description: The main card schema
  fields:
    title:
      type: string
      description: The title

card_types:
  indorsement:
    description: Indorsement
    fields:
      signature_block:
        type: string
`

  it('exposes identity on metadata and schemas on dedicated getters', () => {
    const engine = new Quillmark()
    const quill = engine.quill(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )

    // metadata mirrors the `quill:` section of Quill.yaml — identity only.
    const meta = quill.metadata
    expect(meta).toBeDefined()
    expect(meta.name).toBe('meta_test_quill')
    expect(meta.version).toBe('0.2.1')
    expect(meta.backend).toBe('typst')
    expect(meta.author).toBe('Unknown')
    expect(meta.description).toBe('Metadata test')
    expect(Array.isArray(meta.supportedFormats)).toBe(true)
    expect(meta.supportedFormats.length).toBeGreaterThan(0)
    expect(meta.schema).toBeUndefined()

    // schema: structure + ui hints. QUILL/CARD sentinels with const values.
    const schema = quill.schema
    expect(schema.main.description).toBe('The main card schema')
    expect(schema.main.fields.title).toBeDefined()
    expect(schema.main.fields.QUILL.const).toBe('meta_test_quill@0.2.1')
    expect(schema.card_types.main).toBeUndefined()
    expect(schema.card_types.indorsement.fields.signature_block).toBeDefined()
    expect(schema.card_types.indorsement.fields.CARD.const).toBe('indorsement')
  })

  it('metadata and schema are JSON.stringify-able (plain objects)', () => {
    const engine = new Quillmark()
    const quill = engine.quill(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )
    const meta = JSON.parse(JSON.stringify(quill.metadata))
    expect(meta.name).toBe('meta_test_quill')
    const schema = JSON.parse(JSON.stringify(quill.schema))
    expect(schema.main.fields.QUILL.const).toBe('meta_test_quill@0.2.1')
  })
})

describe('Document.clone', () => {
  it('returns an independent handle', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    clone.setField('title', 'Changed')

    expect(doc.main.frontmatter.title).toBe('Test Document')
    expect(clone.main.frontmatter.title).toBe('Changed')
  })

  it('preserves parse-time warnings on the clone', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    expect(clone.warnings.length).toBe(doc.warnings.length)
  })

  it('produces a clone that renders equivalently to the original', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    const r1 = quill.render(doc, { format: 'svg' })
    const r2 = quill.render(clone, { format: 'svg' })
    expect(r1.artifacts.length).toBe(r2.artifacts.length)
  })
})

// ---------------------------------------------------------------------------
// quill.form / blank_main / blank_card — schema-aware form view
// NOTE: These tests cannot run in the devcontainer (no wasm-pack / browser
//       runtime available).  They are written to run in CI where the WASM
//       bundle is built by wasm-pack and loaded into a vitest/jsdom context.
// ---------------------------------------------------------------------------

describe('quill.form', () => {
  const QUILL_YAML = `quill:
  name: form_smoke_test
  version: "1.0"
  backend: typst
  description: Smoke test for form

main:
  fields:
    title:
      type: string
      default: "Untitled"
    count:
      type: integer

card_types:
  note:
    fields:
      body:
        type: string
        default: "TBD"
      tag:
        type: string
`

  const MD_WITH_TITLE = `---
QUILL: form_smoke_test
title: "Hello"
---
`

  it('form returns a plain object with main, cards, diagnostics', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'form_smoke_test', quillYaml: QUILL_YAML }))
    const doc = Document.fromMarkdown(MD_WITH_TITLE)

    const form = quill.form(doc)

    expect(typeof form).toBe('object')
    expect(form).not.toBeNull()
    expect('main' in form).toBe(true)
    expect('cards' in form).toBe(true)
    expect('diagnostics' in form).toBe(true)
    expect(Array.isArray(form.cards)).toBe(true)
    expect(Array.isArray(form.diagnostics)).toBe(true)
  })

  it('form main.values has correct sources', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'form_smoke_test', quillYaml: QUILL_YAML }))
    const doc = Document.fromMarkdown(MD_WITH_TITLE)

    const form = quill.form(doc)
    const values = form.main.values

    // title is present in doc → source: document
    expect(values.title).toBeDefined()
    expect(values.title.source).toBe('document')
    expect(values.title.value).toBe('Hello')

    // count is absent but schema has no default → source: missing
    expect(values.count).toBeDefined()
    expect(values.count.source).toBe('missing')
    expect(values.count.value).toBeNull()
  })

  it('form result is JSON.stringify-able and round-trips', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'form_smoke_test', quillYaml: QUILL_YAML }))
    const doc = Document.fromMarkdown(MD_WITH_TITLE)

    const form = quill.form(doc)
    const json = JSON.stringify(form)
    expect(typeof json).toBe('string')
    expect(json.length).toBeGreaterThan(0)

    const parsed = JSON.parse(json)
    expect(parsed.main.values.title.source).toBe('document')
  })

  it('blankMain returns a card with no document values', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'form_smoke_test', quillYaml: QUILL_YAML }))

    const blank = quill.blankMain()

    expect(typeof blank).toBe('object')
    expect(blank).not.toBeNull()
    // title has a default
    expect(blank.values.title.source).toBe('default')
    expect(blank.values.title.value).toBeNull()
    expect(blank.values.title.default).toBe('Untitled')
    // count has no default
    expect(blank.values.count.source).toBe('missing')
    expect(blank.values.count.value).toBeNull()
    expect(blank.values.count.default).toBeNull()
  })

  it('blankCard returns a card for a known type', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'form_smoke_test', quillYaml: QUILL_YAML }))

    const blank = quill.blankCard('note')

    expect(blank).not.toBeNull()
    expect(blank.values.body.source).toBe('default')
    expect(blank.values.body.default).toBe('TBD')
    expect(blank.values.tag.source).toBe('missing')
  })

  it('blankCard returns null for an unknown type', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'form_smoke_test', quillYaml: QUILL_YAML }))

    expect(quill.blankCard('does_not_exist')).toBeNull()
  })
})
