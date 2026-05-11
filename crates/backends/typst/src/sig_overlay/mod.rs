//! Adds AcroForm SigField widgets to a typst_pdf-produced PDF via an
//! incremental update. Public entry points called from `compile.rs`:
//! [`extract`] walks the Typst document and returns placements; [`inject`]
//! applies them to the PDF.

use quillmark_core::{Diagnostic, RenderError, Severity};
use typst::layout::PagedDocument;

mod extract;
mod inject;
mod scanner;

/// One signature field's name + page + rect in Typst (top-left origin) points.
/// The PDF inject pass converts to bottom-left.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SigPlacement {
    pub name: String,
    pub page: usize,
    pub rect_typst_pt: [f32; 4],
}

/// Build a single-Diagnostic `RenderError` with the given code. Used at every
/// fail site in `scanner`/`inject`/`extract` — the alternative was a 17-variant
/// enum + `From` impl that all collapsed to this anyway.
pub(super) fn err(code: &'static str, msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, msg.into()).with_code(code.into())],
    }
}

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    extract::extract(doc)
}

pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
) -> Result<Vec<u8>, RenderError> {
    inject::inject(pdf, doc, placements)
}
