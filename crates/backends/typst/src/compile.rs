//! # Typst Compilation
//!
//! Compiles a plate plus injected JSON data into a Typst paged document, then
//! renders selected pages to output bytes (PDF, SVG, or PNG).
//!
//! ## Process
//!
//! 1. Create a `QuillWorld` with the quill's assets and packages.
//! 2. Compile the Typst document with the Typst compiler.
//! 3. Render the requested pages to the target format.
//!
//! Crate-internal entry points: [`compile_to_document`] (source → paged
//! document) and [`render_document_pages`] (paged document → artifacts). The
//! public surface is the [`crate::TypstBackend`] `Backend` implementation.

use typst::diag::Warned;
use typst::utils::Scalar;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;
use typst_svg::SvgOptions;

use crate::error_mapping::map_typst_errors;
use crate::overlay;
use crate::world::QuillWorld;
use quillmark_core::{
    Artifact, Diagnostic, OutputFormat, Quill, RenderError, RenderResult, Severity,
};

/// Build raster render options for a given pixels-per-point scale factor.
fn render_options(pixel_per_pt: f32) -> RenderOptions {
    RenderOptions {
        pixel_per_pt: Scalar::new(pixel_per_pt as f64),
        ..Default::default()
    }
}

fn compile_document(world: &QuillWorld) -> Result<PagedDocument, RenderError> {
    let Warned { output, warnings } = typst::compile::<PagedDocument>(world);

    for warning in warnings {
        eprintln!("Warning: {}", warning.message);
    }

    match output {
        Ok(doc) => Ok(doc),
        Err(errors) => {
            let diagnostics = map_typst_errors(&errors, world);
            Err(RenderError::CompilationFailed { diags: diagnostics })
        }
    }
}

/// Compile Typst source into a paged document with injected JSON data.
pub(crate) fn compile_to_document(
    source: &Quill,
    plated_content: &str,
    json_data: &str,
) -> Result<PagedDocument, RenderError> {
    let world = QuillWorld::new_with_data(source, plated_content, json_data).map_err(|e| {
        RenderError::EngineCreation {
            diags: vec![Diagnostic::new(
                Severity::Error,
                format!("Failed to create Typst compilation environment: {}", e),
            )
            .with_code("typst::world_creation".to_string())
            .with_source(e.as_ref())],
        }
    })?;

    compile_document(&world)
}

/// Default pixels per inch for PNG rendering (2x at 72pt/inch).
const DEFAULT_PPI: f32 = 144.0;

/// Render selected pages from an already-compiled Typst document.
///
/// `sig_placements` is consumed only when emitting PDF. Pass an empty slice
/// for SVG/PNG callers or documents with no `signature-field` calls.
///
/// `producer` overrides the PDF `/Info` `/Producer` string (PDF output only);
/// `None` uses [`overlay::default_producer`] (`Quillmark <version>`).
pub(crate) fn render_document_pages(
    document: &PagedDocument,
    pages: Option<&[usize]>,
    format: OutputFormat,
    ppi: Option<f32>,
    sig_placements: &[overlay::SigPlacement],
    producer: Option<&str>,
) -> Result<RenderResult, RenderError> {
    // PDF does not support selective page rendering
    if format == OutputFormat::Pdf && pages.is_some() {
        return Err(RenderError::FormatNotSupported {
            diags: vec![Diagnostic::new(
                Severity::Error,
                "PDF does not support page selection; pass null/None to render the full document, or use PNG/SVG".to_string(),
            )
            .with_code("typst::pdf_page_selection_not_supported".to_string())],
        });
    }

    let page_count = document.pages().len();
    let selected_indices: Vec<usize> = match pages {
        Some(slice) => {
            let out_of_bounds: Vec<usize> =
                slice.iter().copied().filter(|&i| i >= page_count).collect();
            if !out_of_bounds.is_empty() {
                return Err(RenderError::ValidationFailed {
                    diags: vec![Diagnostic::new(
                        Severity::Error,
                        format!(
                            "Page index out of bounds (page_count={}); offending indices: {:?}. Check `RenderSession.pageCount` before requesting pages.",
                            page_count, out_of_bounds
                        ),
                    )
                    .with_code("typst::page_index_out_of_bounds".to_string())],
                });
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
                let png_data = pixmap
                    .encode_png()
                    .map_err(|e| RenderError::CompilationFailed {
                        diags: vec![Diagnostic::new(
                            Severity::Error,
                            format!("PNG encoding failed: {}", e),
                        )
                        .with_code("typst::png_encoding".to_string())],
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
                RenderError::CompilationFailed {
                    diags: vec![Diagnostic::new(
                        Severity::Error,
                        format!("PDF generation failed: {:?}", e),
                    )
                    .with_code("typst::pdf_generation".to_string())],
                }
            })?;
            let default_producer = overlay::default_producer();
            let pdf = overlay::inject(
                pdf,
                document,
                sig_placements,
                producer.unwrap_or(&default_producer),
            )?;
            Ok(RenderResult::new(
                vec![Artifact {
                    bytes: pdf,
                    output_format: OutputFormat::Pdf,
                }],
                OutputFormat::Pdf,
            ))
        }
        OutputFormat::Txt => Err(RenderError::FormatNotSupported {
            diags: vec![Diagnostic::new(
                Severity::Error,
                "TXT output is not supported for Typst".into(),
            )
            .with_code("typst::format_not_supported".to_string())],
        }),
    }
}
