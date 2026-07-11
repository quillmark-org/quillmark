//! Schema-field geometry, queried from a compiled
//! [`LiveSession`](crate::LiveSession) via
//! [`regions`](crate::LiveSession::regions) and
//! [`field_at`](crate::LiveSession::field_at).
//!
//! A region ties a rectangle on the rendered page to the **quill schema field**
//! that produced it — the address the document author already uses to refer to
//! that field (the same address the Typst plate reads as `data.*` and the
//! pdfform binder resolves against `compile_data`). The two directions a
//! consumer navigates get two queries: `regions` answers *field → rectangle*
//! (scroll to / highlight the focused field), `field_at` answers *point →
//! field* (click a rendered field → focus it in the editor).
//!
//! Three producers feed regions, all keyed on the schema path:
//!
//! - **Content fields** (a richtext body, a `richtext[]` element, a card's
//!   content field) are tracked by the **spans** their glyphs carry: the
//!   backend evaluates each one's value at its own generated call site and
//!   records the site's byte window, so every glyph of that content resolves
//!   back to its field — through *any* placement context, including a package
//!   that rebuilds the content (a `show`-rule pass that captures paragraphs
//!   into a state buffer and re-emits them), because the origin rides the
//!   glyph, not a sibling marker a rebuild could drop. A field that is blank
//!   or draws nothing (an empty or whitespace-only body) has no inked extent
//!   to bound and surfaces no region — present-but-empty is not the same as
//!   placed.
//! - **Direct scalar references** — every `data.<field>` / `data.at("field")`
//!   expression in the plate is its own tracked site: the interpolated
//!   value's glyphs carry a span at or around that reference expression. A
//!   scalar shown in both a header and a footer surfaces both sites, because
//!   two source expressions are two origins; a reference wrapped in an
//!   expression (`#upper(data.subject)`) attributes the whole expression's
//!   ink to the field as long as it is the expression's only reference. Not
//!   tracked: an expression mixing several fields (`data.from + ", " + rank`
//!   has no single owner), a value laundered through an intermediate binding
//!   (`#let s = data.x` … `#s`), and card scalars read from the per-card
//!   loop variable (`card.from` is *one* expression site shared by every card
//!   instance — span data holds no per-instance identity; bind a widget for
//!   those).
//! - **Form-field widgets** carry a schema path explicitly: pdfform binds it
//!   from the form mapping; a Typst `form-field` binds it from its `field:`
//!   argument. A widget that binds none produces **no** region — its backend
//!   identifier (the `/T` widget name) is not a schema address, so there is
//!   nothing for a consumer to route to. Only schema-addressable fields surface
//!   a region.
//!
//! **First placement only.** A content value placed at two sites surfaces one
//! region set — its first placement's — because span data cannot distinguish
//! "package chrome interrupting one placement" from "a second placement of
//! the same value", and a spanning union would claim the ink between them.
//! The first placement is one region per page it touches, in page order, so
//! highlighting covers continuation pages — page marginals (headers, footers,
//! page numbers) between one page's body and the next's do not end it, only a
//! same-page interruption does: foreign ink within a page (a rebuild's
//! numbering chrome) shrinks the region to the placement's true start rather
//! than lying about extent. `field` is still not unique in the
//! result: page fragments, several scalar reference sites, or tracked content
//! plus a bound widget each surface independently.
//! [`LiveSession::regions`](crate::LiveSession::regions) passes the backend's
//! entries through; consumers group by `field`. Later placements stay
//! reachable point-wise: [`field_at`](crate::LiveSession::field_at) resolves
//! a click on *any* placement, since one concrete point identifies one drawn
//! item whose origin is unambiguous.
//!
//! Regions are primarily a session-level query: the geometry is a property of
//! the current compile, re-read from the session per edit without producing
//! any byte artifact — the interactive-preview path (overlays over a
//! `paint`-ed canvas) reads it that way. A one-shot byte render carries the
//! same sidecar only on request ([`RenderOptions::regions`](crate::RenderOptions))
//! for consumers without a live session (static SVG overlays, PDF
//! post-processing, CI coverage probes). Either way regions are an overlay
//! sidecar, never a compositing input: every canvas backend hands back a
//! complete page raster, so nothing about the picture depends on reading a
//! region. Empty for backends that place no schema fields.

/// One schema field placement's extent on a rendered page.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF points with a **bottom-left** origin —
/// the same final geometry the stamp spine writes to the widget `/Rect`, so the
/// region and the rendered field describe the identical box.
///
/// `field` is **not** unique within the `Vec` that
/// [`LiveSession::regions`](crate::LiveSession::regions) returns: a content
/// field breaks into one entry **per segment** (paragraph, heading, whole code
/// fence) and per page each segment touches, a scalar referenced at several
/// plate sites yields one per site, and tracked content plus a bound widget
/// yields both. Consumers group by `field`; every entry routes to that field.
/// The whole-field box is **derived** — the union of a page's `span`-bearing
/// segment rects, so inter-paragraph whitespace stays uncovered (#829); the
/// [`field_boxes`] helper (and
/// [`LiveSession::field_boxes`](crate::LiveSession::field_boxes)) owns that
/// union so consumers need not reimplement it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedRegion {
    /// Quill schema field path, e.g. `"signature_block"` or
    /// `"$cards.indorsement.1.from"` — the author-facing field address (the
    /// card form is kind + 0-based ordinal, `$cards.<kind>.<n>.<field>`), not
    /// any backend widget name.
    pub field: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]`, PDF points, bottom-left origin.
    pub rect: [f32; 4],
    /// The corpus slice this box covers: USV `[start, end)` into the field's
    /// `RichText` for content ink (one segment's range), `None` for a scalar
    /// reference site or a widget — geometry with no corpus address. Additive
    /// and optional: omitted from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<[usize; 2]>,
}

impl RenderedRegion {
    /// Whether the point (`x`, `y`, PDF points, bottom-left origin) on `page`
    /// falls inside this region, edges inclusive. The one point-in-region
    /// predicate every `field_at` hit-test shares, so a click at a region
    /// border resolves identically everywhere.
    pub fn contains(&self, page: usize, x: f32, y: f32) -> bool {
        self.page == page
            && self.rect[0] <= x
            && x <= self.rect[2]
            && self.rect[1] <= y
            && y <= self.rect[3]
    }
}

/// The whole-field highlight boxes for `field`, derived from a region set: one
/// union rect per page, over that field's **`span`-bearing** (content) regions.
///
/// This owns the subtle part [`regions`](crate::LiveSession::regions) leaves to
/// consumers — filter by field, keep only the segment rects that carry a `span`,
/// union per page, inherit first-placement-only from the input — so a
/// "highlight the focused field" consumer never reimplements it and cannot
/// reintroduce the field-level union the #829 disjointness invariant exists to
/// prevent (the input is already striped; this unions the *bounding* box per
/// page, so inter-paragraph whitespace still is not a separate box but the
/// derived rect does bound it). Pass the output of
/// [`LiveSession::regions`](crate::LiveSession::regions) (or a one-shot
/// [`RenderOptions::regions`](crate::RenderOptions) sidecar); the convenience
/// [`LiveSession::field_boxes`](crate::LiveSession::field_boxes) reads the
/// session's own.
///
/// **Content only.** A scalar-reference site or a widget carries no `span`
/// ([`RenderedRegion::span`] is `None`), so a field placed *only* as a scalar
/// reference or a bound widget yields an empty result here — its highlight box
/// is a single region's `rect`, read straight from the region set with no
/// derivation. Each returned region carries the union `span`
/// (`[min start, max end)` over the page's contributing segments);
/// `page`-ascending.
pub fn field_boxes(regions: &[RenderedRegion], field: &str) -> Vec<RenderedRegion> {
    let mut by_page: Vec<RenderedRegion> = Vec::new();
    for r in regions
        .iter()
        .filter(|r| r.field == field && r.span.is_some())
    {
        let span = r.span.expect("filtered to span-bearing");
        match by_page.iter_mut().find(|acc| acc.page == r.page) {
            Some(acc) => {
                acc.rect[0] = acc.rect[0].min(r.rect[0]);
                acc.rect[1] = acc.rect[1].min(r.rect[1]);
                acc.rect[2] = acc.rect[2].max(r.rect[2]);
                acc.rect[3] = acc.rect[3].max(r.rect[3]);
                let s = acc.span.expect("union region carries a span");
                acc.span = Some([s[0].min(span[0]), s[1].max(span[1])]);
            }
            None => by_page.push(RenderedRegion {
                field: r.field.clone(),
                page: r.page,
                rect: r.rect,
                span: Some(span),
            }),
        }
    }
    by_page.sort_by_key(|r| r.page);
    by_page
}

/// How precisely a [`CorpusHit::pos`] resolved — the marker a caret UI reads to
/// decide whether to trust the offset. The value is never sub-cluster; the two
/// variants distinguish the finest this API offers from the segment floor it
/// degrades to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HitGranularity {
    /// Cluster-exact: `pos` is the first corpus char of the grapheme cluster
    /// under the point. The finest resolution — a char that escaped to several
    /// generated bytes (`*`→`\*`, `你`→3, the `//`→`\/\/` coupling) still floors
    /// to its cluster's first char, so this is *not* sub-character. A caret UI
    /// can place the caret at `pos` directly.
    Cluster,
    /// Segment-floored: the point landed on origin-less ink (list markers,
    /// numbering, a multi-line code fence's interior — spans that resolve to no
    /// single run), so `pos` degraded to the containing segment's corpus start
    /// rather than a wrong finer position. A caret UI should treat `pos` as the
    /// segment it selected, not a within-segment caret.
    Segment,
}

/// A resolved point → corpus position: the schema field a click landed in and
/// the USV offset into that field's `RichText`. The forward
/// [`position_at`](crate::LiveSession::position_at) direction, paired with
/// [`locate`](crate::LiveSession::locate) (corpus position → caret rect).
///
/// `pos` is **cluster-exact, not sub-character**: a hit inside a char that
/// escaped to several generated bytes (`*`→`\*`, `你`→3, the `//`→`\/\/`
/// coupling) floors to that cluster's first corpus char. A click on
/// origin-less ink (list markers, numbering, a multi-line code fence's interior
/// — spans that resolve to no single run) degrades to the containing segment's
/// corpus start rather than a wrong finer position, and a click off all content
/// ink resolves to nothing. [`granularity`](Self::granularity) reports which of
/// those two happened, so a caret UI need not guess.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusHit {
    /// The content field's schema path (same address space as
    /// [`RenderedRegion::field`]).
    pub field: String,
    /// USV offset into the field's `RichText`.
    pub pos: usize,
    /// Whether [`pos`](Self::pos) is cluster-exact or floored to the segment
    /// start ([`HitGranularity`]). `None` when the backend does not report it (a
    /// hit straight from a backend with no source map, or an older wire payload).
    /// Additive-optional: omitted from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<HitGranularity>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_round_trips_through_json() {
        let region = RenderedRegion {
            field: "full_name".to_string(),
            page: 0,
            rect: [180.0, 715.0, 520.0, 735.0],
            span: Some([12, 34]),
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"field\":\"full_name\""), "{json}");
        assert!(json.contains("\"span\":[12,34]"), "{json}");
        let back: RenderedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
    }

    /// `span` is omitted when `None` and defaults back on read — the
    /// additive-optional discipline that lets a scalar/widget region (no corpus
    /// address) parse the same as a content region carrying a span.
    #[test]
    fn optional_span_omitted_when_none() {
        let region = RenderedRegion {
            field: "subject".to_string(),
            page: 0,
            rect: [1.0, 2.0, 3.0, 4.0],
            span: None,
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(!json.contains("span"), "scalar region omits span: {json}");
        let back: RenderedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
    }

    #[test]
    fn corpus_hit_round_trips_through_json() {
        let hit = CorpusHit {
            field: "body".to_string(),
            pos: 42,
            granularity: Some(HitGranularity::Cluster),
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert!(json.contains("\"field\":\"body\"") && json.contains("\"pos\":42"));
        assert!(json.contains("\"granularity\":\"cluster\""), "{json}");
        let back: CorpusHit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, hit);
    }

    /// `granularity` omits when `None` and defaults back on read — the
    /// additive-optional discipline, so a hit straight from a backend (no source
    /// map) parses the same as the earlier hit shape lacking it.
    #[test]
    fn corpus_hit_omits_optionals_when_none() {
        let hit = CorpusHit {
            field: "body".to_string(),
            pos: 42,
            granularity: None,
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert!(
            !json.contains("granularity"),
            "unreported granularity omitted: {json}"
        );
        let back: CorpusHit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, hit);
    }

    /// The segment-floored variant serializes to its own tag, so a caret UI can
    /// tell a trusted cluster offset from a floored one.
    #[test]
    fn corpus_hit_segment_granularity_tag() {
        let hit = CorpusHit {
            field: "body".to_string(),
            pos: 7,
            granularity: Some(HitGranularity::Segment),
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert!(json.contains("\"granularity\":\"segment\""), "{json}");
        let back: CorpusHit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, hit);
    }

    fn content(field: &str, page: usize, rect: [f32; 4], span: [usize; 2]) -> RenderedRegion {
        RenderedRegion {
            field: field.to_string(),
            page,
            rect,
            span: Some(span),
        }
    }

    /// `field_boxes` unions a page's span-bearing segment rects into one box and
    /// ignores other fields — the whole-field highlight consumers used to derive
    /// by hand. The union `span` bounds `[min start, max end)`, and each page
    /// gets its own box, page-ascending.
    #[test]
    fn field_boxes_unions_span_bearing_segments_per_page() {
        let regions = vec![
            content("$body", 0, [10.0, 700.0, 200.0, 720.0], [0, 12]),
            content("$body", 0, [10.0, 660.0, 260.0, 680.0], [13, 40]),
            content("$body", 1, [10.0, 700.0, 150.0, 720.0], [41, 55]),
            content("subject", 0, [10.0, 740.0, 90.0, 752.0], [0, 5]),
        ];
        let boxes = field_boxes(&regions, "$body");
        assert_eq!(boxes.len(), 2, "one box per page $body touches");
        assert_eq!(boxes[0].page, 0);
        assert_eq!(boxes[0].rect, [10.0, 660.0, 260.0, 720.0], "page-0 union");
        assert_eq!(boxes[0].span, Some([0, 40]), "page-0 union span");
        assert_eq!(boxes[1].page, 1);
        assert_eq!(boxes[1].rect, [10.0, 700.0, 150.0, 720.0]);
    }

    /// A field placed only as a scalar reference or widget (no `span`) yields no
    /// derived content box — its highlight is a single region's `rect`, read
    /// straight from the set.
    #[test]
    fn field_boxes_empty_for_span_less_field() {
        let regions = vec![RenderedRegion {
            field: "subject".to_string(),
            page: 0,
            rect: [10.0, 740.0, 90.0, 752.0],
            span: None,
        }];
        assert!(field_boxes(&regions, "subject").is_empty());
    }
}
