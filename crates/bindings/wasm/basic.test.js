/**
 * Smoke tests for quillmark-wasm — Document API
 *
 * These tests cover the canonical flow:
 *   Quill.fromTree(tree) → Document.fromMarkdown(markdown) → engine.render(quill, doc, opts)
 *
 * Setup: Tests use the bundler build via @quillmark-wasm alias (see vitest.config.js)
 */

import { describe, it, expect } from 'vitest'
import { Quillmark, Quill, Document } from '@quillmark-wasm'
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

  it('should expose body as a corpus with a markdown projection', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // `body` is the canonical corpus (source-of-truth model); `bodyMarkdown` is
    // the markdown projection.
    expect(typeof doc.main.body).toBe('object')
    expect(typeof doc.main.body.text).toBe('string')
    expect(doc.main.body.text).toContain('Hello World')
    expect(typeof doc.main.bodyMarkdown).toBe('string')
    expect(doc.main.bodyMarkdown).toContain('Hello World')
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
    expect(doc.cards[0].bodyMarkdown).toContain('Card body.')
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
// Document.toMarkdown — emitter integration tests
// ---------------------------------------------------------------------------

describe('Document.toMarkdown — fromMarkdown → mutate → emit → re-parse', () => {
  it('general round-trip: mutated document survives emit → re-parse', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const originalCardCount = doc.cards.length  // 0 for TEST_MARKDOWN

    // Mutate
    doc.setField('title', 'New Title')
    doc.pushCard(Document.makeCard('note', { author: 'Alice' }, 'Hello'))
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
    // stripped but the terminator stays, so `doc2.main.bodyMarkdown === 'Updated body\n'`. The card
    // body is at EOF and has no F2 separator, so it survives byte-for-byte.
    const doc2 = Document.fromMarkdown(emitted)
    expect(field(doc2.main, 'title')).toBe('New Title')
    expect(doc2.main.bodyMarkdown).toBe('Updated body\n')
    expect(doc2.cards.length).toBe(originalCardCount + 1)
    expect(doc2.cards[0].kind).toBe('note')
    expect(field(doc2.cards[0], 'author')).toBe('Alice')
    expect(doc2.cards[0].bodyMarkdown).toBe('Hello\n')
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
    expect(dto).toContain('quillmark/document@0.93.0')
  })

  it('round-trips losslessly: fromJson(toJson(doc)) equals doc', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const restored = Document.fromJson(doc.toJson())
    expect(restored.equals(doc)).toBe(true)
  })

  it('round-trips a mutated document with cards', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('title', 'New Title')
    doc.pushCard(Document.makeCard('note', { author: 'Alice' }, 'Hello'))

    const restored = Document.fromJson(doc.toJson())

    expect(restored.equals(doc)).toBe(true)
    expect(field(restored.main, 'title')).toBe('New Title')
    expect(restored.cards[0].kind).toBe('note')
    expect(field(restored.cards[0], 'author')).toBe('Alice')
    expect(restored.cards[0].bodyMarkdown).toBe('Hello\n')
  })

  it('toJson output is standard JSON parseable by the JSON global', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const parsed = JSON.parse(doc.toJson())
    expect(parsed.schema).toBe('quillmark/document@0.93.0')
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

  it('fromJson accepts a stored DTO with an uppercase field name', () => {
    // Regression: uppercase data-field names (e.g. PRESENTATION) are valid
    // user fields — only `$`-prefixed keys are reserved — so a stored DTO
    // carrying one must deserialize and round-trip verbatim.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setField('PRESENTATION', 'deck')
    const restored = Document.fromJson(doc.toJson())
    expect(field(restored.main, 'PRESENTATION')).toBe('deck')
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
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    expect(quill).toBeDefined()
  })

  it('should accept a plain object tree (Record<string, Uint8Array>)', () => {
    const engine = new Quillmark()
    const mapTree = makeQuill({ name: 'test_quill', plate: TEST_PLATE })
    const objectTree = Object.fromEntries(mapTree)

    const fromMap = Quill.fromTree(mapTree)
    const fromObject = Quill.fromTree(objectTree)

    expect(fromMap.backendId).toBe(fromObject.backendId)

    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const r1 = engine.render(fromMap, doc, { format: 'svg' })
    const r2 = engine.render(fromObject, doc, { format: 'svg' })
    expect(r1.artifacts.length).toBe(r2.artifacts.length)
  })

  it('should reject non-object trees with a clear error', () => {
    expect(() => Quill.fromTree(42)).toThrow()
    expect(() => Quill.fromTree('string')).toThrow()
    expect(() => Quill.fromTree(null)).toThrow()
  })

  it('should render markdown to PDF via quill.render(doc) with default opts', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = engine.render(quill, doc)

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
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = engine.render(quill, doc, { format: 'pdf' })

    expect(result).toBeDefined()
    expect(result.artifacts).toBeDefined()
    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)
    expect(result.artifacts[0].mimeType).toBe('application/pdf')
  })

  it('should render markdown to SVG via quill.render(doc)', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = engine.render(quill, doc, { format: 'svg' })

    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.artifacts[0].mimeType).toBe('image/svg+xml')
  })

  it('should allow rendering the same Document multiple times', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const pdf = engine.render(quill, doc, { format: 'pdf' })
    const svg = engine.render(quill, doc, { format: 'svg' })

    expect(pdf.artifacts[0].mimeType).toBe('application/pdf')
    expect(svg.artifacts[0].mimeType).toBe('image/svg+xml')
  })

  it('session.regions() is always a non-null array', () => {
    // Regions are a session-level query, not on the render result. The document
    // body is a markdown content field, so it auto-tags one schema-field region
    // keyed `$body`; the result is always an array, never undefined.
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = engine.open(quill, doc)
    const regions = session.regions()
    expect(Array.isArray(regions)).toBe(true)
    expect(regions.some((r) => r.field === '$body')).toBe(true)
    session.free()
  })

  it('should throw a quill::name_mismatch error when the document quill ref differs from the quill name', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))

    // Document declares a different quill name
    const otherMarkdown = `~~~card-yaml
$quill: other_quill
$kind: main
title: Mismatch Test
~~~

# Content`
    const doc = Document.fromMarkdown(otherMarkdown)

    try {
      engine.render(quill, doc, { format: 'pdf' })
      throw new Error('render should have thrown on a $quill name mismatch')
    } catch (err) {
      expect(Array.isArray(err.diagnostics)).toBe(true)
      expect(err.diagnostics[0].code).toBe('quill::name_mismatch')
    }
  })
})

// ---------------------------------------------------------------------------
// Document editor surface
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

  it('setField accepts uppercase field names verbatim (lowercase is canonical, not enforced)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['BODY', 'CARDS', 'Title', 'MixedCase_1']) {
      doc.setField(name, 'x')
      expect(field(doc.main, name)).toBe('x')
    }
  })

  it('setField throws EditError::InvalidFieldName for `$`-prefixed names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['$body', '$cards', '$quill', '$kind']) {
      expect(() => doc.setField(name, 'x')).toThrow(/InvalidFieldName/)
    }
  })

  it('setField throws EditError::InvalidFieldName for an invalid name (hyphen)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setField('bad-name', 'x')).toThrow(/InvalidFieldName/)
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

  it('removeField throws EditError::InvalidFieldName for `$`-prefixed names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['$body', '$cards', '$quill', '$kind']) {
      expect(() => doc.removeField(name)).toThrow(/InvalidFieldName/)
    }
  })
})

describe('Document blank-canvas constructor', () => {
  it('new Document(quillRef) starts blank and builds up', () => {
    const doc = new Document('test_quill')
    expect(doc.quillRef).toBe('test_quill')
    expect(doc.cards.length).toBe(0)
    expect(doc.main.bodyMarkdown).toBe('')
    doc.setFields({ title: 'Hello' })
    expect(field(doc.main, 'title')).toBe('Hello')
  })

  it('throws on an invalid quill reference', () => {
    expect(() => new Document('not a valid ref!!')).toThrow(/QuillReference/)
  })
})

describe('Document editor surface — setFields / updateCardFields', () => {
  it('setFields applies every entry, in object order', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setFields({ subtitle: 'A subtitle', pages: 3 })
    expect(field(doc.main, 'subtitle')).toBe('A subtitle')
    expect(field(doc.main, 'pages')).toBe(3)
  })

  it('a failed batch throws one diagnostic per bad field and applies nothing', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    try {
      doc.setFields({ ok_field: 'v', 'bad-name': 'v', 'also bad': 'v' })
      throw new Error('setFields should have thrown')
    } catch (err) {
      expect(err.diagnostics.map((d) => d.path)).toEqual(['bad-name', 'also bad'])
      expect(err.message).toMatch(/InvalidFieldName/)
    }
    expect(hasField(doc.main, 'ok_field')).toBe(false)
  })

  it('setFields rejects a non-object argument', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setFields('not an object')).toThrow(/plain object/)
  })

  it('updateCardFields is the card-indexed twin of setFields', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.pushCard(Document.makeCard('note', { foo: 'bar' }))
    doc.updateCardFields(0, { foo: 'baz', extra: 1 })
    expect(field(doc.cards[0], 'foo')).toBe('baz')
    expect(field(doc.cards[0], 'extra')).toBe(1)
    expect(() => doc.updateCardFields(99, { foo: 'v' })).toThrow(/IndexOutOfRange/)
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
    expect(doc.main.bodyMarkdown).toBe('Brand new body.\n')
  })

  it('setBody accepts a markdown string', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setBody('Body from **markdown**.')
    expect(doc.main.bodyMarkdown).toBe('Body from **markdown**.\n')
  })

  it('setBody accepts a corpus object round-tripped from doc.main.body', () => {
    // The corpus is the source-of-truth shape doc.main.body reads back; writing
    // it straight through is the read/write symmetry #874 closes.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const src = Document.fromMarkdown('~~~card-yaml\n$quill: q@1.0.0\n~~~\n\nCorpus **body** here.')
    const corpus = src.main.body
    expect(typeof corpus).toBe('object')
    doc.setBody(corpus)
    expect(doc.main.body.text).toBe('Corpus body here.')
    expect(doc.main.bodyMarkdown).toBe('Corpus **body** here.\n')
  })

  it('setBody(null) clears the body', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setBody(null)
    expect(doc.main.bodyMarkdown).toBe('')
  })

  it('setBody throws BodyDecode on a non-corpus object', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setBody({ not: 'a corpus' })).toThrow(/BodyDecode/)
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
    doc.pushCard(Document.makeCard('note', {}, 'My card.'))
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(doc.cards[0].bodyMarkdown).toBe('My card.\n')
  })

  it('pushCard throws on invalid kind', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.pushCard({ kind: 'BadKind' })).toThrow(/InvalidKindName/)
  })

  it('removeCard → pushCard round-trips a card with fields (read shape == write shape)', () => {
    // The whole point of the one-Card-shape change: a card returned by
    // removeCard feeds straight back into pushCard with its fields intact.
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // `note` (foo: bar) + `summary`
    const initialCount = doc.cards.length
    const removed = doc.removeCard(0) // the `note` card
    expect(doc.cards.length).toBe(initialCount - 1)
    expect(field(removed, 'foo')).toBe('bar')

    doc.pushCard(removed) // re-push the returned card; fields must not drop
    expect(doc.cards.length).toBe(initialCount)
    const repushed = doc.cards[doc.cards.length - 1]
    expect(repushed.kind).toBe('note')
    expect(field(repushed, 'foo')).toBe('bar')
  })

  it('makeCard accepts any kind; pushCard is the kind gate', () => {
    // makeCard is pure data-shaping (permissive); the cards-list invariant is
    // enforced at insertion, not construction.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const bad = Document.makeCard('BadKind', { x: 1 })
    expect(bad.kind).toBe('BadKind') // construction succeeds
    expect(() => doc.pushCard(bad)).toThrow(/InvalidKindName/) // insertion rejects
  })

  it('makeCard treats fields and body as optional', () => {
    // Both `fields` and `body` are omittable; a bare kind yields an empty card.
    // The `.d.ts` marks them `fields?` / `body?` to match (see makeCard's
    // unchecked_optional_param_type bindings).
    const bare = Document.makeCard('note')
    expect(bare.kind).toBe('note')
    expect(bare.payloadItems).toEqual([])
    expect(bare.bodyMarkdown).toBe('')
  })

  it('a stale { kind, fields } object is a loud error, not a silent empty card', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.pushCard({ kind: 'note', fields: { x: 1 } })).toThrow()
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

  it('updateCardField accepts uppercase names verbatim', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.updateCardField(0, 'BODY', 'x')
    expect(field(doc.cards[0], 'BODY')).toBe('x')
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
    expect(doc.cards[0].bodyMarkdown).toBe('New card body.\n')
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
    expect(doc.main.bodyMarkdown).toBe('New body text.\n')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(doc.cards[0].bodyMarkdown).toBe('Card content.\n')
    expect(doc.quillRef).toBe('updated_quill')

    // Original title still present
    expect(field(doc.main, 'title')).toBe('Test Document')

    // Warnings untouched
    expect(Array.isArray(doc.warnings)).toBe(true)
  })
})

describe('Document editor surface — $ext mutators', () => {
  it('setExt adds an opaque map readable via card.ext', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setExt({ editor: { title: 'Greeting' } })
    expect(doc.main.ext.editor.title).toBe('Greeting')
  })

  it('setExt rejects non-object values', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setExt('nope')).toThrow(/must be a plain object/)
    expect(() => doc.setExt(42)).toThrow(/must be a plain object/)
  })

  it('$ext round-trips through toMarkdown', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setExt({ agent: { pinned: true } })
    const reparsed = Document.fromMarkdown(doc.toMarkdown())
    expect(reparsed.main.ext.agent.pinned).toBe(true)
  })

  it('setExtNamespace preserves sibling namespaces', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setExtNamespace('editor', { title: 'A' })
    doc.setExtNamespace('agent', { pinned: true })
    doc.setExtNamespace('editor', { title: 'B' })
    expect(doc.main.ext.editor.title).toBe('B')
    expect(doc.main.ext.agent.pinned).toBe(true)
  })

  it('removeExtNamespace clears one slot and drops $ext once empty', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setExtNamespace('editor', { title: 'A' })
    doc.setExtNamespace('tutorial', ['step-1', 'step-2'])
    // Returns the removed value; siblings survive.
    expect(doc.removeExtNamespace('tutorial')).toEqual(['step-1', 'step-2'])
    expect(doc.main.ext.editor.title).toBe('A')
    expect(doc.main.ext.tutorial).toBeUndefined()
    // Removing the last namespace clears $ext entirely.
    doc.removeExtNamespace('editor')
    expect(doc.main.ext == null).toBe(true)
    // Absent namespace is a no-op returning undefined.
    expect(doc.removeExtNamespace('nope')).toBeUndefined()
  })

  it('removeExt returns the previous map and clears it', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setExt({ agent: { n: 1 } })
    expect(doc.removeExt().agent.n).toBe(1)
    expect(doc.main.ext == null).toBe(true)
    expect(doc.removeExt()).toBeUndefined()
  })

  it('card-level ext mutators target the card at index', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.pushCard({ kind: 'note', body: 'x' })
    doc.setCardExt(0, { agent: { note: 'y' } })
    expect(doc.cards[0].ext.agent.note).toBe('y')
    expect(doc.removeCardExt(0).agent.note).toBe('y')
    expect(doc.cards[0].ext == null).toBe(true)
  })

  it('card-level namespace mutators preserve siblings and clear when empty', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.pushCard({ kind: 'note', body: 'x' })
    doc.setCardExtNamespace(0, 'editor', { title: 'A' })
    doc.setCardExtNamespace(0, 'tutorial', ['step-1'])
    expect(doc.removeCardExtNamespace(0, 'tutorial')).toEqual(['step-1'])
    expect(doc.cards[0].ext.editor.title).toBe('A')
    doc.removeCardExtNamespace(0, 'editor')
    expect(doc.cards[0].ext == null).toBe(true)
  })

  it('card-level ext mutators throw IndexOutOfRange', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.setCardExt(5, {})).toThrow(/IndexOutOfRange/)
    expect(() => doc.removeCardExt(5)).toThrow(/IndexOutOfRange/)
    expect(() => doc.setCardExtNamespace(5, 'a', {})).toThrow(/IndexOutOfRange/)
    expect(() => doc.removeCardExtNamespace(5, 'a')).toThrow(/IndexOutOfRange/)
  })
})

// ---------------------------------------------------------------------------
// open + session.render
// ---------------------------------------------------------------------------

describe('quill.open + session.render', () => {
  it('should support open + session.render with pageCount', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = engine.open(quill, doc)
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
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)
    const oob = session.pageCount + 10

    expect(() => {
      session.render({ format: 'png', ppi: 80, pages: [0, oob] })
    }).toThrow(/out of bounds/)
  })

  it('should error when requesting page selection with PDF', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)

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
  description: Metadata test

typst:
  plate_file: plate.typ

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
    const quill = Quill.fromTree(
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
    // supportedFormats moved off metadata onto the engine.
    expect(meta.supportedFormats).toBeUndefined()
    const supportedFormats = engine.supportedFormats(quill)
    expect(Array.isArray(supportedFormats)).toBe(true)
    expect(supportedFormats.length).toBeGreaterThan(0)
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
    const quill = Quill.fromTree(
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
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    const r1 = engine.render(quill, doc, { format: 'svg' })
    const r2 = engine.render(quill, clone, { format: 'svg' })
    expect(r1.artifacts.length).toBe(r2.artifacts.length)
  })
})

// ---------------------------------------------------------------------------
// quill.validate — editor-facing schema validation
// Run via `npm test` after scripts/build-wasm.sh has produced the bundle;
// vitest loads it in a Node environment.
// ---------------------------------------------------------------------------

describe('quill.validate', () => {
  const QUILL_YAML = `quill:
  name: validate_smoke_test
  version: "1.0"
  backend: typst
  description: Smoke test for validate

main:
  fields:
    title:
      type: string
    count:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: string
`

  const buildQuill = () => {
    return Quill.fromTree(makeQuill({ name: 'validate_smoke_test', quillYaml: QUILL_YAML }))
  }

  it('returns an empty array for a complete, well-formed document', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
title: "Hello"
count: 1
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    expect(Array.isArray(diags)).toBe(true)
    expect(diags.length).toBe(0)
  })

  it('forwards a type_mismatch with canonical code, path, and hint', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
title: "Hello"
count: "not-a-number"
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    const mismatch = diags.find((d) => d.code === 'validation::type_mismatch')
    expect(mismatch).toBeDefined()
    expect(mismatch.path).toBe('count')
    expect(typeof mismatch.hint).toBe('string')
  })

  it('reports an unknown card kind under validation::unknown_card', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
title: "T"
count: 1
~~~

~~~card-yaml
$kind: ghost
body: "B"
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    expect(diags.some((d) => d.code === 'validation::unknown_card')).toBe(true)
  })

  it('result is JSON.stringify-able', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
count: "nope"
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    const json = JSON.stringify(diags)
    expect(typeof json).toBe('string')
    expect(JSON.parse(json).length).toBe(diags.length)
  })
})

// ---------------------------------------------------------------------------
// Schema / blueprint / validation — Unendorsed vs Endorsed
// ---------------------------------------------------------------------------
//
// The schema axis is implicit: a field with a `default:` is Endorsed (the
// rendered default is shippable as-is and the blueprint emits the concrete
// value with a type-only `# <type>` annotation); a field without a `default:`
// is Unendorsed (the blueprint emits the `!must_fill` marker).
//
// These tests pin the JS-facing contract:
//   - `QuillFieldSchema` carries no `required` axis.
//   - `quill.blueprint` carries the `!must_fill` marker on Unendorsed fields.
//   - `quill.render(doc)` *tolerates* an absent Unendorsed field: zero-filled
//     render fills it with its type-empty value in the plate projection
//     (never persisted), so absence is not a render error.
//   - A `!must_fill` marker left in the document is non-fatal: `quill.render`
//     succeeds (the field zero-fills or uses its suggested value), and
//     `quill.validate(doc)` surfaces a non-fatal `validation::must_fill`
//     warning per marker.
//
// See prose/canon/SCHEMAS.md.

describe('Unendorsed / Endorsed schema model', () => {
  // The plate `unwrap`s `data.title` (Unendorsed) and substitutes the optional
  // `data.subtitle` if present. Authoring a quill with both Unendorsed and
  // Endorsed fields lets us exercise both validation codes without having to
  // ship two separate test quills.
  const SCHEMA_QUILL_YAML = `quill:
  name: schema_test
  version: "1.0"
  backend: typst
  description: Unendorsed / Endorsed coverage

typst:
  plate_file: plate.typ

main:
  fields:
    title:
      type: string
      description: Document title (Unendorsed — no default)
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
    const quill = Quill.fromTree(
      makeQuill({
        name: 'schema_test',
        plate: SCHEMA_PLATE,
        quillYaml: SCHEMA_QUILL_YAML,
      }),
    )
    return { engine, quill }
  }

  it('schema fields carry no `required` axis', () => {
    const { quill } = buildQuill()
    const fields = quill.schema.main.fields

    expect(fields.title).toBeDefined()
    expect(fields.subtitle).toBeDefined()

    // Cell status is implied by `default:` presence, not a `required` axis.
    expect('required' in fields.title).toBe(false)
    expect('required' in fields.subtitle).toBe(false)

    // Unendorsed fields have no `default`; Endorsed fields do.
    expect(fields.title.default).toBeUndefined()
    expect(fields.subtitle.default).toBe('Untitled subtitle')
  })

  it('blueprint carries `!must_fill` for Unendorsed fields and a type-only annotation for Endorsed', () => {
    const { quill } = buildQuill()
    const blueprint = quill.blueprint

    expect(typeof blueprint).toBe('string')
    expect(blueprint.length).toBeGreaterThan(0)

    // Unendorsed: value cell is the `!must_fill` marker.
    expect(blueprint).toContain('title: !must_fill # string')

    // Endorsed: rendered default with a type-only `# string` annotation. The
    // emitter does not quote strings that don't need quoting (`Untitled
    // subtitle` has no YAML ambiguity), so the value cell is bare.
    expect(blueprint).toContain('subtitle: Untitled subtitle # string')

    // The `; delete-ok` tag is gone entirely — shippability is the value cell.
    expect(blueprint).not.toContain('delete-ok')

    // The `; required` / `; optional` role tag must not appear anywhere.
    expect(blueprint).not.toContain('; required')
    expect(blueprint).not.toContain('; optional')
  })

  it('render tolerates an absent Unendorsed field (zero-filled, not an error)', () => {
    const { engine, quill } = buildQuill()

    // Document omits `title`. Schema declares no default → Unendorsed. Under
    // zero-filled render this is merely *incomplete*, not malformed: render
    // fills `title` with its type-empty value in the plate projection and
    // succeeds. Absence is not a hard error.
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
subtitle: "Just a subtitle"
~~~

# Body
`
    const doc = Document.fromMarkdown(md)

    const result = engine.render(quill, doc, { format: 'svg' })
    expect(result).toBeDefined()
    expect(Array.isArray(result.artifacts)).toBe(true)
    expect(result.artifacts.length).toBeGreaterThan(0)
  })

  it('render tolerates a `!must_fill` marker left in (non-fatal, zero-fills)', () => {
    const { engine, quill } = buildQuill()

    // Document leaves a `!must_fill` marker on `title` — the LLM didn't fill
    // it. This is non-fatal: render zero-fills the field and succeeds.
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title: !must_fill
~~~

# Body
`
    const doc = Document.fromMarkdown(md)

    const result = engine.render(quill, doc, { format: 'svg' })
    expect(result).toBeDefined()
    expect(Array.isArray(result.artifacts)).toBe(true)
    expect(result.artifacts.length).toBeGreaterThan(0)
  })

  it('render succeeds when every Unendorsed field is supplied with a real value', () => {
    const { engine, quill } = buildQuill()

    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title: "A Real Title"
~~~

# Body
`
    const doc = Document.fromMarkdown(md)
    const result = engine.render(quill, doc, { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
  })

  it('validate surfaces a non-fatal `validation::must_fill` warning per marker', () => {
    const { quill } = buildQuill()

    // A `!must_fill` marker left in surfaces a non-fatal warning from validate.
    const mdFill = `~~~card-yaml
$quill: schema_test
$kind: main
title: !must_fill
~~~
`
    const diagsFill = quill.validate(Document.fromMarkdown(mdFill))
    expect(
      diagsFill.some(
        (d) =>
          d.code === 'validation::must_fill' &&
          d.severity === 'warning' &&
          d.path === 'title' &&
          typeof d.hint === 'string' &&
          d.hint.includes('!must_fill'),
      ),
    ).toBe(true)
    // The removed `validation::field_absent` completeness code never surfaces —
    // absent Unendorsed fields zero-fill silently.
    expect(diagsFill.some((d) => d.code === 'validation::field_absent')).toBe(false)
  })
})

describe('nested !must_fill', () => {
  it('exposes nestedFills on a field item, surviving storage and pushCard', () => {
    const md = `~~~card-yaml
$quill: q@0.1
$kind: main
addr:
  street: !must_fill
  city: Anytown
~~~
`
    const doc = Document.fromMarkdown(md)
    const addr = doc.main.payloadItems.find((i) => i.key === 'addr')
    expect(addr.nestedFills).toEqual([['street']])

    // Storage round-trip preserves the nested marker.
    const restored = Document.fromJson(doc.toJson())
    expect(restored.toMarkdown()).toContain('street: !must_fill')

    // A card built with nestedFills survives pushCard → emit.
    const doc2 = Document.fromMarkdown(
      '~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: x\n~~~\n',
    )
    doc2.pushCard({
      kind: 'note',
      payloadItems: [
        {
          type: 'field',
          key: 'addr',
          value: { street: null, city: 'A' },
          nestedFills: [['street']],
        },
      ],
      body: '',
    })
    expect(doc2.toMarkdown()).toContain('street: !must_fill')
  })

  it('omits nestedFills for a field with no nested markers', () => {
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: Hello\n~~~\n',
    )
    const title = doc.main.payloadItems.find((i) => i.key === 'title')
    expect(title.nestedFills).toBeUndefined()
  })
})
