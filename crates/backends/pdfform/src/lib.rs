//! # quillmark-pdfform — the PDF-form backend
//!
//! A Typst-free Quillmark backend dedicated to filling existing PDF forms —
//! something the Typst backend fundamentally cannot do. A `pdfform` quill ships
//! two assets the (out-of-scope) qualification layer produced upstream:
//!
//! - **`form.pdf`** — the *stripped background*: the normalized form with
//!   its `/AcroForm`, widget annotations, and page `/Annots` removed.
//! - **`form.json`** — the complete, value-free field reconstruction spec.
//!
//! The backend reads both, binds document values against `compile_data`, and
//! writes a fresh AcroForm onto the background via the shared `quillmark-pdf`
//! stamping spine. It never reads or reconciles a foreign AcroForm. Both
//! backends collapse to the same `&[FieldSpec]` seam; they differ only in where
//! geometry and values come from.

#[cfg(feature = "preview")]
mod flatten;
mod form;
mod resolve;
#[cfg(feature = "preview")]
mod typography;

pub use form::{FieldKind, FormField, FormParseError, FormSpec, Rect};

use std::any::Any;

#[cfg(feature = "preview")]
use flatten::flatten as flatten_to_pdf;
use quillmark_core::session::SessionHandle;
use quillmark_core::{
    Artifact, Backend, Diagnostic, OutputFormat, Quill, RenderError, RenderOptions, RenderResult,
    RenderSession, Severity,
};
#[cfg(feature = "preview")]
use quillmark_pdf::regions_of;
use quillmark_pdf::{stamp, FieldSpec, PdfError, StampOptions};

#[cfg(feature = "preview")]
use {
    hayro::hayro_interpret::{font::FontQuery, InterpreterSettings},
    hayro::hayro_syntax::Pdf as HayroPdf,
    hayro::{render as hayro_render, RenderCache, RenderSettings},
    hayro_svg::{convert as hayro_svg_convert, RenderCache as SvgCache, SvgRenderSettings},
    std::sync::Arc,
};

/// Conventional filenames a `pdfform` quill ships at its root.
const FORM_PDF: &str = "form.pdf";
const FORM_JSON: &str = "form.json";

#[cfg(not(feature = "preview"))]
const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf];
#[cfg(feature = "preview")]
const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf, OutputFormat::Svg];

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

    #[cfg(feature = "preview")]
    fn supports_canvas(&self) -> bool {
        true
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

        // Under `preview`, pre-flatten once so render_rgba / SVG renders have
        // a ready-to-rasterize flat PDF without re-running flatten on every
        // paint call. The producer string only goes in the PDF Info dict and
        // doesn't affect rasterisation, so None is fine here.
        #[cfg(feature = "preview")]
        let flat_pdf = flatten_to_pdf(
            base_pdf.clone(),
            &field_specs,
            &StampOptions { producer: None },
        )
        .map(|r| r.pdf)
        .map_err(map_pdf_err)?;

        Ok(RenderSession::new(Box::new(PdfformSession {
            base_pdf,
            field_specs,
            page_count,
            #[cfg(feature = "preview")]
            page_boxes,
            #[cfg(feature = "preview")]
            flat_pdf,
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
    /// Page media-boxes from the background; used by `page_size_pt` under
    /// `preview` without reparsing the PDF on every canvas-paint call.
    #[cfg(feature = "preview")]
    page_boxes: Vec<[f32; 4]>,
    /// Pre-flattened PDF (values baked as content-stream operators) ready for
    /// hayro rasterisation. Produced once at session-open time.
    #[cfg(feature = "preview")]
    flat_pdf: Vec<u8>,
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

        // SVG output: convert the pre-flattened PDF to SVG via hayro-svg.
        #[cfg(feature = "preview")]
        if format == OutputFormat::Svg {
            return self.render_svg();
        }

        // The producer threads from the product layer, else the backend default.
        // PDF output is always an interactive AcroForm (Technique A / `stamp`);
        // value-flattening is internal preview-only machinery, never a PDF
        // deliverable.
        let producer = Some(opts.producer.clone().unwrap_or_else(default_producer));
        let stamp_opts = StampOptions { producer };
        let stamped =
            stamp(self.base_pdf.clone(), &self.field_specs, &stamp_opts).map_err(map_pdf_err)?;

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

    /// Page dimensions in PDF points derived from the background's `/MediaBox`.
    /// Available only under the `preview` feature; the default `None` satisfies
    /// the canvas-capability contract when `preview` is off.
    #[cfg(feature = "preview")]
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        let [x0, y0, x1, y1] = *self.page_boxes.get(page)?;
        Some((x1 - x0, y1 - y0))
    }

    /// Rasterise `page` of the pre-flattened PDF via hayro. Field values are
    /// baked into the flat PDF as content-stream operators, so they appear in
    /// the raster without any regions-compositing by the caller.
    #[cfg(feature = "preview")]
    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        use hayro::vello_cpu::color::palette::css::WHITE;

        let pdf = HayroPdf::new(self.flat_pdf.clone()).ok()?;
        let p = pdf.pages().get(page)?;
        let cache = RenderCache::new();
        let interp = standard_font_settings();
        let render_settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        };
        let pixmap = hayro_render(p, &cache, &interp, &render_settings);
        let w = pixmap.width() as u32;
        let h = pixmap.height() as u32;
        let bytes: Vec<u8> = pixmap
            .take_unpremultiplied()
            .into_iter()
            .flat_map(|px| [px.r, px.g, px.b, px.a])
            .collect();
        Some((w, h, bytes))
    }
}

#[cfg(feature = "preview")]
impl PdfformSession {
    /// Render all pages as SVG via hayro-svg, using the pre-flattened PDF.
    fn render_svg(&self) -> Result<RenderResult, RenderError> {
        let pdf = HayroPdf::new(self.flat_pdf.clone()).map_err(|_| {
            engine_err(
                "pdfform::svg_parse_failed",
                "failed to parse pre-flattened PDF for SVG render",
            )
        })?;
        let interp = standard_font_settings();
        let svg_settings = SvgRenderSettings {
            bg_color: [255, 255, 255, 255],
        };
        let artifacts: Vec<Artifact> = pdf
            .pages()
            .iter()
            .map(|page| {
                let cache = SvgCache::new();
                let svg = hayro_svg_convert(page, &cache, &interp, &svg_settings);
                Artifact {
                    bytes: svg.into_bytes(),
                    output_format: OutputFormat::Svg,
                }
            })
            .collect();

        let regions = regions_of(&self.field_specs);
        Ok(RenderResult::new(artifacts, OutputFormat::Svg).with_regions(regions))
    }
}

/// Build an `InterpreterSettings` that satisfies standard Type1 font queries
/// (Helvetica, ZapfDingbats, etc.) using hayro's embedded font data.
/// Required for rendering the flat PDF's Helv and ZaDb content streams.
#[cfg(feature = "preview")]
fn standard_font_settings() -> InterpreterSettings {
    InterpreterSettings {
        font_resolver: Arc::new(|query| match query {
            FontQuery::Standard(s) => Some(s.get_font_data()),
            FontQuery::Fallback(f) => Some(f.pick_standard_font().get_font_data()),
        }),
        ..Default::default()
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
