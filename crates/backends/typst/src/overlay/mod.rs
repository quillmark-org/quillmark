//! The signature-field adapter: a thin introspection→[`FieldSpec`] bridge onto
//! the shared `quillmark-pdf` stamping spine.
//!
//! Public entry points called from `compile.rs`: [`extract`] walks the Typst
//! document for `signature-field` placements; [`build_field_specs`] converts
//! those (Typst top-left origin) into spine [`FieldSpec`]s (PDF bottom-left
//! origin) — coordinate ownership lives here, in the backend, so the spine
//! never imports `typst_layout`. [`default_producer`] supplies the default
//! `/Info` `/Producer` string the product layer threads down.

use quillmark_core::{Diagnostic, RenderError, Severity};
use quillmark_pdf::{FieldSpec, FieldType};
use typst_layout::PagedDocument;

mod extract;

/// One signature field's name + page + rect in Typst (top-left origin) points.
/// [`build_field_specs`] converts to the spine's bottom-left geometry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SigPlacement {
    pub name: String,
    pub page: usize,
    pub rect_typst_pt: [f32; 4],
}

/// Build a single-`Diagnostic` `RenderError` with `code`. Shared by the
/// adapter and the introspection walk: every fail site just needs a code plus a
/// message.
pub(crate) fn err(code: &'static str, msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, msg.into()).with_code(code.into())],
    }
}

/// Default `/Producer` value: `Quillmark <crate-version>`. Owned by the backend
/// (the product layer), never defaulted from the leaf spine's version.
pub(crate) fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    extract::extract(doc)
}

/// Convert signature placements into spine [`FieldSpec`]s, flipping each rect
/// from Typst's top-left origin to the PDF bottom-left origin the spine
/// consumes. Page heights come from the Typst document (the geometry source);
/// the spine only ever sees final bottom-left geometry.
pub(crate) fn build_field_specs(
    doc: &PagedDocument,
    placements: &[SigPlacement],
) -> Result<Vec<FieldSpec>, RenderError> {
    let page_heights: Vec<f32> = doc
        .pages()
        .iter()
        .map(|p| p.frame.size().y.to_pt() as f32)
        .collect();

    placements
        .iter()
        .map(|p| {
            let page_h = *page_heights.get(p.page).ok_or_else(|| {
                err(
                    "typst::signature_page_out_of_range",
                    format!(
                        "signature-field {:?} targets page {} but the document has {} page(s)",
                        p.name,
                        p.page,
                        page_heights.len()
                    ),
                )
            })?;
            let [x0, y0, x1, y1] = p.rect_typst_pt;
            Ok(FieldSpec {
                name: p.name.clone(),
                page: p.page,
                // Typst top-left → PDF bottom-left.
                rect: [x0, page_h - y1, x1, page_h - y0],
                field_type: FieldType::Signature,
                value: None,
                tooltip: None,
            })
        })
        .collect()
}
