//! Visual preview harness for the `pdfform` backend.
//!
//! Renders `sample_form` → `sample_form_filled.pdf` and `taro` → `taro_preview.svg`,
//! writes both to the fixtures output directory, and prints the regions sidecar
//! for the filled form so field geometry can be cross-checked against a viewer.
//!
//! Run with:
//!   cargo run --example pdfform_preview -p quillmark

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::{example_output_dir, quills_path, write_example_output};

const SAMPLE_FORM_MD: &str = "\
~~~
$quill: sample_form
$kind: main
full_name: Ada Lovelace
comments:
  - First comment line.
  - Second comment line.
agree: true
favorite_color: green
~~~
";

const TARO_MD: &str = "\
~~~
$quill: taro
$kind: main
author: Ada Lovelace
title: Taro Preview
ice_cream: taro
~~~

This is a preview document rendered for visual review of the SVG backend output.
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = Quillmark::new();
    let out_dir = example_output_dir();

    // --- pdfform: sample_form → PDF ---
    println!("=== pdfform backend: sample_form → PDF ===");
    let gf_quill =
        quillmark::quill_from_path(quills_path("sample_form")).expect("load sample_form quill");
    let gf_doc = Document::from_markdown(SAMPLE_FORM_MD).expect("parse sample_form markdown");
    let gf_result = engine
        .render(
            &gf_quill,
            &gf_doc,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        )
        .expect("sample_form render");

    write_example_output("sample_form_filled.pdf", &gf_result.artifacts[0].bytes)?;
    println!("Written: {}", out_dir.join("sample_form_filled.pdf").display());

    println!(
        "\nField regions sidecar ({} fields):",
        gf_result.regions.len()
    );
    for region in &gf_result.regions {
        // `RegionKind` is an enum so new kinds can be added later; use a
        // refutable `if let` (not an irrefutable `let`) so a future variant is
        // non-breaking at this site. The `allow` silences the
        // single-variant-today warning; it lapses on its own once a second
        // variant exists.
        #[allow(irrefutable_let_patterns)]
        if let quillmark_core::RegionKind::Field { field_type, value } = &region.kind {
            let val_display = value.as_deref().unwrap_or("<blank>");
            println!(
                "  {:20}  page={:<2}  rect=[{:.1},{:.1},{:.1},{:.1}]  type={:<10}  value={:?}",
                region.name,
                region.page,
                region.rect[0],
                region.rect[1],
                region.rect[2],
                region.rect[3],
                field_type,
                val_display,
            );
        }
    }

    // --- Typst backend: taro → SVG ---
    println!("\n=== Typst backend: taro → SVG ===");
    let taro_quill = quillmark::quill_from_path(quills_path("taro")).expect("load taro quill");
    let taro_doc = Document::from_markdown(TARO_MD).expect("parse taro markdown");
    let taro_result = engine
        .render(
            &taro_quill,
            &taro_doc,
            &RenderOptions {
                output_format: Some(OutputFormat::Svg),
                ..Default::default()
            },
        )
        .expect("taro SVG render");

    write_example_output("taro_preview.svg", &taro_result.artifacts[0].bytes)?;
    println!("Written: {}", out_dir.join("taro_preview.svg").display());
    println!("SVG size: {} bytes", taro_result.artifacts[0].bytes.len());

    println!("\nDone. Open the files in the output directory to review:");
    println!("  PDF: {}", out_dir.join("sample_form_filled.pdf").display());
    println!("  SVG: {}", out_dir.join("taro_preview.svg").display());

    Ok(())
}
