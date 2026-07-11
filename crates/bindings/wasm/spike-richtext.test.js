/**
 * Spike verification — corpus-native richtext editing + bidirectional
 * editor⇄preview navigation over a real quill (airmark `usaf_memo`), driven
 * entirely through the public `@quillmark/wasm` runtime surface.
 *
 * Proves the data path a ProseMirror integration relies on, with NO markdown
 * intermediary anywhere:
 *   1. A document whose body and richtext fields are canonical corpus objects
 *      (the exact shape `docToCorpus` in the web-app emits) loads via
 *      `Document.fromJson` and renders.
 *   2. `LiveSession.regions()` surfaces the body's `$body` region with a corpus
 *      `span`, plus the richtext scalar/array fields (subject, signature_block).
 *   3. Preview → editor: `fieldAt`/`positionAt` map a page point back to the
 *      field and a USV corpus offset.
 *   4. Editor → preview: `locate(field, pos)` maps a corpus offset to a caret
 *      rect on the page.
 *   5. `apply(editedDoc)` recompiles and reports dirty pages.
 */
import { describe, it, expect, beforeAll } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Engine, Document, Quill } from '@quillmark-wasm/runtime'

// The in-repo usaf_memo fixture (subject/tag_line/references migrated to
// richtext; the plate binds `signature_block` as a form-field region). Mirrors
// the airmark-quiver spike quill, so this stays self-contained / CI-safe.
const __dirname = dirname(fileURLToPath(import.meta.url))
const QUILL_DIR = join(
  __dirname,
  '..',
  '..',
  'fixtures',
  'resources',
  'quills',
  'usaf_memo',
  '0.2.0'
)

function loadTree(dir) {
  const tree = new Map()
  const walk = (d) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name)
      if (statSync(p).isDirectory()) walk(p)
      else tree.set(relative(dir, p).split('\\').join('/'), new Uint8Array(readFileSync(p)))
    }
  }
  walk(dir)
  return tree
}

// A block-richtext body corpus: two lines (a paragraph + a bulleted item),
// with a bold and an italic run in the first line. This is exactly what the
// ProseMirror→corpus codec produces — hand-authored here so the test has no
// prosemirror dependency.
//
//        1         2         3
// 0123456789012345678901234567890123456
// The mission is BOLD and italic here.
const BODY = {
  islands: [],
  text: 'The mission is BOLD and italic here.\nSecond point, auto-lettered.',
  lines: [
    { kind: 'para', containers: [] },
    { kind: 'para', containers: [{ container: 'list_item', ordered: false, start: 1, ordinal: 0 }] }
  ],
  marks: [
    { start: 15, end: 19, type: 'strong' }, // BOLD
    { start: 24, end: 30, type: 'emph' } // italic
  ]
}

const SUBJECT = {
  islands: [],
  text: 'Spike Subject With Emphasis',
  lines: [{ kind: 'para', containers: [] }],
  marks: [{ start: 6, end: 13, type: 'strong' }] // "Subject"
}

/** Build a corpus-native document DTO by editing the seeded DTO in place. */
function buildDoc(quill) {
  const dto = JSON.parse(quill.seedDocument().toJson())
  dto.main.body = BODY
  for (const item of dto.main.payload.items) {
    if (item.type === 'field' && item.key === 'subject') item.value = SUBJECT
  }
  return Document.fromJson(JSON.stringify(dto))
}

describe('spike: corpus-native richtext + bidirectional nav (usaf_memo)', () => {
  let quill
  let engine
  // Warm up here (long timeout): the first `open` pays the one-time cost of
  // instantiating the WASM module and building the Typst world — multi-second
  // on the unoptimized `--ci` build. Absorbing it in beforeAll keeps each test's
  // own timeout meaningful.
  beforeAll(async () => {
    quill = Quill.fromTree(loadTree(QUILL_DIR))
    engine = new Engine()
    const warm = await engine.open(quill, buildDoc(quill))
    warm.free()
  }, 180000)

  it('loads a corpus-native document and opens a canvas session', async () => {
    const doc = buildDoc(quill)
    const session = await engine.open(quill, doc)
    try {
      expect(session.pageCount).toBeGreaterThanOrEqual(1)
      expect(session.supportsCanvas).toBe(true)
      const size = session.pageSize(0)
      expect(size.widthPt).toBeGreaterThan(0)
      expect(size.heightPt).toBeGreaterThan(0)
    } finally {
      session.free()
    }
  })

  it('regions() surfaces the $body span and the richtext fields', async () => {
    const doc = buildDoc(quill)
    const session = await engine.open(quill, doc)
    try {
      const regions = session.regions()
      const fields = [...new Set(regions.map((r) => r.field))]
      console.log('[spike] region fields:', fields)

      const body = regions.find((r) => r.field === '$body' && r.span)
      expect(body, 'a $body region with a corpus span').toBeTruthy()
      expect(body.span[1]).toBeGreaterThan(body.span[0])
      expect(body.rect).toHaveLength(4)

      // The richtext fields we migrated surface their own regions.
      expect(fields).toContain('subject')
      expect(fields).toContain('signature_block')
    } finally {
      session.free()
    }
  })

  it('preview → editor: fieldAt / positionAt resolve a page point to a corpus offset', async () => {
    const doc = buildDoc(quill)
    const session = await engine.open(quill, doc)
    try {
      const body = session.regions().find((r) => r.field === '$body' && r.span)
      const [x0, y0, x1, y1] = body.rect
      const cx = (x0 + x1) / 2
      const cy = (y0 + y1) / 2

      expect(session.fieldAt(body.page, cx, cy)).toBe('$body')

      const hit = session.positionAt(body.page, cx, cy)
      expect(hit.field).toBe('$body')
      expect(hit.pos).toBeGreaterThanOrEqual(body.span[0])
      expect(hit.pos).toBeLessThanOrEqual(body.span[1])
      console.log('[spike] positionAt center →', JSON.stringify(hit))

      // Off any field ink resolves to nothing.
      expect(session.fieldAt(body.page, 1, 1)).toBeUndefined()
    } finally {
      session.free()
    }
  })

  it('editor → preview: locate maps a corpus offset to a caret rect', async () => {
    const doc = buildDoc(quill)
    const session = await engine.open(quill, doc)
    try {
      const body = session.regions().find((r) => r.field === '$body' && r.span)
      const caret = session.locate('$body', body.span[0])
      expect(caret, 'a caret rect for the body start').toBeTruthy()
      expect(caret.rect).toHaveLength(4)
      expect(caret.page).toBe(body.page)
      console.log('[spike] locate($body, span.start) →', JSON.stringify(caret.rect))

      // Round-trip: the located caret point resolves back to the same field.
      const [rx0, , rx1, ry1] = caret.rect
      expect(session.fieldAt(caret.page, (rx0 + rx1) / 2, ry1 - 1)).toBe('$body')
    } finally {
      session.free()
    }
  })

  it('apply(editedDoc) recompiles and reports dirty pages', async () => {
    const doc = buildDoc(quill)
    const session = await engine.open(quill, doc)
    try {
      // Edit the body corpus (append a third line) — the corpus-native edit loop.
      const edited = JSON.parse(doc.toJson())
      edited.main.body = {
        islands: [],
        text: BODY.text + '\nA third paragraph added by the edit loop.',
        lines: [...BODY.lines, { kind: 'para', containers: [] }],
        marks: BODY.marks
      }
      const cs = session.apply(Document.fromJson(JSON.stringify(edited)))
      expect(cs.pageCount).toBeGreaterThanOrEqual(1)
      expect(Array.isArray(cs.dirtyPages)).toBe(true)
      expect(cs.dirtyPages).toContain(0)
      console.log('[spike] apply ChangeSet:', JSON.stringify(cs))
    } finally {
      session.free()
    }
  })

  it('setBody(corpus) sets the body corpus with no markdown (#874)', async () => {
    const doc = buildDoc(quill)
    const NEW = {
      islands: [],
      text: 'Replaced body via setBody.',
      lines: [{ kind: 'para', containers: [] }],
      marks: [{ start: 0, end: 8, type: 'strong' }]
    }
    doc.setBody(NEW)
    // The mutator commits the corpus in place — read it straight back.
    expect(doc.main.body.text).toBe('Replaced body via setBody.')
    const session = await engine.open(quill, doc)
    try {
      const body = session.regions().find((r) => r.field === '$body' && r.span)
      expect(body, 'setBody corpus renders a $body region').toBeTruthy()
    } finally {
      session.free()
    }
  })

  it('applyFieldDelta / mapFieldPos / revision drive an incremental $body edit (#876)', async () => {
    const doc = buildDoc(quill)
    const session = await engine.open(quill, doc)
    try {
      expect(session.revision).toBe(0)

      // A form caret sitting at USV 15 (start of "BOLD") before the edit.
      const caretBefore = 15
      // Prepend "NEW " with a text-splice delta (CodeMirror ChangeSet semantics).
      const delta = { ops: [{ insert: 'NEW ' }, { retain: BODY.text.length }] }
      const cs = session.applyFieldDelta(doc, '$body', 0, delta)

      expect(cs.dirtyPages).toContain(0)
      expect(session.revision).toBe(1)
      // doc is mutated in place across the WASM seam.
      expect(doc.main.body.text.startsWith('NEW ')).toBe(true)
      // The pre-edit caret maps forward past the 4-char insert.
      const mapped = session.mapFieldPos('$body', 0, caretBefore, 'after')
      expect(mapped).toBe(caretBefore + 4)
      console.log(`[spike] revision ${session.revision}; mapped ${caretBefore} -> ${mapped}`)

      // A stale base revision is rejected transactionally (revision unchanged).
      expect(() => session.applyFieldDelta(doc, '$body', 0, delta)).toThrow()
      expect(session.revision).toBe(1)
    } finally {
      session.free()
    }
  })
})
