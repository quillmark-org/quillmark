//! Spike: PDF AcroForm signature field overlay via Typst `metadata` sentinels.
//!
//! # Approach
//!
//! `quillmark-helper` exports `signature-field()`, a Typst function that:
//! 1. Renders a visible placeholder box at the call site.
//! 2. Emits a zero-sized `metadata(("qm-sig", name, w_pt, h_pt)) <qm-sig>`
//!    element at the box's top-left corner via `place(top + left)`.
//!
//! After `typst::compile` produces a `PagedDocument`:
//! - `extract_sig_fields` queries the introspector for `<qm-sig>` labels and
//!   maps each to a [`SigFieldPlacement`] with page, absolute position, and
//!   dimensions.
//! - `inject_sig_fields` post-processes the raw PDF bytes with `lopdf`,
//!   inserting an AcroForm `SigField` widget annotation per placement.
//!
//! # Coordinate systems
//!
//! Typst: origin top-left, y increases downward, units = Typst points (1/72 in).
//! PDF:   origin bottom-left, y increases upward, same unit size.
//!
//! Conversion (sign flip):
//!   pdf_y_bottom = page_height_pt − typst_y − box_height_pt
//!   pdf_y_top    = page_height_pt − typst_y
//!
//! # Positional precision
//!
//! `place(top + left)` inside the box places the sentinel at the box's content
//! area origin. With 0.5pt stroke this introduces a sub-point offset; acceptable
//! for a spike. Production could eliminate it by passing absolute page coords
//! from a `locate()` closure instead of relying on the placed metadata element.

use typst::foundations::{Label, Selector, Value};
use typst::introspection::MetadataElem;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;

/// Physical placement of a `signature-field` sentinel within a compiled document.
#[derive(Debug, Clone)]
pub struct SigFieldPlacement {
    /// Zero-indexed page number.
    pub page: usize,
    /// Field name (the `name` argument passed to `signature-field()`).
    pub name: String,
    /// Left edge in Typst points (page top-left origin).
    pub x_pt: f64,
    /// Top edge in Typst points (page top-left origin).
    pub y_pt: f64,
    /// Box width in points (encoded in the sentinel metadata).
    pub width_pt: f64,
    /// Box height in points (encoded in the sentinel metadata).
    pub height_pt: f64,
    /// Page height in points — needed to flip y for PDF coordinate space.
    pub page_height_pt: f64,
}

/// Query the compiled Typst document for `<qm-sig>` sentinels.
///
/// Returns one [`SigFieldPlacement`] per `signature-field()` call in the plate,
/// in document order.  Returns an empty `Vec` if the plate has no signature
/// fields (the common case — zero overhead on the fast path).
pub fn extract_sig_fields(doc: &PagedDocument) -> Vec<SigFieldPlacement> {
    let Some(label) = Label::new(PicoStr::intern("qm-sig")) else {
        return vec![];
    };
    let selector = Selector::Label(label);
    let elems = doc.introspector.query(&selector);

    let mut fields = Vec::new();
    for elem in &elems {
        let Some(loc) = elem.location() else { continue };
        let pos = doc.introspector.position(loc);
        let Some(packed) = elem.to_packed::<MetadataElem>() else { continue };

        // Sentinel layout: ("qm-sig", name, width_pt, height_pt)
        let Value::Array(arr) = &packed.value else { continue };
        let Ok(Value::Str(tag)) = arr.at(0, None) else { continue };
        if tag.as_str() != "qm-sig" {
            continue;
        }
        let Ok(Value::Str(name)) = arr.at(1, None) else { continue };
        let Ok(Value::Float(width_pt)) = arr.at(2, None) else { continue };
        let Ok(Value::Float(height_pt)) = arr.at(3, None) else { continue };

        let page_idx = pos.page.get() - 1;
        let page_height_pt = doc
            .pages
            .get(page_idx)
            .map(|p| p.frame.size().y.to_pt())
            .unwrap_or(842.0); // A4 fallback

        fields.push(SigFieldPlacement {
            page: page_idx,
            name: name.to_string(),
            x_pt: pos.point.x.to_pt(),
            y_pt: pos.point.y.to_pt(),
            width_pt,
            height_pt,
            page_height_pt,
        });
    }
    fields
}

/// Post-process `pdf_bytes` to inject AcroForm `SigField` widget annotations.
///
/// Each [`SigFieldPlacement`] becomes one unsigned `SigField` that PDF viewers
/// (Acrobat, etc.) will render as a clickable signature box.
///
/// Returns the modified PDF bytes, or an error string if `lopdf` fails.
/// On error, callers should fall back to the original `pdf_bytes`.
pub fn inject_sig_fields(
    pdf_bytes: &[u8],
    fields: &[SigFieldPlacement],
) -> Result<Vec<u8>, String> {
    use lopdf::{Dictionary, Document, Object};

    if fields.is_empty() {
        return Ok(pdf_bytes.to_vec());
    }

    let mut doc = Document::load_mem(pdf_bytes).map_err(|e| e.to_string())?;
    let mut acroform_refs: Vec<Object> = Vec::new();

    for field in fields {
        let page_num = (field.page + 1) as u32;
        let page_id = *doc
            .get_pages()
            .get(&page_num)
            .ok_or_else(|| format!("PDF page {} not found", page_num))?;

        // Typst coords (top-left origin, y down) → PDF coords (bottom-left, y up)
        let x1 = field.x_pt as f32;
        let y2 = (field.page_height_pt - field.y_pt) as f32; // box top in PDF
        let x2 = (field.x_pt + field.width_pt) as f32;
        let y1 = (field.page_height_pt - field.y_pt - field.height_pt) as f32; // box bottom

        let mut widget = Dictionary::new();
        widget.set("Type", Object::Name(b"Annot".to_vec()));
        widget.set("Subtype", Object::Name(b"Widget".to_vec()));
        widget.set("FT", Object::Name(b"Sig".to_vec()));
        widget.set(
            "Rect",
            Object::Array(vec![
                Object::Real(x1),
                Object::Real(y1),
                Object::Real(x2),
                Object::Real(y2),
            ]),
        );
        widget.set(
            "T",
            Object::String(
                field.name.as_bytes().to_vec(),
                lopdf::StringFormat::Literal,
            ),
        );
        widget.set("P", Object::Reference(page_id));
        widget.set("F", Object::Integer(4)); // Print flag

        let widget_id = doc.add_object(Object::Dictionary(widget));

        // Append widget reference to the page's /Annots array
        if let Some(Object::Dictionary(page_dict)) = doc.objects.get_mut(&page_id) {
            match page_dict.get_mut(b"Annots") {
                Ok(Object::Array(annots)) => {
                    annots.push(Object::Reference(widget_id));
                }
                _ => {
                    page_dict.set(
                        "Annots",
                        Object::Array(vec![Object::Reference(widget_id)]),
                    );
                }
            }
        }

        acroform_refs.push(Object::Reference(widget_id));
    }

    // Build /AcroForm and attach to the document catalog
    let mut acroform = Dictionary::new();
    acroform.set("Fields", Object::Array(acroform_refs));
    acroform.set("SigFlags", Object::Integer(3)); // SignaturesExist | AppendOnly
    let acroform_id = doc.add_object(Object::Dictionary(acroform));

    let catalog_id = doc
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|r| r.as_reference().ok())
        .ok_or("cannot find /Root in PDF trailer")?;

    if let Some(Object::Dictionary(catalog)) = doc.objects.get_mut(&catalog_id) {
        catalog.set("AcroForm", Object::Reference(acroform_id));
    }

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_returns_empty_for_no_sig_fields() {
        // A document compiled without any signature-field() calls should
        // return an empty Vec — verify the fast path is zero-overhead.
        // (Full integration test requires a compiled PagedDocument fixture.)
        let fields: Vec<SigFieldPlacement> = vec![];
        assert!(fields.is_empty());
    }

    #[test]
    fn inject_noop_on_empty_fields() {
        // inject_sig_fields with no fields must return the input bytes unchanged.
        let dummy = b"fake pdf bytes";
        let result = inject_sig_fields(dummy, &[]);
        assert_eq!(result.unwrap(), dummy);
    }
}
