//! # Dry Run Validation Tests

use quillmark::{Document, Quillmark};
use std::fs;
use tempfile::TempDir;

fn make_test_quill_path(temp_dir: &TempDir, with_required_field: bool) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();

    let fields_section = if with_required_field {
        "cards:\n  main:\n    fields:\n      title:\n        type: \"string\"\n        required: true\n      author:\n        type: \"string\"\n        required: false\n"
    } else {
        ""
    };

    fs::write(
        quill_path.join("Quill.yaml"),
        format!(
            "quill:\n  name: \"test_quill\"\n  version: \"1.0\"\n  backend: \"typst\"\n  main_file: \"main.typ\"\n  description: \"Test\"\n\n{}",
            fields_section
        ),
    ).unwrap();
    fs::write(quill_path.join("main.typ"), "Title: {{ title }}").unwrap();
    quill_path
}

#[test]
fn test_dry_run_success() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, true);

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: test_quill\ntitle: My Document\nauthor: Test\n---\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(result.is_ok(), "dry_run should succeed: {:?}", result);
}

#[test]
fn test_dry_run_missing_required_field() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, true);

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: test_quill\nauthor: Test\n---\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_err(),
        "dry_run should fail for missing required field"
    );

    let err = result.unwrap_err();
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("ValidationFailed") || err_str.contains("title"),
        "Error should indicate validation failure: {}",
        err_str
    );
}

#[test]
fn test_dry_run_no_schema() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, false);

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: test_quill\nrandom_field: anything\n---\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(result.is_ok(), "dry_run should succeed without schema");
}
