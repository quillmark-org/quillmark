//! Compiles a plate plus injected JSON data into a Typst paged document, then
//! renders selected pages to output bytes (PDF, SVG, or PNG).
//!
//! Crate-internal entry points: [`compile_document`] (world → paged document
//! plus compile warnings) and [`render_document_pages`] (paged document →
//! artifacts). The public surface is the [`crate::TypstBackend`] `Backend`
//! implementation.

use typst::diag::Warned;
use typst::utils::Scalar;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;
use typst_svg::SvgOptions;

use crate::error_mapping::map_typst_errors;
use crate::overlay;
use crate::world::QuillWorld;
use quillmark_core::{Artifact, Diagnostic, OutputFormat, RenderError, RenderResult, Severity};
use quillmark_pdf::{stamp, PdfError, StampOptions};

/// Map a stamp-spine [`PdfError`] to the backend's `RenderError`. The spine owns
/// its own error type; this is the boundary translation.
fn map_pdf_err(e: PdfError) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, e.message).with_code(e.code.to_string()),
    )
}

/// Build raster render options for a given pixels-per-point scale factor.
fn render_options(pixel_per_pt: f32) -> RenderOptions {
    RenderOptions {
        pixel_per_pt: Scalar::new(pixel_per_pt as f64),
        ..Default::default()
    }
}

/// `comemo` memo entries older than this many compiles are evicted after each
/// compile. The cache is process-global and grows unboundedly without
/// eviction — an editing loop (one compile per keystroke) leaks otherwise.
/// 10 matches typst-cli's watch-loop policy: deep enough to keep everything a
/// live document's recompile reuses, shallow enough to bound a long session.
///
/// Caveat: the "age" clock is also process-global, not per-`QuillWorld`. Two
/// sessions compiling interleaved in one process share it, so a document that
/// goes quiet for 10 compiles *summed across all sessions* has its entries
/// evicted even when its own edit history is shorter — reuse degrades under
/// concurrent-session use (WASM canvas-preview, a long-lived multi-session
/// Python/CLI process). Never a wrong render (comemo entries are pure functions
/// of input), only lost reuse. A true per-session bound needs an eviction
/// counter scoped to the `World`, which comemo doesn't expose today.
const COMEMO_EVICT_MAX_AGE: usize = 10;

/// Compile the world, returning the paged document together with Typst's
/// non-fatal compile warnings (font fallback, overfull pages, …) mapped into
/// [`Diagnostic`]s with resolved spans — the boundary carries everything
/// `typst::compile` hands back. On failure only the errors are returned; the
/// failed compile's warnings die with it (the session keeps its last-good
/// compile and that compile's warnings).
pub(crate) fn compile_document(
    world: &QuillWorld,
) -> Result<(PagedDocument, Vec<Diagnostic>), RenderError> {
    let Warned { output, warnings } = typst::compile::<PagedDocument>(world);
    comemo::evict(COMEMO_EVICT_MAX_AGE);

    match output {
        Ok(doc) => Ok((doc, map_typst_errors(&warnings, world))),
        Err(errors) => Err(RenderError::new(map_typst_errors(&errors, world))),
    }
}

/// Default pixels per inch for PNG rendering (2x at 72pt/inch).
const DEFAULT_PPI: f32 = 144.0;

/// Render selected pages from an already-compiled Typst document.
///
/// `field_placements` become spine `FieldSpec`s; only PDF stamps them as
/// AcroForm widgets, but every format carries the resulting field regions. Pass
/// an empty slice for documents with no `form-field` calls.
///
/// `producer` overrides the PDF `/Info` `/Producer` string (PDF output only);
/// `None` uses [`overlay::default_producer`] (`Quillmark <version>`).
pub(crate) fn render_document_pages(
    document: &PagedDocument,
    pages: Option<&[usize]>,
    format: OutputFormat,
    ppi: Option<f32>,
    field_placements: &[overlay::FieldPlacement],
    producer: Option<&str>,
) -> Result<RenderResult, RenderError> {
    // PDF does not support selective page rendering
    if format == OutputFormat::Pdf && pages.is_some() {
        return Err(RenderError::from_diag(
            Diagnostic::new(
                Severity::Error,
                "PDF does not support page selection; pass null/None to render the full document, or use PNG/SVG".to_string(),
            )
            .with_code("typst::pdf_page_selection_not_supported".to_string()),
        ));
    }

    let page_count = document.pages().len();
    let selected_indices: Vec<usize> = match pages {
        Some(slice) => {
            let out_of_bounds: Vec<usize> =
                slice.iter().copied().filter(|&i| i >= page_count).collect();
            if !out_of_bounds.is_empty() {
                return Err(RenderError::from_diag(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "Page index out of bounds (page_count={}); offending indices: {:?}. Check `LiveSession.pageCount` before requesting pages.",
                            page_count, out_of_bounds
                        ),
                    )
                    .with_code("typst::page_index_out_of_bounds".to_string()),
                ));
            }
            slice.to_vec()
        }
        None => (0..page_count).collect(),
    };

    match format {
        OutputFormat::Svg => {
            let artifacts = selected_indices
                .into_iter()
                .map(|idx| Artifact {
                    bytes: typst_svg::svg(&document.pages()[idx], &SvgOptions::default())
                        .into_bytes(),
                    output_format: OutputFormat::Svg,
                })
                .collect();
            Ok(RenderResult::new(artifacts, OutputFormat::Svg))
        }
        OutputFormat::Png => {
            let scale = ppi.unwrap_or(DEFAULT_PPI) / 72.0;
            let opts = render_options(scale);
            let mut artifacts = Vec::with_capacity(selected_indices.len());
            for idx in selected_indices {
                let pixmap = typst_render::render(&document.pages()[idx], &opts);
                let png_data = pixmap.encode_png().map_err(|e| {
                    RenderError::from_diag(
                        Diagnostic::new(Severity::Error, format!("PNG encoding failed: {}", e))
                            .with_code("typst::png_encoding".to_string()),
                    )
                })?;
                artifacts.push(Artifact {
                    bytes: png_data,
                    output_format: OutputFormat::Png,
                });
            }
            Ok(RenderResult::new(artifacts, OutputFormat::Png))
        }
        OutputFormat::Pdf => {
            let pdf = typst_pdf::pdf(document, &PdfOptions::default()).map_err(|e| {
                RenderError::from_diag(
                    Diagnostic::new(Severity::Error, format!("PDF generation failed: {:?}", e))
                        .with_code("typst::pdf_generation".to_string()),
                )
            })?;
            // Form-field placements → spine field specs (Typst top-left → PDF
            // bottom-left), stamped as AcroForm widgets. Only the PDF path needs
            // them; SVG/PNG render the pages directly, and field geometry is a
            // session-level query (`TypstSession::regions`), not a render output.
            let field_specs = overlay::build_field_specs(document, field_placements)?;
            // The producer is always stamped (the always-on `/Info` pass); the
            // override threads from the product layer, else the backend default.
            let producer = Some(
                producer
                    .map(str::to_string)
                    .unwrap_or_else(overlay::default_producer),
            );
            let stamped =
                stamp(pdf, &field_specs, &StampOptions { producer }).map_err(map_pdf_err)?;
            Ok(RenderResult::new(
                vec![Artifact {
                    bytes: stamped,
                    output_format: OutputFormat::Pdf,
                }],
                OutputFormat::Pdf,
            ))
        }
        OutputFormat::Txt => Err(RenderError::from_diag(
            Diagnostic::new(
                Severity::Error,
                "TXT output is not supported for Typst".into(),
            )
            .with_code("typst::format_not_supported".to_string()),
        )),
    }
}
