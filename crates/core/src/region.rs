//! Schema-field geometry, queried from a compiled
//! [`LiveSession`](crate::LiveSession) via
//! [`regions`](crate::LiveSession::regions).
//!
//! A region ties a rectangle on the rendered page to the **quill schema field**
//! that produced it — the address the document author already uses to refer to
//! that field (the same address the Typst plate reads as `data.*` and the
//! pdfform binder resolves against `compile_data`). It exists so a consumer can
//! map between a place on the page and a field in the editor: click a rendered
//! field → focus it in the editor, or highlight the page rectangle for the
//! focused field.
//!
//! Three producers feed regions, all keyed on the schema path:
//!
//! - **Content fields** (a markdown body) auto-tag from their content — the
//!   Typst eval site brackets each one's *value* with its schema address, so
//!   the key is carried by construction when the plate places the value
//!   verbatim. Three cases produce no region: a computed scalar (`data.from +
//!   ", " + rank`) cannot be tagged at all (a string has no label); a field
//!   that is blank or draws nothing (an empty or whitespace-only body) is
//!   tagged but has no inked extent to bound — present-but-empty is not the
//!   same as placed; and a value passed through a package that **rebuilds its
//!   content** (a `show`-rule pass that captures paragraphs into a state
//!   buffer and re-emits them) loses the markers in the reconstruction — the
//!   auto-tag contract is *content placed verbatim*, not arbitrary Typst.
//! - **Explicit `tagged(field)[..]` placements** bracket whatever a plate
//!   places at that site — the recovery for both auto-tag gaps: a scalar
//!   becomes taggable as content at its placement site, and markers wrapped
//!   *around* a package call's output survive whatever the package does
//!   internally. The region covers the ink of the placement (package chrome
//!   included), where an auto-tag covers the value's own ink.
//! - **Form-field widgets** carry a schema path explicitly: pdfform binds it
//!   from the form mapping; a Typst `form-field` binds it from its `field:`
//!   argument. A widget that binds none produces **no** region — its backend
//!   identifier (the `/T` widget name) is not a schema address, so there is
//!   nothing for a consumer to route to. Only schema-addressable fields surface
//!   a region.
//!
//! **One region per (placement, page fragment).** A field placed at two sites
//! surfaces two independent regions — a spanning union would claim the ink of
//! whatever sits between them — and a body that breaks across pages surfaces
//! one fragment per page it touches, so highlighting a focused field covers
//! its continuation pages. A field arising from both tagged content and a
//! bound widget surfaces both (overlapping rects that route to the same
//! field). [`LiveSession::regions`](crate::LiveSession::regions) passes the
//! backend's entries through; consumers group by `field`. Nested markers for
//! the same field collapse into their outer placement — double-tagging one
//! placement is not placing it twice.
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
/// [`LiveSession::regions`](crate::LiveSession::regions) returns: a field
/// placed at several sites, breaking across pages, or arising from both
/// tagged content and a bound widget yields one entry per (placement, page
/// fragment). Consumers group by `field`; every entry routes to that field.
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
