# crates/backends/typst

## Needs judgment

### span_scan.rs — `walk_glyphs`/`collect_page_hits` geometry deduplicated; classifier merge remains

Done: the ~60 lines of identical frame-walk geometry (`Group` transform
recursion, per-glyph cursor/advance/bbox, `Shape`/`Image` extents) now live in
one `walk_items` walker that invokes a per-item `visit(page, span, offset,
|| rect)` callback; `collect_page_hits` and `walk_glyphs` are thin consumers
(the region scan emits every item with a lazy box; the corpus walk emits only
classified+resolved ink). The box thunk keeps the region scan's "compute a box
only for classified ink" laziness. Remaining: `walk_glyphs` still calls both
`classify_seg` and `resolve_range` per item though `classify_seg` already
resolves internally — a `classify` returning `(window, seg, node_range)`
together would drop the second resolve. Smaller, separable; `classify_seg` is
shared with the region path, so its signature change is pinned separately.

### span_scan.rs — `locate` allocated a hit per document glyph; now filters in the walk

`locate` still visits every page's frame, but `walk_glyphs` now takes an
`only: Option<(window, seg)>` target so the caret path allocates a `GlyphHit`
and computes a box for **only** its target segment's glyphs, not every
window-classified glyph in the document; the now-redundant post-walk filter is
gone. Remaining (deferred, needs region-scan integration): the frame *traversal*
is still whole-document — restricting it to the page(s) the segment's region
scan placed the field on would need `locate` to consult that scan's output,
which it does not currently hold.
