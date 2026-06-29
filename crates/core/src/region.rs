//! Rendered-region sidecar carried on every [`RenderResult`](crate::RenderResult).
//!
//! A region ties a rectangle on the rendered page to the **quill schema field**
//! that produced it — the address the document author already uses to refer to
//! that field (the same address the Typst plate reads as `data.*` and the
//! pdfform binder resolves against `compile_data`). It exists so a consumer can
//! map between a place on the page and a field in the editor: click a rendered
//! field → focus it in the editor, or highlight the page rectangle for the
//! focused field.
//!
//! Backend-internal identifiers never appear here. A pdfform AcroForm widget
//! name (`/T`) is an implementation detail of the stamp spine; the region
//! carries the schema path it maps to (`signature_block`), not the widget name
//! (`Signature`). A region is emitted only for a field with a schema address —
//! an unbound decorative widget produces none.
//!
//! Regions ride on *every* render regardless of output format — a GUI overlay
//! needs the geometry whether it shows the PDF or a rastered background — and
//! default to empty for backends that produce none. They are an overlay
//! sidecar, never a compositing input: both canvas backends hand back a
//! complete page raster, so nothing about the picture depends on reading a
//! region.

/// One schema field's placement on a rendered page.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF points with a **bottom-left** origin —
/// the same final geometry the stamp spine writes to the widget `/Rect`, so the
/// region and the rendered field describe the identical box.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedRegion {
    /// Quill schema field path, e.g. `"signature_block"` or
    /// `"$cards.indorsement.1.from"` — the author-facing field address, not any
    /// backend widget name.
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
