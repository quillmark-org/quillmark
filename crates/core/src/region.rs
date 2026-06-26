//! Rendered-region sidecar carried on every [`RenderResult`](crate::RenderResult).
//!
//! A region records *where a named field landed* on the rendered page —
//! geometry plus the kind of field and its bound value. It is the only path to
//! field values in non-interactive output: under the stamping engine's
//! Technique A (real AcroForm fields + `/NeedAppearances`, no baked appearance
//! streams), a flat rasterizer renders the widgets blank, so a consumer that
//! must composite values (a canvas preview, a server-side flattener) reads them
//! from here.
//!
//! Regions ride on *every* render regardless of output format — a GUI overlay
//! needs the geometry whether it shows the PDF or a rastered background — and
//! default to empty for backends that produce none.

/// One field's placement on a rendered page.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF points with a **bottom-left** origin —
/// the same final geometry the stamp spine writes to the widget `/Rect`, so the
/// region and the stamped widget describe the identical box.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedRegion {
    /// Fully-qualified field name (matches the widget `/T`).
    pub name: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]`, PDF points, bottom-left origin.
    pub rect: [f32; 4],
    /// What kind of region this is, plus its kind-specific payload.
    pub kind: RegionKind,
}

/// The kind of a [`RenderedRegion`] and its payload.
///
/// An enum from day one (rather than a bare string) so future region kinds —
/// e.g. a flattened-value glyph run carrying resolved typography — extend it
/// additively without reshaping the sidecar.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RegionKind {
    /// An interactive form field. `field_type` is the lowercase field-type id
    /// (`"text"`, `"checkbox"`, `"choice"`, `"signature"`); `value` is the bound
    /// value, `None` for a blank/unbound field.
    Field {
        #[serde(rename = "fieldType")]
        field_type: String,
        value: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_round_trips_through_json() {
        let region = RenderedRegion {
            name: "FullName".to_string(),
            page: 0,
            rect: [180.0, 715.0, 520.0, 735.0],
            kind: RegionKind::Field {
                field_type: "text".to_string(),
                value: Some("Ada Lovelace".to_string()),
            },
        };
        let json = serde_json::to_string(&region).unwrap();
        // camelCase field names and the internally-tagged kind.
        assert!(json.contains("\"fieldType\":\"text\""), "{json}");
        assert!(json.contains("\"type\":\"field\""), "{json}");
        let back: RenderedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
    }

    #[test]
    fn blank_field_value_is_null() {
        let region = RenderedRegion {
            name: "Signature".to_string(),
            page: 0,
            rect: [0.0, 0.0, 10.0, 10.0],
            kind: RegionKind::Field {
                field_type: "signature".to_string(),
                value: None,
            },
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"value\":null"), "{json}");
    }
}
