//! Adds AcroForm SigField widgets to a typst_pdf-produced PDF via a
//! traditional incremental update — see the module docs in `inject.rs` and
//! `extract.rs` for details. The compiled Typst document supplies the field
//! placements via the `qm-sig` metadata labels emitted by the
//! `quillmark-helper` `signature-field` function.
//!
//! Public entry points called from `compile.rs`:
//! - [`extract`] — walk the Typst document and return placements.
//! - [`inject`] — apply placements to a PDF.

use quillmark_core::{Diagnostic, RenderError, Severity};
use typst::layout::PagedDocument;

mod extract;
mod inject;
mod scanner;

/// One signature field's name + page + rect in Typst (top-left origin) points.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SigPlacement {
    pub name: String,
    pub page: usize,
    /// `[x0, y0, x1, y1]` in Typst points, top-left origin. The PDF inject
    /// pass converts to bottom-left.
    pub rect_typst_pt: [f32; 4],
}

/// Internal errors raised by the inject pipeline. Surfaced to callers as
/// `RenderError::CompilationFailed` via `into_render_error`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SigOverlayError {
    #[error("missing startxref marker near EOF")]
    MissingStartxref,
    #[error("trailer marker not found")]
    MissingTrailer,
    #[error("/Root missing or malformed in trailer")]
    MissingRoot,
    #[error("/Size missing or malformed in trailer")]
    MissingSize,
    #[error("catalog object not found")]
    MissingCatalog,
    #[error("catalog /Pages reference not found")]
    MissingPagesRoot,
    #[error("page node object {id} not found")]
    MissingPageNode { id: u32 },
    #[error("page tree cycles through object {node}")]
    PageTreeCycle { node: u32 },
    #[error("PDF declares an xref stream; only traditional xref is supported")]
    XrefStreamUnsupported,
    #[error("PDF is encrypted; signature inject does not handle encrypted PDFs")]
    EncryptedPdfUnsupported,
    #[error(
        "page already declares /AcroForm; refusing to merge to avoid clobbering pre-existing form"
    )]
    PreExistingAcroForm,
    #[error("page tree resolved to zero pages")]
    NoPages,
    #[error(
        "page count mismatch: typst document has {typst} pages, PDF page tree has {pdf}"
    )]
    PageCountMismatch { typst: usize, pdf: usize },
    #[error("signature-field placement on page {page} but PDF has only {page_count} pages")]
    PagePlacementOutOfRange { page: usize, page_count: usize },
    #[error(
        "/Annots is encoded as an indirect reference; only inline arrays are supported \
         (typst-pdf 0.14 emits inline; future versions may require an update)"
    )]
    IndirectAnnotsUnsupported,
    #[error("/Annots value is malformed (not an array, not an indirect reference)")]
    MalformedAnnotsArray,
    #[error("pdf-writer chunk: failed to locate emitted object {id}")]
    PdfWriterChunkScan { id: u32 },
}

impl SigOverlayError {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingStartxref
            | Self::MissingTrailer
            | Self::MissingRoot
            | Self::MissingSize
            | Self::MissingCatalog
            | Self::MissingPagesRoot
            | Self::MissingPageNode { .. }
            | Self::MalformedAnnotsArray
            | Self::PdfWriterChunkScan { .. } => "typst::sig_overlay_pdf_parse",
            Self::PageTreeCycle { .. } => "typst::sig_overlay_page_cycle",
            Self::XrefStreamUnsupported => "typst::sig_overlay_xref_stream",
            Self::EncryptedPdfUnsupported => "typst::sig_overlay_encrypted",
            Self::PreExistingAcroForm => "typst::sig_overlay_existing_acroform",
            Self::NoPages | Self::PageCountMismatch { .. } | Self::PagePlacementOutOfRange { .. } => {
                "typst::sig_overlay_pages"
            }
            Self::IndirectAnnotsUnsupported => "typst::sig_overlay_indirect_annots",
        }
    }
}

impl From<SigOverlayError> for RenderError {
    fn from(e: SigOverlayError) -> Self {
        let code = e.code().to_string();
        RenderError::CompilationFailed {
            diags: vec![Diagnostic::new(Severity::Error, e.to_string()).with_code(code)],
        }
    }
}

/// Extract placements from a compiled Typst document. Returns `Vec::new()` if
/// the document contains no `signature-field` calls.
pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    extract::extract(doc)
}

/// Append SigField widgets and an AcroForm to a typst_pdf-produced PDF.
/// Returns `pdf` unchanged if `placements` is empty.
pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
) -> Result<Vec<u8>, RenderError> {
    inject::inject(pdf, doc, placements).map_err(Into::into)
}
