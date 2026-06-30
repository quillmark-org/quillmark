//! Schema-field geometry, queried from a compiled
//! [`RenderSession`](crate::RenderSession) via
//! [`regions`](crate::RenderSession::regions).
//!
//! A region ties a rectangle on the rendered page to the **quill schema field**
//! that produced it — the address the document author already uses to refer to
//! that field (the same address the Typst plate reads as `data.*` and the
//! pdfform binder resolves against `compile_data`). It exists so a consumer can
//! map between a place on the page and a field in the editor: click a rendered
//! field → focus it in the editor, or highlight the page rectangle for the
//! focused field.
//!
//! Two producers feed regions, both keyed on the schema path:
//!
//! - **Content fields** (a markdown body) auto-tag from their content — the
//!   Typst eval site brackets each one with its schema address, so the key is
//!   carried by construction with no plate-author effort. Two cases produce no
//!   region: a computed scalar (`data.from + ", " + rank`) cannot be tagged at
//!   all (a string has no label), and a field that is blank or draws nothing
//!   (an empty or whitespace-only body) is tagged but has no inked extent to
//!   bound — present-but-empty is not the same as placed.
//! - **Form-field widgets** carry a schema path explicitly: pdfform binds it
//!   from the form mapping; a Typst `form-field` binds it from its `field:`
//!   argument. A widget that binds none produces **no** region — its backend
//!   identifier (the `/T` widget name) is not a schema address, so there is
//!   nothing for a consumer to route to. Only schema-addressable fields surface
//!   a region.
//!
//! **One region per logical field.** A field can arise from more than one
//! source — a content auto-tag *and* a `field:`-bound widget — or as several
//! page-fragments of a body that breaks across pages. [`RenderSession::regions`]
//! collapses these to one entry per `field`: a bound widget wins over a content
//! tag (the explicit binding is the author's deliberate mapping), and a
//! page-spanning body keeps the first page it occupies as its anchor. A consumer
//! looks a field up and gets exactly one rectangle.
//!
//! Regions are a session-level query, not a render output: the geometry is a
//! property of the compiled snapshot, read once from the session without
//! producing any byte artifact. Only the interactive-preview path wants it (to
//! lay out overlays over a `paint`-ed canvas); a one-shot byte render
//! (PDF/PNG/SVG) never does. They are an overlay sidecar, never a compositing
//! input: both canvas backends hand back a complete page raster, so nothing
//! about the picture depends on reading a region. Empty for backends that place
//! no schema fields.

/// One schema field's placement on a rendered page.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF points with a **bottom-left** origin —
/// the same final geometry the stamp spine writes to the widget `/Rect`, so the
/// region and the rendered field describe the identical box.
///
/// `field` is unique within the `Vec` that [`RenderSession::regions`] returns —
/// one region per logical schema field. (A backend's [`SessionHandle::regions`]
/// may emit a field more than once in precedence order; the session wrapper
/// keeps the first.)
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
