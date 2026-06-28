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
fn test_quill_engine_creation() {
    let engine = Quillmark::new();
    let backends = engine.registered_backends();
    #[cfg(feature = "typst")]
    assert!(!backends.is_empty());
    let _ = backends;
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
    match engine.supported_formats(&quill) {
        Err(quillmark::RenderError::UnsupportedBackend { .. }) => {}
        other => panic!("Expected UnsupportedBackend, got: {:?}", other),
    }
}

#[test]
#[cfg(feature = "typst")]
fn test_quill_engine_end_to_end() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        "quill:\n  name: \"my_test_quill\"\n  version: \"1.0\"\n  backend: \"typst\"\n  description: \"Test\"\n\ntypst:\n  plate_file: plate.typ\n",
    ).unwrap();
    fs::write(
        quill_path.join("plate.typ"),
        "= {{ title | String(default=\"Test\") }}\n\n{{ body | Content }}",
    )
    .unwrap();

    let quill = quillmark::quill_from_path(&quill_path).expect("Quill::from_path failed");

    let markdown =
        "~~~card-yaml\n$quill: my_test_quill\n$kind: main\ntitle: Test Document\n~~~\n\n# Introduction\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(result.is_ok(), "dry_run failed: {:?}", result);
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

    if let Err(quillmark::RenderError::EngineCreation { diags }) = &result {
        if diags[0].message.contains("No fonts found") {
            return;
        }
    }
    assert!(
        result.is_ok(),
        "render should succeed for engine-loaded quill"
    );
}
