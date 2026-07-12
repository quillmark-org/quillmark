//! The pdfform backend exports PDF, SVG, and PNG.
//!
//! PDF is the interactive AcroForm deliverable (stamped). SVG and PNG are
//! raster/vector views of the *flattened* form — field values baked into the
//! page content via hayro — so they render in any viewer without appearance
//! synthesis. This test renders the `sample_form` fixture to all three and
//! asserts each artifact is well-formed for its format.

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

const FILLED: &str = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Ada Lovelace\n\
comments:\n\
  - First comment line.\n\
agree: true\n\
favorite_color: green\n\
~~~\n";

fn render(format: OutputFormat, ppi: Option<f32>) -> Vec<quillmark_core::Artifact> {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::from_markdown(FILLED).expect("parse markdown");
    engine
        .render(
            &quill,
            &doc,
            &RenderOptions {
                output_format: Some(format),
                ppi,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("render {format:?}: {e:?}"))
        .artifacts
}

#[test]
fn renders_svg_per_page() {
    let artifacts = render(OutputFormat::Svg, None);
    assert!(!artifacts.is_empty(), "at least one SVG page");
    for art in &artifacts {
        assert_eq!(art.output_format, OutputFormat::Svg);
        let text = std::str::from_utf8(&art.bytes).expect("SVG is UTF-8");
        assert!(text.contains("<svg"), "artifact must be an SVG document");
    }
}

#[test]
fn renders_png_per_page() {
    let artifacts = render(OutputFormat::Png, Some(96.0));
    assert!(!artifacts.is_empty(), "at least one PNG page");
    for art in &artifacts {
        assert_eq!(art.output_format, OutputFormat::Png);
        // PNG 8-byte signature.
        assert!(
            art.bytes
                .starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "artifact must carry the PNG signature"
        );
    }
}

#[test]
fn png_ppi_controls_raster_size() {
    // Higher ppi → more bytes (larger raster), confirming ppi reaches hayro.
    let small = render(OutputFormat::Png, Some(72.0));
    let large = render(OutputFormat::Png, Some(216.0));
    assert!(
        large[0].bytes.len() > small[0].bytes.len(),
        "216 ppi PNG ({} B) should exceed 72 ppi PNG ({} B)",
        large[0].bytes.len(),
        small[0].bytes.len(),
    );
}
