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

const { Quillmark, Document } = await import('@quillmark-wasm')
const { makeQuill } = await import('./test-helpers.js')

const TEST_MARKDOWN = `---
QUILL: test_quill
title: Canvas Test
---

# Hello canvas
`

const TEST_MAIN = `#import "@local/quillmark-helper:0.1.0": data
= #data.title

#data.BODY`

function openQuill() {
  const engine = new Quillmark()
  return engine.quill(makeQuill({ name: 'test_quill', main: TEST_MAIN }))
}

function openSession() {
  return openQuill().open(Document.fromMarkdown(TEST_MARKDOWN))
}

describe('RenderSession canvas preview', () => {
  it('exposes pageCount, backendId, supportsCanvas, warnings, and pageSize on a Typst session', () => {
    const quill = openQuill()
    expect(quill.supportsCanvas).toBe(true)

    const session = quill.open(Document.fromMarkdown(TEST_MARKDOWN))
    expect(session.pageCount).toBeGreaterThan(0)
    expect(session.backendId).toBe('typst')
    expect(session.supportsCanvas).toBe(true)
    expect(Array.isArray(session.warnings)).toBe(true)

    const size = session.pageSize(0)
    expect(size.widthPt).toBeGreaterThan(0)
    expect(size.heightPt).toBeGreaterThan(0)
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

    // Pixel-content sanity. The test main file renders a title heading, so the
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
