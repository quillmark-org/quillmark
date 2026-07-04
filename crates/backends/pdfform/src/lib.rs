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

mod flatten;
mod form;
mod resolve;
mod typography;

use flatten::flatten as flatten_to_pdf;
use form::FormSpec;
use quillmark_core::session::SessionHandle;
use quillmark_core::{
    Artifact, Backend, ChangeSet, Diagnostic, LiveSession, OutputFormat, Quill, RenderError,
    RenderOptions, RenderResult, RenderedRegion, Severity,
};
use quillmark_pdf::regions_of;
use quillmark_pdf::{stamp, FieldSpec, PdfError, StampOptions};

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

/// Default raster resolution for PNG output (2× at 72 pt/in), matching the
/// core `RenderOptions::ppi` default and the Typst backend.
const DEFAULT_PPI: f32 = 144.0;

const SUPPORTED_FORMATS: &[OutputFormat] =
    &[OutputFormat::Pdf, OutputFormat::Svg, OutputFormat::Png];

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
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<LiveSession, RenderError> {
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

        let field_specs = resolve_field_specs(&spec, &page_boxes, json_data)?;

        // Pre-flatten once so render_rgba / SVG / PNG renders have a
        // ready-to-rasterize flat PDF without re-running flatten on every
        // paint call.
        let flat_pdf = flatten_to_pdf(base_pdf.clone(), &field_specs).map_err(map_pdf_err)?;

        Ok(LiveSession::new(Box::new(PdfformSession {
            base_pdf,
            spec,
            field_specs,
            page_boxes,
            flat_pdf,
        })))
    }
}

/// Resolve the form spec's fields against document data — the per-document
/// half of `open`, re-run by each `apply`.
fn resolve_field_specs(
    spec: &FormSpec,
    page_boxes: &[[f32; 4]],
    json_data: &serde_json::Value,
) -> Result<Vec<FieldSpec>, RenderError> {
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
    Ok(field_specs)
}

/// A `pdfform` render session: the stripped background plus the resolved field
/// specs, ready to stamp on each `render`.
#[derive(Debug)]
struct PdfformSession {
    base_pdf: Vec<u8>,
    /// The parsed `form.json` field definitions; re-resolved against new
    /// document data by each `apply`.
    spec: FormSpec,
    field_specs: Vec<FieldSpec>,
    /// Page media-boxes from the background; used by `page_size_pt` without
    /// reparsing the PDF on every canvas-paint call. Its length is the page
    /// count (the background is fixed for the session).
    page_boxes: Vec<[f32; 4]>,
    /// Pre-flattened PDF (values baked as content-stream operators) ready for
    /// hayro rasterisation. Produced once at session-open time.
    flat_pdf: Vec<u8>,
}

impl SessionHandle for PdfformSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);
        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::from_diag(
                Diagnostic::new(
                    Severity::Error,
                    format!("{format:?} not supported by the pdfform backend"),
                )
                .with_code("pdfform::format_not_supported".to_string())
                .with_hint(format!("Supported formats: {SUPPORTED_FORMATS:?}")),
            ));
        }

        // Raster/vector output: rasterise the pre-flattened PDF via hayro.
        // SVG is vector (hayro-svg); PNG is raster at `opts.ppi`. Both bake the
        // values in, so they render in any viewer (no appearance synthesis).
        if format == OutputFormat::Svg {
            return self.render_svg();
        }
        if format == OutputFormat::Png {
            let scale = opts.ppi.unwrap_or(DEFAULT_PPI) / 72.0;
            return self.render_png(scale);
        }

        // The producer threads from the product layer, else the backend default.
        // PDF output is always an interactive AcroForm (Technique A / `stamp`);
        // value-flattening is internal raster machinery (SVG/PNG/canvas),
        // never a PDF deliverable.
        let producer = Some(opts.producer.clone().unwrap_or_else(default_producer));
        let stamp_opts = StampOptions { producer };
        let stamped =
            stamp(self.base_pdf.clone(), &self.field_specs, &stamp_opts).map_err(map_pdf_err)?;

        Ok(RenderResult::new(
            vec![Artifact {
                bytes: stamped,
                output_format: OutputFormat::Pdf,
            }],
            OutputFormat::Pdf,
        ))
    }

    fn page_count(&self) -> usize {
        self.page_boxes.len()
    }

    /// Page dimensions in PDF points derived from the background's `/MediaBox`.
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        let [x0, y0, x1, y1] = *self.page_boxes.get(page)?;
        Some((x1 - x0, y1 - y0))
    }

    /// Rasterise `page` of the pre-flattened PDF via hayro. Field values are
    /// baked into the flat PDF as content-stream operators, so they appear in
    /// the raster without any regions-compositing by the caller.
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

    /// Schema-field geometry from the resolved specs — keyed on the schema path,
    /// skipping unbound widgets. Computed from cached state, no rasterization.
    fn regions(&self) -> Vec<RenderedRegion> {
        regions_of(&self.field_specs)
    }

    /// Full re-resolve + re-flatten against new document data — this backend's
    /// compile is cheap, so `apply` recomputes rather than incrementally
    /// recompiling. Transactional: specs and flat PDF swap together only after
    /// both succeed. Dirty pages are those carrying a field whose resolved spec
    /// changed; the background never changes, so field deltas are the only
    /// visible delta.
    fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        let field_specs = resolve_field_specs(&self.spec, &self.page_boxes, json_data)?;
        let flat_pdf = flatten_to_pdf(self.base_pdf.clone(), &field_specs).map_err(map_pdf_err)?;

        let mut dirty_pages: Vec<usize> = self
            .field_specs
            .iter()
            .zip(&field_specs)
            .filter(|(old, new)| old != new)
            .map(|(_, new)| new.page)
            .collect();
        dirty_pages.sort_unstable();
        dirty_pages.dedup();

        self.field_specs = field_specs;
        self.flat_pdf = flat_pdf;

        Ok(ChangeSet {
            page_count: self.page_boxes.len(),
            dirty_pages,
        })
    }
}

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

        Ok(RenderResult::new(artifacts, OutputFormat::Svg))
    }

    /// Render all pages as PNG via hayro, using the pre-flattened PDF. `scale`
    /// is device pixels per PDF point (`ppi / 72`), so the values baked into
    /// the flat PDF rasterise at the requested resolution.
    fn render_png(&self, scale: f32) -> Result<RenderResult, RenderError> {
        use hayro::vello_cpu::color::palette::css::WHITE;

        let pdf = HayroPdf::new(self.flat_pdf.clone()).map_err(|_| {
            engine_err(
                "pdfform::png_parse_failed",
                "failed to parse pre-flattened PDF for PNG render",
            )
        })?;
        let interp = standard_font_settings();
        let render_settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        };

        let mut artifacts = Vec::with_capacity(pdf.pages().len());
        for page in pdf.pages().iter() {
            let cache = RenderCache::new();
            let pixmap = hayro_render(page, &cache, &interp, &render_settings);
            let png = pixmap.into_png().map_err(|e| {
                engine_err(
                    "pdfform::png_encoding",
                    format!("failed to encode page as PNG: {e}"),
                )
            })?;
            artifacts.push(Artifact {
                bytes: png,
                output_format: OutputFormat::Png,
            });
        }

        Ok(RenderResult::new(artifacts, OutputFormat::Png))
    }
}

/// Build an `InterpreterSettings` that satisfies standard Type1 font queries
/// (Helvetica, ZapfDingbats, etc.) using hayro's embedded font data.
/// Required for rendering the flat PDF's Helv and ZaDb content streams.
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
    RenderError::from_diag(Diagnostic::new(Severity::Error, e.message).with_code(e.code.to_string()))
}

/// A single-diagnostic `RenderError` with `code`.
fn engine_err(code: &str, message: impl Into<String>) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string()),
    )
}
