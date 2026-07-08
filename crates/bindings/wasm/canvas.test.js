/**
 * Canvas-preview smoke tests for quillmark-wasm.
 *
 * Vitest runs in a Node environment with no DOM, so we polyfill the bare
 * minimum needed for wasm-bindgen's `instanceof` checks to pass:
 *
 *   - `globalThis.CanvasRenderingContext2D`
 *   - `globalThis.OffscreenCanvasRenderingContext2D`
 *   - `globalThis.ImageData`
 *
 * The polyfill captures `putImageData` calls into a buffer so the test can
 * assert that `paint` actually invoked the context with sensibly-sized
 * pixels and non-empty pixel content. Pixel-perfect correctness needs a
 * real browser test; this catches regressions like broken downcast,
 * mis-sized buffer, swapped channels, missing demultiply, or panics.
 */

import { describe, it, expect, beforeAll } from 'vitest'

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
    // Copy the byte view so the test can inspect pixels even if Rust later
    // reuses the underlying buffer.
    this.calls.push({
      width: img.width,
      height: img.height,
      data: new Uint8ClampedArray(img.data),
      dx,
      dy,
    })
  }
}

// In real browsers, OffscreenCanvasRenderingContext2D and
// CanvasRenderingContext2D do NOT share an inheritance chain — they're
// siblings. Defining the polyfill as an independent class (not a subclass)
// ensures the Rust-side `instanceof` dispatch actually exercises the
// second branch, instead of matching `CanvasRenderingContext2D` via
// inheritance.
class FakeOffscreenCanvasRenderingContext2D {
  constructor() {
    this.calls = []
    this.canvas = { width: 0, height: 0 }
  }
  putImageData(img, dx, dy) {
    this.calls.push({
      width: img.width,
      height: img.height,
      data: new Uint8ClampedArray(img.data),
      dx,
      dy,
    })
  }
}

beforeAll(() => {
  globalThis.ImageData = FakeImageData
  globalThis.CanvasRenderingContext2D = FakeCanvasRenderingContext2D
  globalThis.OffscreenCanvasRenderingContext2D = FakeOffscreenCanvasRenderingContext2D
})

const { Quillmark, Quill, Document } = await import('@quillmark-wasm')
// The pdfform backend bundle: same engine + LiveSession + canvas
// surface as the typst bundle, but a Typst-free PDF-form backend that paints by
// rasterizing its pre-flattened page. SEPARATE WASM memory from the typst
// bundle — its handles never mix with the typst ones.
const {
  Quillmark: PdfformQuillmark,
  Quill: PdfformQuill,
  Document: PdfformDocument,
} = await import('@quillmark-wasm/pdfform')
const { makeQuill, makeSampleFormQuill, SAMPLE_FORM_MARKDOWN } = await import('./test-helpers.js')

const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Canvas Test
~~~

# Hello canvas
`

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
= #data.title

#data.at("$body")`

function openQuill() {
  const engine = new Quillmark()
  const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
  return { engine, quill }
}

function openSession() {
  const { engine, quill } = openQuill()
  return engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
}

describe('LiveSession canvas preview', () => {
  it('exposes pageCount, backendId, supportsCanvas, warnings, and pageSize on a Typst session', () => {
    const { engine, quill } = openQuill()
    expect(engine.supportsCanvas(quill)).toBe(true)

    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    expect(session.pageCount).toBeGreaterThan(0)
    expect(session.backendId).toBe('typst')
    expect(session.supportsCanvas).toBe(true)
    expect(Array.isArray(session.warnings)).toBe(true)

    const size = session.pageSize(0)
    expect(size.widthPt).toBeGreaterThan(0)
    expect(size.heightPt).toBeGreaterThan(0)
  })

  it('positionAt resolves content ink to a corpus position and locate reverses it (#829)', () => {
    const session = openSession()

    // A content field surfaces one span-bearing region per segment (#829); the
    // body's heading is one.
    const bodyRegion = session.regions().find((r) => r.field === '$body' && r.span)
    expect(bodyRegion, 'a $body segment region carries a span').toBeTruthy()
    const [x0, y0, x1, y1] = bodyRegion.rect
    const cy = (y0 + y1) / 2

    // Forward: scan across the segment's ink until a point resolves (glyph
    // layout decides which x lands on ink), then assert the CorpusHit crosses
    // the JS boundary with the right shape.
    let hit = null
    for (let f = 0.1; f <= 0.9 && !hit; f += 0.1) {
      hit = session.positionAt(bodyRegion.page, x0 + (x1 - x0) * f, cy)
    }
    expect(hit, 'positionAt resolves a point on the body ink').toBeTruthy()
    expect(hit.field).toBe('$body')
    expect(typeof hit.pos).toBe('number')
    const [start, end] = bodyRegion.span
    expect(hit.pos).toBeGreaterThanOrEqual(start)
    expect(hit.pos).toBeLessThanOrEqual(end)

    // Reverse: corpus position → caret rect on the same field.
    const caret = session.locate('$body', hit.pos)
    expect(caret, 'locate reverses positionAt').toBeTruthy()
    expect(caret.field).toBe('$body')
    expect(caret.rect[2]).toBeGreaterThanOrEqual(caret.rect[0])

    // A click far off any ink resolves to nothing (wasm maps `None` to undefined).
    expect(session.positionAt(bodyRegion.page, 2, 2)).toBeFalsy()
  })

  it('paint sizes the canvas backing store and returns layout + pixel dimensions', () => {
    const session = openSession()
    const { widthPt, heightPt } = session.pageSize(0)
    const layoutScale = 1
    const densityScale = 1.5

    const ctx = new FakeCanvasRenderingContext2D()
    const result = session.paint(ctx, 0, { layoutScale, densityScale })

    // Layout dimensions reflect layoutScale only — independent of density.
    expect(result.layoutWidth).toBeCloseTo(widthPt * layoutScale, 4)
    expect(result.layoutHeight).toBeCloseTo(heightPt * layoutScale, 4)

    // Pixel dimensions reflect layoutScale * densityScale, rounded.
    expect(result.pixelWidth).toBe(Math.round(widthPt * layoutScale * densityScale))
    expect(result.pixelHeight).toBe(Math.round(heightPt * layoutScale * densityScale))

    // Painter owns canvas.width/height — they must equal the reported
    // pixel dimensions.
    expect(ctx.canvas.width).toBe(result.pixelWidth)
    expect(ctx.canvas.height).toBe(result.pixelHeight)

    expect(ctx.calls).toHaveLength(1)
    const call = ctx.calls[0]
    expect(call.dx).toBe(0)
    expect(call.dy).toBe(0)
    expect(call.width).toBe(result.pixelWidth)
    expect(call.height).toBe(result.pixelHeight)
    expect(call.data.length).toBe(call.width * call.height * 4)

    // Pixel-content sanity. The test plate renders a title heading, so the
    // rasterized buffer must contain non-white pixels (visible glyph ink)
    // *and* opaque pixels (page background). A regression that wrote zeros,
    // swapped channels, or skipped demultiply would fail at least one of
    // these.
    let inkPixels = 0
    let opaquePixels = 0
    for (let i = 0; i < call.data.length; i += 4) {
      const [r, g, b, a] = [call.data[i], call.data[i + 1], call.data[i + 2], call.data[i + 3]]
      if (a > 0 && (r < 250 || g < 250 || b < 250)) inkPixels++
      if (a === 255) opaquePixels++
    }
    expect(inkPixels).toBeGreaterThan(0)
    expect(opaquePixels).toBeGreaterThan(0)
  })

  it('paint defaults layoutScale and densityScale to 1 when opts are omitted', () => {
    const session = openSession()
    const { widthPt, heightPt } = session.pageSize(0)

    const ctx = new FakeCanvasRenderingContext2D()
    const result = session.paint(ctx, 0)

    expect(result.layoutWidth).toBeCloseTo(widthPt, 4)
    expect(result.layoutHeight).toBeCloseTo(heightPt, 4)
    expect(result.pixelWidth).toBe(Math.round(widthPt))
    expect(result.pixelHeight).toBe(Math.round(heightPt))
  })

  it('also paints into an OffscreenCanvasRenderingContext2D', () => {
    const session = openSession()
    const ctx = new FakeOffscreenCanvasRenderingContext2D()
    const result = session.paint(ctx, 0, { densityScale: 2 })

    expect(ctx.calls).toHaveLength(1)
    expect(ctx.canvas.width).toBe(result.pixelWidth)
    expect(ctx.canvas.height).toBe(result.pixelHeight)
  })

  it('paint clamps backing-store dimensions to the safe maximum', () => {
    const session = openSession()
    const { widthPt, heightPt } = session.pageSize(0)
    const longest = Math.max(widthPt, heightPt)
    // Pick a densityScale that drives the longest backing dimension well
    // past the 16384-px clamp threshold.
    const densityScale = (16384 / longest) * 4

    const ctx = new FakeCanvasRenderingContext2D()
    const result = session.paint(ctx, 0, { densityScale })

    // Backing dimensions clamp at 16384 on the longer side.
    expect(Math.max(result.pixelWidth, result.pixelHeight)).toBeLessThanOrEqual(16384)
    // Layout dimensions are independent of the clamp.
    expect(result.layoutWidth).toBeCloseTo(widthPt, 4)
    expect(result.layoutHeight).toBeCloseTo(heightPt, 4)
    // Detect-clamp contract: pixelWidth < round(layoutWidth * densityScale).
    expect(result.pixelWidth).toBeLessThan(Math.round(result.layoutWidth * densityScale))
  })

  it('paint throws on non-finite or non-positive layoutScale / densityScale', () => {
    const session = openSession()
    const ctx = new FakeCanvasRenderingContext2D()
    expect(() => session.paint(ctx, 0, { layoutScale: 0 })).toThrow(/layoutScale/)
    expect(() => session.paint(ctx, 0, { layoutScale: -1 })).toThrow(/layoutScale/)
    expect(() => session.paint(ctx, 0, { layoutScale: Number.NaN })).toThrow(/layoutScale/)
    expect(() => session.paint(ctx, 0, { densityScale: 0 })).toThrow(/densityScale/)
    expect(() =>
      session.paint(ctx, 0, { densityScale: Number.POSITIVE_INFINITY }),
    ).toThrow(/densityScale/)
  })

  it('throws an out-of-range error when paint is called with a bad page index', () => {
    const session = openSession()
    const ctx = new FakeCanvasRenderingContext2D()
    expect(() => session.paint(ctx, session.pageCount + 5)).toThrow(
      /out of range.*pageCount=/,
    )
  })
})

describe('LiveSession canvas preview (pdfform backend)', () => {
  function openPdfformQuill() {
    const engine = new PdfformQuillmark()
    const quill = PdfformQuill.fromTree(makeSampleFormQuill())
    return { engine, quill }
  }

  function openPdfformSession() {
    const { engine, quill } = openPdfformQuill()
    return engine.open(quill, PdfformDocument.fromMarkdown(SAMPLE_FORM_MARKDOWN))
  }

  it('reports canvas support and page geometry for a pdfform quill', () => {
    const { engine, quill } = openPdfformQuill()
    // The pdfform backend rasterizes pre-flattened pages.
    expect(engine.supportsCanvas(quill)).toBe(true)

    const session = engine.open(quill, PdfformDocument.fromMarkdown(SAMPLE_FORM_MARKDOWN))
    expect(session.pageCount).toBeGreaterThan(0)
    expect(session.backendId).toBe('pdfform')
    expect(session.supportsCanvas).toBe(true)

    const size = session.pageSize(0)
    expect(size.widthPt).toBeGreaterThan(0)
    expect(size.heightPt).toBeGreaterThan(0)
  })

  it('paint sizes the canvas per the DPR math and bakes field-value ink into the raster', () => {
    const session = openPdfformSession()
    const { widthPt, heightPt } = session.pageSize(0)
    const layoutScale = 1
    const densityScale = 1.5

    const ctx = new FakeCanvasRenderingContext2D()
    const result = session.paint(ctx, 0, { layoutScale, densityScale })

    // Layout dimensions reflect layoutScale only — independent of density.
    expect(result.layoutWidth).toBeCloseTo(widthPt * layoutScale, 4)
    expect(result.layoutHeight).toBeCloseTo(heightPt * layoutScale, 4)

    // Pixel dimensions reflect layoutScale * densityScale, rounded (toBeCloseTo
    // precision -1 tolerates the rasterizer's per-axis rounding).
    expect(result.pixelWidth).toBeCloseTo(Math.round(widthPt * layoutScale * densityScale), -1)
    expect(result.pixelHeight).toBeCloseTo(Math.round(heightPt * layoutScale * densityScale), -1)

    // Painter owns canvas.width/height — they equal the reported pixel dims.
    expect(ctx.canvas.width).toBe(result.pixelWidth)
    expect(ctx.canvas.height).toBe(result.pixelHeight)

    expect(ctx.calls).toHaveLength(1)
    const call = ctx.calls[0]
    expect(call.width).toBe(result.pixelWidth)
    expect(call.height).toBe(result.pixelHeight)
    expect(call.data.length).toBe(call.width * call.height * 4)

    // COMPLETE-RASTER contract: the pre-flattened "Ada Lovelace" et al. are
    // baked into the page, so the buffer must carry non-white opaque ink
    // (field values + form lines) AND opaque page background. A backend that
    // returned only a blank background (no values) would fail the ink check.
    let inkPixels = 0
    let opaquePixels = 0
    for (let i = 0; i < call.data.length; i += 4) {
      const [r, g, b, a] = [call.data[i], call.data[i + 1], call.data[i + 2], call.data[i + 3]]
      if (a > 0 && (r < 250 || g < 250 || b < 250)) inkPixels++
      if (a === 255) opaquePixels++
    }
    expect(inkPixels).toBeGreaterThan(0)
    expect(opaquePixels).toBeGreaterThan(0)
  })
})

describe('LiveSession.apply', () => {
  it('recompiles in place and reports the dirty page set', () => {
    const { engine, quill } = openQuill()
    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    const before = session.pageCount

    const cs = session.apply(
      Document.fromMarkdown(TEST_MARKDOWN.replace('Canvas Test', 'Edited Title'))
    )
    expect(cs.pageCount).toBe(before)
    expect(cs.pageCount).toBe(session.pageCount)
    expect(cs.dirtyPages).toContain(0)

    // Identical re-apply → nothing dirty.
    const cs2 = session.apply(
      Document.fromMarkdown(TEST_MARKDOWN.replace('Canvas Test', 'Edited Title'))
    )
    expect(cs2.dirtyPages).toEqual([])

    // Reads serve the new compile: the repainted page differs.
    session.free()
  })

  it('keeps the last-good compile when apply throws, and recovers', () => {
    const { engine, quill } = openQuill()
    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    const before = session.pageCount

    // A document for the wrong quill fails the $quill reference check.
    const wrong = Document.fromMarkdown(
      TEST_MARKDOWN.replace('$quill: test_quill', '$quill: other_quill')
    )
    expect(() => session.apply(wrong)).toThrow()

    // Every read still serves the last-good compile.
    expect(session.pageCount).toBe(before)
    const ctx = new FakeCanvasRenderingContext2D()
    const result = session.paint(ctx, 0)
    expect(result.pixelWidth).toBeGreaterThan(0)
    expect(ctx.calls.length).toBe(1)

    // The session recovers on the next good apply.
    const cs = session.apply(
      Document.fromMarkdown(TEST_MARKDOWN.replace('Canvas Test', 'Recovered'))
    )
    expect(cs.pageCount).toBe(session.pageCount)
    session.free()
  })
})

describe('LiveSession revision stamp + applyFieldDelta (PR-F/PR-G)', () => {
  // A pure-insert delta prepends `text` to the body corpus without needing to
  // know its length (Delta.apply appends the untouched remainder after the ops).
  const prepend = (text) => ({ ops: [{ insert: text }] })

  it('stamps geometry reads with the current revision', () => {
    const session = openSession()
    expect(session.revision).toBe(0)

    // Every region carries the session's revision; a fresh session is at 0.
    const regions = session.regions()
    expect(regions.length).toBeGreaterThan(0)
    for (const r of regions) expect(r.revision).toBe(0)

    // positionAt / locate stamp the same revision.
    const bodyRegion = regions.find((r) => r.field === '$body' && r.span)
    expect(bodyRegion).toBeTruthy()
    const caret = session.locate('$body', bodyRegion.span[0])
    expect(caret.revision).toBe(0)
  })

  it('applyFieldDelta splices the body, advances the revision, and dirties the page', () => {
    const { engine, quill } = openQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)
    expect(session.revision).toBe(0)

    const cs = session.applyFieldDelta(doc, '$body', 0, prepend('NEW '))
    expect(cs.dirtyPages).toContain(0)

    // The revision advanced and the mutation reached the document body.
    expect(session.revision).toBe(1)
    expect(doc.main.bodyMarkdown).toContain('NEW')

    // The next read is stamped at the new revision.
    const region = session.regions().find((r) => r.field === '$body')
    expect(region.revision).toBe(1)
    session.free()
  })

  it('maps a position captured at an older revision forward through the edit', () => {
    const { engine, quill } = openQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)

    // At the base revision, mapping is the identity.
    expect(session.mapFieldPos('$body', 0, 0, 'after')).toBe(0)

    // Prepend a 4-USV run ("NEW ") at corpus position 0.
    session.applyFieldDelta(doc, '$body', 0, prepend('NEW '))

    // A position captured at revision 0 maps forward past the leading insert
    // with Assoc::After, and stays put with Assoc::Before.
    expect(session.mapFieldPos('$body', 0, 0, 'after')).toBe(4)
    expect(session.mapFieldPos('$body', 0, 0, 'before')).toBe(0)
    session.free()
  })

  it('rejects a stale base revision without mutating (session::revision_mismatch)', () => {
    const { engine, quill } = openQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)

    session.applyFieldDelta(doc, '$body', 0, prepend('A '))
    expect(session.revision).toBe(1)

    // Re-submitting against the now-stale base 0 throws and changes nothing.
    let thrown
    try {
      session.applyFieldDelta(doc, '$body', 0, prepend('B '))
    } catch (e) {
      thrown = e
    }
    expect(thrown).toBeTruthy()
    expect(thrown.diagnostics?.[0]?.code).toBe('session::revision_mismatch')
    expect(session.revision).toBe(1)
    session.free()
  })

  it('rejects a non-body field as a delta-path target', () => {
    const { engine, quill } = openQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)
    expect(() => session.applyFieldDelta(doc, 'subject', 0, prepend('x'))).toThrow(/\$body/)
    session.free()
  })

  it('mapFieldPos accepts only "before" or "after"', () => {
    const session = openSession()
    expect(() => session.mapFieldPos('$body', 0, 0, 'sideways')).toThrow(/before.*after/)
  })
})
