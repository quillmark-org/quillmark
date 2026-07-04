//! Shared test helpers for integration tests.

use quillmark_fixtures::{example_output_dir, quills_path, write_example_output};
use std::error::Error;

/// Load a quill, render its generated `example` document to PDF, and write the
/// result to the demo output directory.
///
/// Uses the `example` reference document (example › default › zero) so cells
/// carry illustrative values and the document renders without field-filling; the
/// plain `blueprint()` carries `!must_fill` placeholders (which render via the
/// zero-fill floor but warn until replaced).
pub fn demo(quill_dir: &str, render_output: &str) -> Result<(), Box<dyn Error>> {
    let quill_path = quills_path(quill_dir);
    let engine = quillmark::Quillmark::new();
    let quill = quillmark::quill_from_path(quill_path.clone()).expect("Failed to load quill");

    let parsed = quill.seed_document();

    let rendered = engine.render(
        &quill,
        &parsed,
        &quillmark_core::RenderOptions {
            output_format: Some(quillmark_core::OutputFormat::Pdf),
            ..Default::default()
        },
    )?;
    let output_bytes = rendered.artifacts[0].bytes.clone();

    write_example_output(render_output, &output_bytes)?;

    println!("------------------------------");
    println!(
        "Access render output: {}",
        example_output_dir().join(render_output).display()
    );

    Ok(())
}
