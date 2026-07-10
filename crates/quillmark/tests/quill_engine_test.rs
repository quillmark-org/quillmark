//! Integration tests for the Quillmark engine.

use std::fs;
use tempfile::TempDir;

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

fn make_quill_dir(temp_dir: &TempDir, name: &str, backend: &str) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join(name);
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        format!(
            "quill:\n  name: \"{}\"\n  version: \"1.0\"\n  backend: \"{}\"\n  description: \"Test\"\n\n{}:\n  plate_file: plate.typ\n",
            name, backend, backend
        ),
    )
    .unwrap();
    fs::write(quill_path.join("plate.typ"), "#rect(width: 1cm)").unwrap();
    quill_path
}

#[test]
#[cfg(feature = "typst")]
fn test_quill_from_path_engine_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill_dir(&temp_dir, "my_test_quill", "typst");

    let quill = quillmark::quill_from_path(quill_path).expect("Quill::from_path failed");

    assert_eq!(quill.name(), "my_test_quill");
    assert_eq!(quill.backend_id(), "typst");

    let engine = Quillmark::new();
    assert!(engine
        .supported_formats(&quill)
        .expect("supported_formats failed")
        .contains(&OutputFormat::Pdf));
}

#[test]
fn test_unsupported_backend_errors_at_render_time() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill_dir(&temp_dir, "bad_backend_quill", "non_existent");

    // Loading does not resolve a backend: it succeeds for an unknown backend
    // id, tagging the quill with the declared intent. The backend-existence
    // check happens at render time.
    let quill =
        quillmark::quill_from_path(quill_path).expect("load succeeds; backend resolved later");
    assert_eq!(quill.backend_id(), "non_existent");

    let engine = Quillmark::new();
    let err = engine
        .supported_formats(&quill)
        .expect_err("unregistered backend must not resolve");
    assert_eq!(
        err.diagnostics()[0].code.as_deref(),
        Some("engine::backend_not_found")
    );
}

#[test]
#[cfg(feature = "typst")]
fn test_quill_render_succeeds_with_engine_loaded_quill() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill_dir(&temp_dir, "my_quill", "typst");

    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quill_path).expect("Quill::from_path failed");
    let parsed = Document::from_markdown("~~~card-yaml\n$quill: my_quill\n$kind: main\n~~~\n")
        .expect("parse failed");
    let result = engine.render(
        &quill,
        &parsed,
        &RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        },
    );

    assert!(
        result.is_ok(),
        "render should succeed for engine-loaded quill"
    );
}
