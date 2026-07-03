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
//! - **Content fields** (a markdown body, a `markdown[]` element, a card's
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
/// field's first placement breaking across pages yields one entry per page,
/// a scalar referenced at several plate sites yields one per site, and
/// tracked content plus a bound widget yields both. Consumers group by
/// `field`; every entry routes to that field.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_round_trips_through_json() {
        let region = RenderedRegion {
            field: "full_name".to_string(),
            page: 0,
            rect: [180.0, 715.0, 520.0, 735.0],
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"field\":\"full_name\""), "{json}");
        let back: RenderedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
    }
}
