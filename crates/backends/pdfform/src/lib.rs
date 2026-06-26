//! # quillmark-pdfform — the PDF-form backend
//!
//! A Typst-free Quillmark backend dedicated to filling government PDF forms —
//! something the Typst backend fundamentally cannot do. A `pdfform` quill ships
//! two assets the (out-of-scope) qualification layer produced upstream:
//!
//! - **`form.pdf`** — the *stripped background*: the normalized gov form with
//!   its `/AcroForm`, widget annotations, and page `/Annots` removed.
//! - **`form.json`** — the complete, value-free field reconstruction spec.
//!
//! The backend reads both, binds document values against `compile_data`, and
//! writes a fresh AcroForm onto the background via the shared `quillmark-pdf`
//! stamping spine. It never reads or reconciles a foreign AcroForm. Both
//! backends collapse to the same `&[FieldSpec]` seam; they differ only in where
//! geometry and values come from.

mod form;
mod resolve;

pub use form::{FieldKind, FormField, FormParseError, FormSpec, Rect};

use std::any::Any;

use quillmark_core::session::SessionHandle;
use quillmark_core::{
    Artifact, Backend, Diagnostic, OutputFormat, Quill, RenderError, RenderOptions, RenderResult,
    RenderSession, Severity,
};
use quillmark_pdf::{stamp, FieldSpec, PdfError, StampOptions};

/// Conventional filenames a `pdfform` quill ships at its root.
const FORM_PDF: &str = "form.pdf";
const FORM_JSON: &str = "form.json";

const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf];

/// The PDF-form backend.
#[derive(Debug, Default)]
pub struct PdfformBackend;

impl Backend for PdfformBackend {
    fn id(&self) -> &'static str {
        "pdfform"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    fn open(
        &self,
        _plate_content: &str,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<RenderSession, RenderError> {
        let files = source.files();
        let base_pdf = files
            .get_file(FORM_PDF)
            .ok_or_else(|| {
                engine_err(
                    "pdfform::missing_form_pdf",
                    format!("pdfform quill is missing its `{FORM_PDF}` background"),
                )
            })?
            .to_vec();
        let form_json = files.get_file(FORM_JSON).ok_or_else(|| {
            engine_err(
                "pdfform::missing_form_json",
                format!("pdfform quill is missing its `{FORM_JSON}` field spec"),
            )
        })?;

        let spec = FormSpec::parse(form_json)
            .map_err(|e| engine_err("pdfform::invalid_form_json", e.to_string()))?;

        // Page boxes drive the top-left → bottom-left flip (honouring a
        // non-zero page origin); reading them from the background also surfaces
        // a malformed/out-of-contract base early.
        let page_boxes = quillmark_pdf::page_media_boxes(&base_pdf).map_err(map_pdf_err)?;
        let page_count = page_boxes.len();

        let mut field_specs: Vec<FieldSpec> = Vec::with_capacity(spec.fields.len());
        for field in &spec.fields {
            let media_box = page_boxes.get(field.page).copied().ok_or_else(|| {
                engine_err(
                    "pdfform::field_page_out_of_range",
                    format!(
                        "field {:?} targets page {} but `{FORM_PDF}` has {page_count} page(s)",
                        field.name, field.page
                    ),
                )
            })?;
            field_specs.push(resolve::field_spec(field, media_box, json_data));
        }

        Ok(RenderSession::new(Box::new(PdfformSession {
            base_pdf,
            field_specs,
            page_count,
        })))
    }
}

/// A `pdfform` render session: the stripped background plus the resolved field
/// specs, ready to stamp on each `render`.
#[derive(Debug)]
struct PdfformSession {
    base_pdf: Vec<u8>,
    field_specs: Vec<FieldSpec>,
    page_count: usize,
}

impl SessionHandle for PdfformSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);
        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::FormatNotSupported {
                diags: vec![Diagnostic::new(
                    Severity::Error,
                    format!("{format:?} not supported by the pdfform backend"),
                )
                .with_code("pdfform::format_not_supported".to_string())
                .with_hint(format!("Supported formats: {SUPPORTED_FORMATS:?}"))],
            });
        }

        // The producer threads from the product layer, else the backend default.
        let producer = Some(opts.producer.clone().unwrap_or_else(default_producer));
        let stamped = stamp(
            self.base_pdf.clone(),
            &self.field_specs,
            &StampOptions { producer },
        )
        .map_err(map_pdf_err)?;

        Ok(RenderResult::new(
            vec![Artifact {
                bytes: stamped.pdf,
                output_format: OutputFormat::Pdf,
            }],
            OutputFormat::Pdf,
        )
        .with_regions(stamped.regions))
    }

    fn page_count(&self) -> usize {
        self.page_count
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Default `/Producer`: `Quillmark <crate-version>`, owned by the backend (the
/// product layer), never defaulted from the leaf spine's version.
fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}

/// Map a stamp-spine [`PdfError`] to the backend's `RenderError` at the boundary.
fn map_pdf_err(e: PdfError) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, e.message).with_code(e.code.to_string())],
    }
}

/// A single-diagnostic `EngineCreation` error with `code`.
fn engine_err(code: &str, message: impl Into<String>) -> RenderError {
    RenderError::EngineCreation {
        diags: vec![Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string())],
    }
}
