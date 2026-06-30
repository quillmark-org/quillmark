/**
 * Unit tests for `RegionMap` — the pure, per-page projection of
 * `RenderSession.regions()` into overlay geometry and a click hit-test.
 *
 * `RegionMap` is pure data (no WASM, no DOM), so these tests feed it plain
 * `FieldRegion`/`PageSize` literals and assert the coordinate transform
 * (Y-flip, bottom-left→top-left origin, pt↔device-px), the page filter, the
 * smallest-region-wins hit-test, and the input validation. Aliased to
 * pkg/runtime/runtime.js in vitest.config.js.
 */
import { describe, it, expect } from 'vitest'
import { RegionMap } from '@quillmark-wasm/runtime'

// A 200×400 pt page picked for clean percentages. Regions are in PDF points,
// bottom-left origin: `[x0, y0, x1, y1]`.
const PAGE = { widthPt: 200, heightPt: 400 }
const REGIONS = [
  { field: 'title', page: 0, rect: [20, 360, 120, 380] }, // top band; disjoint from body
  { field: 'body', page: 0, rect: [10, 20, 190, 300] }, //   large; contains `inner`
  { field: 'inner', page: 0, rect: [30, 100, 80, 150] }, //   nested inside body
  { field: 'page2', page: 1, rect: [0, 0, 100, 100] }, //     different page → filtered out
]

const map = () => RegionMap.from(REGIONS, PAGE, 0)

describe('RegionMap.from', () => {
  it('filters to the requested page and reports page + pageSize + fields', () => {
    const m = map()
    expect(m.page).toBe(0)
    expect(m.pageSize).toEqual(PAGE)
    // `page2` lives on page 1 and is dropped; order matches the input.
    expect(m.fields).toEqual(['title', 'body', 'inner'])
  })

  it('throws when pageSize is not positive and finite on both axes', () => {
    expect(() => RegionMap.from(REGIONS, { widthPt: 0, heightPt: 400 }, 0)).toThrow(/pageSize/)
    expect(() => RegionMap.from(REGIONS, { widthPt: 200, heightPt: -1 }, 0)).toThrow(/pageSize/)
    expect(() => RegionMap.from(REGIONS, { widthPt: 200, heightPt: NaN }, 0)).toThrow(/pageSize/)
    expect(() =>
      RegionMap.from(REGIONS, { widthPt: Infinity, heightPt: 400 }, 0),
    ).toThrow(/pageSize/)
  })
})

describe('RegionMap.region', () => {
  it('returns the raw region for a field on this page', () => {
    expect(map().region('title')).toEqual(REGIONS[0])
  })

  it('returns undefined for an unknown field or one on another page', () => {
    expect(map().region('nope')).toBeUndefined()
    expect(map().region('page2')).toBeUndefined()
  })
})

describe('RegionMap.overlayPercent', () => {
  it('projects to percent-of-page with the Y axis flipped to a top-left origin', () => {
    // title rect [20,360,120,380] on a 200×400 page:
    //   left=20/200=10%  top=(400-380)/400=5%  width=100/200=50%  height=20/400=5%
    expect(map().overlayPercent('title')).toEqual({ left: 10, top: 5, width: 50, height: 5 })
  })

  it('returns undefined for an absent field', () => {
    expect(map().overlayPercent('page2')).toBeUndefined()
  })
})

describe('RegionMap.overlayDevice', () => {
  it('projects to device pixels at renderScale with the Y axis flipped', () => {
    // title rect [20,360,120,380] at renderScale 2:
    //   left=20*2=40  top=(400-380)*2=40  width=100*2=200  height=20*2=40
    expect(map().overlayDevice('title', 2)).toEqual({ left: 40, top: 40, width: 200, height: 40 })
  })

  it('returns undefined for an absent field (before touching geometry)', () => {
    expect(map().overlayDevice('page2', 2)).toBeUndefined()
  })

  it('throws on a non-positive or non-finite renderScale', () => {
    expect(() => map().overlayDevice('title', 0)).toThrow(/renderScale/)
    expect(() => map().overlayDevice('title', -1)).toThrow(/renderScale/)
    expect(() => map().overlayDevice('title', NaN)).toThrow(/renderScale/)
    expect(() => map().overlayDevice('title', Infinity)).toThrow(/renderScale/)
  })
})

describe('RegionMap.overlaysPercent / overlaysDevice', () => {
  it('emits one non-optional box per field on the page, in order', () => {
    const all = map().overlaysPercent()
    expect(all.map((o) => o.field)).toEqual(['title', 'body', 'inner'])
    expect(all[0].box).toEqual({ left: 10, top: 5, width: 50, height: 5 })
  })

  it('overlaysDevice validates renderScale like the singular form', () => {
    expect(() => map().overlaysDevice(0)).toThrow(/renderScale/)
    expect(map().overlaysDevice(2)).toHaveLength(3)
  })
})

describe('RegionMap.at (hit-test, page percent, top-left origin)', () => {
  it('returns the field under a point in its own band', () => {
    // (15%, 7%) is inside title's percent box [10..60] × [5..10] and nothing else.
    expect(map().at(15, 7)?.field).toBe('title')
  })

  it('returns the smallest region when boxes nest', () => {
    // inner ([15..40]×[62.5..75]) sits inside body; a point in both resolves to inner.
    expect(map().at(25, 68)?.field).toBe('inner')
    // A point inside body but outside inner resolves to body.
    expect(map().at(50, 50)?.field).toBe('body')
  })

  it('returns undefined outside every region and for a non-finite coordinate', () => {
    expect(map().at(98, 98)).toBeUndefined()
    expect(map().at(NaN, 7)).toBeUndefined()
  })
})
