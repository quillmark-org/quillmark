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

/** Read a field value from a card's payloadItems list by key. */
const field = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

/** True when a field key is absent from a card's payloadItems. */
const hasField = (card, key) =>
  card.payloadItems.some((i) => i.type === 'field' && i.key === key)

const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is a test document.`

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let body = data.at("$body")

= #title

#body`

describe('Document.fromMarkdown', () => {
  it('should parse markdown with YAML payload', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(doc).toBeDefined()
    expect(doc.quillRef).toBe('test_quill')
  })

  it('should expose typed payload (no $quill / $body / $cards as fields)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(field(doc.main, 'title')).toBe('Test Document')
    expect(field(doc.main, 'author')).toBe('Test Author')
    // $-prefixed system metadata must NOT appear as payload fields
    expect(hasField(doc.main, 'quill')).toBe(false)
    expect(hasField(doc.main, '$quill')).toBe(false)
    expect(hasField(doc.main, '$body')).toBe(false)
    expect(hasField(doc.main, '$cards')).toBe(false)
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
    const md = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Global body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card body.
`
    const doc = Document.fromMarkdown(md)

    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(field(doc.cards[0], 'foo')).toBe('bar')
    expect(doc.cards[0].body).toContain('Card body.')
  })

  it('should expose warnings array', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(Array.isArray(doc.warnings)).toBe(true)
    expect(doc.warnings.length).toBe(0)
  })

  it('should throw on invalid YAML payload', () => {
    const badMarkdown = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test
this is not valid yaml
~~~

# Content`

    expect(() => {
      Document.fromMarkdown(badMarkdown)
    }).toThrow()
  })

  it('should throw when $quill metadata is absent', () => {
    const markdownWithoutQuill = `~~~card-yaml
title: Default Test
author: Test Author
~~~

# Hello Default

This document has no $quill metadata.`

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
    doc.pushCard({ kind: 'note', fields: { author: 'Alice' }, body: 'Hello' })
    doc.replaceBody('Updated body')

    // Emit
    const emitted = doc.toMarkdown()
    expect(typeof emitted).toBe('string')
    expect(emitted.length).toBeGreaterThan(0)

    // Re-parse and assert structure survives.
    //
    // Note on trailing newlines: the global body is followed by a card fence,
    // so the wire format inserts a line terminator + F2 blank line between
    // them (`Updated body\n\n~~~card-yaml`). On re-parse the F2 blank is
    // stripped but the terminator stays, so `doc2.main.body === 'Updated body\n'`. The card
    // body is at EOF and has no F2 separator, so it survives byte-for-byte.
    const doc2 = Document.fromMarkdown(emitted)
    expect(field(doc2.main, 'title')).toBe('New Title')
    expect(doc2.main.body).toBe('Updated body\n')
    expect(doc2.cards.length).toBe(originalCardCount + 1)
    expect(doc2.cards[0].kind).toBe('note')
    expect(field(doc2.cards[0], 'author')).toBe('Alice')
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
    expect(field(doc2.main, 'flag_on')).toBe('on')
    expect(field(doc2.main, 'flag_off')).toBe('off')
    expect(field(doc2.main, 'flag_yes')).toBe('yes')
    expect(field(doc2.main, 'flag_no')).toBe('no')
    expect(field(doc2.main, 'str_true')).toBe('true')
    expect(field(doc2.main, 'str_false')).toBe('false')
    expect(field(doc2.main, 'str_null')).toBe('null')
    expect(field(doc2.main, 'octal_str')).toBe('01234')
    expect(field(doc2.main, 'date_str')).toBe('2024-01-15')
  })
})

// ---------------------------------------------------------------------------
// Document.toJson / Document.fromJson — versioned storage DTO round-trip
// ---------------------------------------------------------------------------

describe('Document JSON DTO — toJson / fromJson', () => {
  it('toJson emits a string carrying the schema version', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const dto = doc.toJson()
    expect(typeof dto).toBe('string')
    expect(dto).toContain('quillmark/document@0.82.0')
  })

  it('round-trips losslessly: fromJson(toJson(doc)) equals doc', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const restored = Document.fromJson(doc.toJson())
    expect(restored.equals(doc)).toBe(true)
  })

  it('round-trips a mutated document with cards', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('title', 'New Title')
    doc.pushCard({ kind: 'note', fields: { author: 'Alice' }, body: 'Hello' })

    const restored = Document.fromJson(doc.toJson())

    expect(restored.equals(doc)).toBe(true)
    expect(field(restored.main, 'title')).toBe('New Title')
    expect(restored.cards[0].kind).toBe('note')
    expect(field(restored.cards[0], 'author')).toBe('Alice')
    expect(restored.cards[0].body).toBe('Hello')
  })

  it('toJson output is standard JSON parseable by the JSON global', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const parsed = JSON.parse(doc.toJson())
    expect(parsed.schema).toBe('quillmark/document@0.82.0')
  })

  it('drops parse-time warnings on reconstruction', () => {
    // An unknown YAML tag triggers a `parse::unsupported_yaml_tag` warning.
    const warnMd =
      '~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hi\nweird: !custom value\n~~~\n\nBody\n'
    const doc = Document.fromMarkdown(warnMd)
    expect(doc.warnings.length).toBeGreaterThan(0)

    const restored = Document.fromJson(doc.toJson())
    expect(restored.warnings.length).toBe(0)
  })

  it('fromJson rejects an unknown schema version', () => {
    expect(() =>
      Document.fromJson('{"schema":"quillmark/document@0.99.0","main":{}}'),
    ).toThrow()
  })

  it('fromJson rejects malformed JSON', () => {
    expect(() => Document.fromJson('not json at all')).toThrow()
  })

  it('toJson is deterministic across repeated calls', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.toJson()).toBe(doc.toJson())
  })

  it('toJson is byte-identical for equal documents', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromJson(a.toJson())
    expect(b.equals(a)).toBe(true)
    expect(b.toJson()).toBe(a.toJson())
  })

  it('tryFromJson returns a Document for a valid DTO', () => {
    const dto = Document.fromMarkdown(TEST_MARKDOWN).toJson()
    const restored = Document.tryFromJson(dto)
    expect(restored).toBeDefined()
    expect(restored.equals(Document.fromMarkdown(TEST_MARKDOWN))).toBe(true)
  })

  it('tryFromJson returns undefined for non-DTO input instead of throwing', () => {
    expect(Document.tryFromJson('not json at all')).toBeUndefined()
    expect(
      Document.tryFromJson('{"schema":"quillmark/document@0.99.0","main":{}}'),
    ).toBeUndefined()
    expect(Document.tryFromJson(TEST_MARKDOWN)).toBeUndefined()
  })

  it('currentSchemaVersion matches what toJson writes', () => {
    const dto = JSON.parse(Document.fromMarkdown(TEST_MARKDOWN).toJson())
    expect(dto.schema).toBe(Document.currentSchemaVersion())
  })

  it('schemaVersionOf reads the schema field from any object payload', () => {
    const current = Document.fromMarkdown(TEST_MARKDOWN).toJson()
    expect(Document.schemaVersionOf(current)).toBe(
      Document.currentSchemaVersion(),
    )

    // Future versions are returned as-is, even though fromJson would reject.
    expect(
      Document.schemaVersionOf(
        '{"schema":"quillmark/document@0.99.0","main":{}}',
      ),
    ).toBe('quillmark/document@0.99.0')

    expect(Document.schemaVersionOf('not json')).toBeUndefined()
    expect(Document.schemaVersionOf('{"foo":"bar"}')).toBeUndefined()
    expect(Document.schemaVersionOf(TEST_MARKDOWN)).toBeUndefined()
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

  it('should emit a quill::ref_mismatch warning when the document quill ref differs from the quill name', () => {
    const engine = new Quillmark()
    const quill = engine.quill(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))

    // Document declares a different quill name
    const otherMarkdown = `~~~card-yaml
$quill: other_quill
$kind: main
title: Mismatch Test
~~~

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
  it('setField inserts a new payload field', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('subtitle', 'A subtitle')
    expect(field(doc.main, 'subtitle')).toBe('A subtitle')
  })

  it('setField updates an existing field', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('title', 'Updated')
    expect(field(doc.main, 'title')).toBe('Updated')
  })

  it('setField throws EditError::InvalidFieldName for legacy uppercase names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['BODY', 'CARDS', 'QUILL', 'CARD']) {
      expect(() => doc.setField(name, 'x')).toThrow(/InvalidFieldName/)
    }
  })

  it('setField throws EditError::InvalidFieldName for `$`-prefixed names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['$body', '$cards', '$quill', '$kind']) {
      expect(() => doc.setField(name, 'x')).toThrow(/InvalidFieldName/)
    }
  })

  it('setField throws EditError::InvalidFieldName for uppercase name', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('Title', 'x')).toThrow(/InvalidFieldName/)
  })

  it('removeField returns the removed value', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const removed = doc.removeField('title')
    expect(removed).toBe('Test Document')
    expect(hasField(doc.main, 'title')).toBe(false)
  })

  it('removeField returns undefined when field absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.removeField('nonexistent')).toBeUndefined()
  })

  it('removeField throws EditError::InvalidFieldName for legacy uppercase names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['BODY', 'CARDS', 'QUILL', 'CARD']) {
      expect(() => doc.removeField(name)).toThrow(/InvalidFieldName/)
    }
  })

  it('removeField throws EditError::InvalidFieldName for `$`-prefixed names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['$body', '$cards', '$quill', '$kind']) {
      expect(() => doc.removeField(name)).toThrow(/InvalidFieldName/)
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
  const MD_WITH_CARDS = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card one.

~~~card-yaml
$kind: summary
~~~

Card two.
`

  it('pushCard appends a card', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.pushCard({ kind: 'note', fields: {}, body: 'My card.' })
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(doc.cards[0].body).toBe('My card.')
  })

  it('pushCard throws on invalid kind', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.pushCard({ kind: 'BadKind' })).toThrow(/InvalidKindName/)
  })

  it('insertCard inserts at specified index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.insertCard(0, { kind: 'intro' })
    expect(doc.cards[0].kind).toBe('intro')
    expect(doc.cards[1].kind).toBe('note')
  })

  it('insertCard throws IndexOutOfRange when index > len', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expect(() => doc.insertCard(5, { kind: 'note' })).toThrow(/IndexOutOfRange/)
  })

  it('removeCard removes and returns the card', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    const removed = doc.removeCard(0)
    expect(removed).toBeDefined()
    expect(removed.kind).toBe('note')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('summary')
  })

  it('removeCard returns undefined when out of range', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.removeCard(0)).toBeUndefined()
  })

  it('moveCard swaps positions correctly', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(1, 0) // summary → front
    expect(doc.cards[0].kind).toBe('summary')
    expect(doc.cards[1].kind).toBe('note')
  })

  it('moveCard no-op when from == to', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(0, 0)
    expect(doc.cards[0].kind).toBe('note')
  })

  it('moveCard throws IndexOutOfRange on out-of-range index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // 2 cards
    expect(() => doc.moveCard(5, 0)).toThrow(/IndexOutOfRange/)
  })

  it('setCardKind renames the kind in place', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.setCardKind(0, 'annotation')
    expect(doc.cards[0].kind).toBe('annotation')
    // Payload items preserved across rename.
    expect(Array.isArray(doc.cards[0].payloadItems)).toBe(true)
  })

  it('setCardKind throws InvalidKindName for empty/uppercase/dashed kinds', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    for (const bad of ['', 'BadKind', 'with-dash']) {
      expect(() => doc.setCardKind(0, bad)).toThrow(/InvalidKindName/)
    }
  })

  it('setCardKind throws IndexOutOfRange when index >= len', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // 2 cards
    expect(() => doc.setCardKind(5, 'annotation')).toThrow(/IndexOutOfRange/)
  })

  it('cardCount reports composable card count without allocating', () => {
    const empty = Document.fromMarkdown(TEST_MARKDOWN)
    expect(empty.cardCount).toBe(0)

    const two = Document.fromMarkdown(MD_WITH_CARDS)
    expect(two.cardCount).toBe(2)
    two.pushCard({ kind: 'extra' })
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

  it('returns false after a payload mutation', () => {
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
    b.pushCard({ kind: 'note' })
    expect(a.equals(b)).toBe(false)
  })

  it('survives round-trip through toMarkdown / fromMarkdown', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(a.toMarkdown())
    expect(a.equals(b)).toBe(true)
  })
})

describe('Document editor surface — updateCardField / updateCardBody', () => {
  const MD_WITH_CARD = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card body.
`

  it('updateCardField sets a field on a card', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.updateCardField(0, 'content', 'hello')
    expect(field(doc.cards[0], 'content')).toBe('hello')
  })

  it('updateCardField throws EditError::InvalidFieldName for uppercase names', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    expect(() => doc.updateCardField(0, 'BODY', 'x')).toThrow(/InvalidFieldName/)
  })

  it('updateCardField throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expect(() => doc.updateCardField(0, 'title', 'x')).toThrow(/IndexOutOfRange/)
  })

  it('removeCardField returns the removed value and deletes the key', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    const removed = doc.removeCardField(0, 'foo')
    expect(removed).toBe('bar')
    expect(hasField(doc.cards[0], 'foo')).toBe(false)
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
    doc.pushCard({ kind: 'note', body: 'Card content.' })
    doc.setQuillRef('updated_quill')

    // Assert state
    expect(field(doc.main, 'author')).toBe('Bob')
    expect(doc.main.body).toBe('New body text.')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(doc.cards[0].body).toBe('Card content.')
    expect(doc.quillRef).toBe('updated_quill')

    // Original title still present
    expect(field(doc.main, 'title')).toBe('Test Document')

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

card_kinds:
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

    // schema: user-fillable fields + ui hints. No QUILL/CARD sentinels.
    const schema = quill.schema
    expect(schema.main.description).toBe('The main card schema')
    expect(schema.main.fields.title).toBeDefined()
    expect(schema.main.fields.QUILL).toBeUndefined()
    expect(schema.card_kinds.main).toBeUndefined()
    expect(schema.card_kinds.indorsement.fields.signature_block).toBeDefined()
    expect(schema.card_kinds.indorsement.fields.CARD).toBeUndefined()
  })

  it('metadata and schema are JSON.stringify-able (plain objects)', () => {
    const engine = new Quillmark()
    const quill = engine.quill(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )
    const meta = JSON.parse(JSON.stringify(quill.metadata))
    expect(meta.name).toBe('meta_test_quill')
    const schema = JSON.parse(JSON.stringify(quill.schema))
    expect(schema.main.fields.title).toBeDefined()
    expect(schema.main.fields.QUILL).toBeUndefined()
  })
})

describe('Document.clone', () => {
  it('returns an independent handle', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    clone.setField('title', 'Changed')

    expect(field(doc.main, 'title')).toBe('Test Document')
    expect(field(clone.main, 'title')).toBe('Changed')
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

card_kinds:
  note:
    fields:
      body:
        type: string
        default: "TBD"
      tag:
        type: string
`

  const MD_WITH_TITLE = `~~~card-yaml
$quill: form_smoke_test
$kind: main
title: "Hello"
~~~
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

// ---------------------------------------------------------------------------
// Schema / blueprint / validation — Must Fill vs Endorsed
// ---------------------------------------------------------------------------
//
// Post-mcp-feedback the schema axis is implicit: a field with a `default:` is
// Endorsed (the rendered default is shippable as-is and the blueprint emits
// the value + `; delete-ok` annotation); a field without a `default:` is Must
// Fill (the blueprint emits the `<must-fill>` sentinel).
//
// These tests pin the JS-facing contract:
//   - `QuillFieldSchema` no longer carries a `required` axis.
//   - `quill.blueprint` carries `<must-fill>` and `; delete-ok` annotations.
//   - `quill.render(doc)` *tolerates* an absent Must Fill field: zero-filled
//     render fills it with its type-empty value in the plate projection
//     (never persisted), so absence is not a render error.
//   - `quill.render(doc)` raises `validation::must_fill_sentinel` when the
//     `<must-fill>` sentinel is left in (malformed, always fatal).
//   - `quill.form(doc)` still flags both situations under `diagnostics`
//     (the form view reports completeness independent of the render gate).
//
// See prose/proposals/zero-filled-render.md.

describe('Must Fill / Endorsed schema model', () => {
  // The plate `unwrap`s `data.title` (Must Fill) and substitutes the optional
  // `data.subtitle` if present. Authoring a quill with both Must Fill and
  // Endorsed fields lets us exercise both validation codes without having to
  // ship two separate test quills.
  const SCHEMA_QUILL_YAML = `quill:
  name: schema_test
  version: "1.0"
  backend: typst
  plate_file: plate.typ
  description: Must Fill / Endorsed coverage

main:
  fields:
    title:
      type: string
      description: Document title (Must Fill — no default)
    subtitle:
      type: string
      default: "Untitled subtitle"
      description: Document subtitle (Endorsed — default shippable)
`

  const SCHEMA_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let subtitle = data.at("subtitle", default: "")
#let body = data.at("$body")

= #title

#subtitle

#body`

  const buildQuill = () => {
    const engine = new Quillmark()
    return engine.quill(
      makeQuill({
        name: 'schema_test',
        plate: SCHEMA_PLATE,
        quillYaml: SCHEMA_QUILL_YAML,
      }),
    )
  }

  it('schema fields carry no legacy `required` axis', () => {
    const quill = buildQuill()
    const fields = quill.schema.main.fields

    expect(fields.title).toBeDefined()
    expect(fields.subtitle).toBeDefined()

    // The `required` axis was removed; cell is implied by `default:` presence.
    expect('required' in fields.title).toBe(false)
    expect('required' in fields.subtitle).toBe(false)

    // Must Fill fields have no `default`; Endorsed fields do.
    expect(fields.title.default).toBeUndefined()
    expect(fields.subtitle.default).toBe('Untitled subtitle')
  })

  it('blueprint carries `<must-fill>` for Must Fill fields and `; delete-ok` for Endorsed', () => {
    const quill = buildQuill()
    const blueprint = quill.blueprint

    expect(typeof blueprint).toBe('string')
    expect(blueprint.length).toBeGreaterThan(0)

    // Must Fill: value cell is the literal sentinel; no `; delete-ok` tag.
    expect(blueprint).toContain('title: <must-fill>  # string')
    expect(blueprint).not.toMatch(/title: <must-fill>.*delete-ok/)

    // Endorsed: rendered default + `; delete-ok` tag. The emitter does not
    // quote strings that don't need quoting (`Untitled subtitle` has no YAML
    // ambiguity), so the value cell is bare.
    expect(blueprint).toContain('subtitle: Untitled subtitle  # string; delete-ok')

    // The legacy `; required` / `; optional` role tag must not appear anywhere.
    expect(blueprint).not.toContain('; required')
    expect(blueprint).not.toContain('; optional')
  })

  it('render tolerates an absent Must Fill field (zero-filled, not an error)', () => {
    const quill = buildQuill()

    // Document omits `title`. Schema declares no default → Must Fill. Under
    // zero-filled render this is merely *incomplete*, not malformed: render
    // fills `title` with its type-empty value in the plate projection and
    // succeeds. Absence is no longer a hard error (the form's `source:
    // "missing"` carries the doneness signal instead).
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
subtitle: "Just a subtitle"
~~~

# Body
`
    const doc = Document.fromMarkdown(md)

    const result = quill.render(doc, { format: 'svg' })
    expect(result).toBeDefined()
    expect(Array.isArray(result.artifacts)).toBe(true)
    expect(result.artifacts.length).toBeGreaterThan(0)
  })

  it('render throws `validation::must_fill_sentinel` when the `<must-fill>` sentinel is left in', () => {
    const quill = buildQuill()

    // Document supplies the literal sentinel — the LLM forgot to fill it.
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title: <must-fill>
~~~

# Body
`
    const doc = Document.fromMarkdown(md)

    try {
      quill.render(doc, { format: 'svg' })
      throw new Error('render should have thrown ValidationFailed')
    } catch (err) {
      expect(Array.isArray(err.diagnostics)).toBe(true)
      const codes = err.diagnostics.map((d) => d.code)
      expect(codes).toContain('validation::must_fill_sentinel')
      const placeholder = err.diagnostics.find(
        (d) => d.code === 'validation::must_fill_sentinel',
      )
      expect(placeholder.path).toBe('title')
      expect(placeholder.severity).toBe('error')
      // Hint nudges the caller toward the action they need to take.
      expect(placeholder.hint).toBeDefined()
      expect(placeholder.hint).toContain('<must-fill>')
    }
  })

  it('render succeeds when every Must Fill field is supplied with a real value', () => {
    const quill = buildQuill()

    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title: "A Real Title"
~~~

# Body
`
    const doc = Document.fromMarkdown(md)
    const result = quill.render(doc, { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
  })

  it('form surfaces validation diagnostics for both absent and sentinel cases', () => {
    const quill = buildQuill()

    // Case 1: `title` absent — form should flag it.
    const mdAbsent = `~~~card-yaml
$quill: schema_test
$kind: main
subtitle: "Just a subtitle"
~~~
`
    const formAbsent = quill.form(Document.fromMarkdown(mdAbsent))
    // `title` is missing in the document, and the schema declares no default,
    // so the form view marks the value as `missing`.
    expect(formAbsent.main.values.title.source).toBe('missing')
    expect(formAbsent.main.values.title.default).toBeNull()
    // The validation error surfaces under `diagnostics` with its canonical
    // `validation::*` code, path, and hint — the form view forwards the
    // structured diagnostic verbatim.
    expect(formAbsent.diagnostics.length).toBeGreaterThan(0)
    expect(
      formAbsent.diagnostics.some(
        (d) => d.severity === 'error' && d.path === 'title',
      ),
    ).toBe(true)

    // Case 2: sentinel left in — form should also flag it.
    const mdSentinel = `~~~card-yaml
$quill: schema_test
$kind: main
title: <must-fill>
~~~
`
    const formSentinel = quill.form(Document.fromMarkdown(mdSentinel))
    // The sentinel is a literal string value, so the source is `document`.
    expect(formSentinel.main.values.title.source).toBe('document')
    expect(formSentinel.main.values.title.value).toBe('<must-fill>')
    expect(
      formSentinel.diagnostics.some(
        (d) =>
          d.severity === 'error' &&
          d.path === 'title' &&
          typeof d.hint === 'string' &&
          d.hint.includes('<must-fill>'),
      ),
    ).toBe(true)
  })
})
