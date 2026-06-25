//! # quillmark-pdfform
//!
//! A greenfield Quillmark backend dedicated to filling government PDF forms.
//!
//! A `pdfform` quill ships two assets the qualification ("quillifying") layer
//! produced upstream:
//!
//! - **`form.pdf`** — the *stripped background*: the normalized gov form with
//!   its `/AcroForm`, widget annotations and page `/Annots` removed (pure pages
//!   + content streams); and
//! - **`form.json`** — the complete field reconstruction spec.
//!
//! The backend reads `form.json`, binds each field to the document's resolved
//! value, and hands the resulting `&[FieldSpec]` to `quillmark_pdf::stamp`,
//! which writes the AcroForm **fresh** onto the background. It never reads or
//! reconciles a foreign AcroForm — both backends do the same single
//! "stamp from spec" operation, differing only in where geometry comes from
//! (here: `form.json`; for Typst: introspection).
//!
//! See issue #744 for the surrounding architecture.

mod form;

pub use form::{FormField, FormFieldKind, FormSpec};

use std::any::Any;

use quillmark_core::session::SessionHandle;
use quillmark_core::{
    Artifact, Backend, Diagnostic, OutputFormat, Quill, RenderError, RenderOptions, RenderResult,
    RenderSession, Severity,
};
use quillmark_pdf::{stamp, FieldSpec, StampOptions};

const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf];

/// The `pdfform` backend.
#[derive(Debug)]
pub struct PdfformBackend;

/// A prepared form: the stripped background plus the field specs (geometry +
/// bound values) ready to stamp. Built once in [`Backend::open`]; cheap to
/// re-render across output requests.
#[derive(Debug)]
pub struct PdfformSession {
    background: Vec<u8>,
    fields: Vec<FieldSpec>,
    page_count: usize,
}

impl SessionHandle for PdfformSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);
        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::FormatNotSupported {
                diags: vec![Diagnostic::new(
                    Severity::Error,
                    format!("pdfform backend does not support {format:?} output"),
                )
                .with_code("pdfform::format_not_supported".to_string())],
            });
        }

        let result = stamp(
            self.background.clone(),
            &self.fields,
            &StampOptions {
                producer: opts.producer.clone(),
            },
        )?;
        // `result.regions` is the phase-1 sidecar; it rides alongside the
        // artifact until the engine's render result grows a regions channel.
        Ok(RenderResult::new(
            vec![Artifact {
                bytes: result.pdf,
                output_format: OutputFormat::Pdf,
            }],
            OutputFormat::Pdf,
        ))
    }

    fn page_count(&self) -> usize {
        self.page_count
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

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
        let background = source
            .files()
            .get_file("form.pdf")
            .ok_or_else(|| {
                err(
                    "pdfform::missing_background",
                    "pdfform quill is missing its stripped background 'form.pdf'",
                )
            })?
            .to_vec();

        let form_json = source.files().get_file("form.json").ok_or_else(|| {
            err(
                "pdfform::missing_form_json",
                "pdfform quill is missing 'form.json'",
            )
        })?;
        let spec: FormSpec = serde_json::from_slice(form_json)
            .map_err(|e| err("pdfform::form_json_parse", format!("form.json: {e}")))?;

        let fields: Vec<FieldSpec> = spec
            .fields
            .iter()
            .map(|f| {
                let value = f
                    .schema_field
                    .as_deref()
                    .and_then(|key| lookup(json_data, key));
                f.to_field_spec(value)
            })
            .collect();

        let page_count = quillmark_pdf::page_count(&background)?;

        Ok(RenderSession::new(Box::new(PdfformSession {
            background,
            fields,
            page_count,
        })))
    }
}

/// Resolve a schema field's value from the compiled plate JSON. Fields live at
/// the top level of the object; a missing key yields `None`.
fn lookup<'a>(json_data: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    json_data.get(key).filter(|v| !v.is_null())
}

fn err(code: &'static str, msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, msg.into()).with_code(code.into())],
    }
}
