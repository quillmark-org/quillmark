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
            "quill:\n  name: \"{}\"\n  version: \"1.0\"\n  backend: \"{}\"\n  main_file: \"main.typ\"\n  description: \"Test\"\n",
            name, backend
        ),
    )
    .unwrap();
    fs::write(quill_path.join("main.typ"), "#rect(width: 1cm)").unwrap();
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

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(quill_path)
        .expect("quill_from_path failed");

    assert_eq!(quill.name(), "my_test_quill");
    assert_eq!(quill.backend_id(), "typst");
    assert!(quill.supported_formats().contains(&OutputFormat::Pdf));
}

#[test]
fn test_quill_engine_backend_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_quill_dir(&temp_dir, "bad_backend_quill", "non_existent");

    let engine = Quillmark::new();
    let result = engine.quill_from_path(quill_path);

    assert!(result.is_err());
    match result {
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
        "quill:\n  name: \"my_test_quill\"\n  version: \"1.0\"\n  backend: \"typst\"\n  main_file: \"main.typ\"\n  description: \"Test\"\n",
    ).unwrap();
    fs::write(
        quill_path.join("main.typ"),
        "= {{ title | String(default=\"Test\") }}\n\n{{ body | Content }}",
    )
    .unwrap();

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: my_test_quill\ntitle: Test Document\n---\n\n# Introduction\n";
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
    let quill = engine
        .quill_from_path(quill_path)
        .expect("quill_from_path failed");
    let parsed = Document::from_markdown("---\nQUILL: my_quill\n---\n").expect("parse failed");
    let result = quill.render(
        &parsed,
        &RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        },
    );

    if let Err(quillmark::RenderError::EngineCreation { diag }) = &result {
        if diag.message.contains("No fonts found") {
            return;
        }
    }
    assert!(
        result.is_ok(),
        "render should succeed for engine-loaded quill"
    );
}
