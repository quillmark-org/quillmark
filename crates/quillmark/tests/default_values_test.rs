//! # Default Values Tests

use quillmark::{Document, Quillmark};
use std::fs;
use tempfile::TempDir;

fn create_test_quill(temp_dir: &TempDir, quill_yaml: &str) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(quill_path.join("Quill.yaml"), quill_yaml).unwrap();
    fs::write(
        quill_path.join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data\n= Document\n#data",
    )
    .unwrap();
    quill_path
}

#[test]
fn test_default_values_applied_via_dry_run() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  plate_file: "plate.typ"
  description: "Test quill with defaults"

main:
  fields:
    title:
      type: "string"
    status:
      type: "string"
      default: "draft"
    version:
      type: "number"
      default: 1
"#,
    );

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: test_quill\ntitle: My Document\n---\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_ok(),
        "Dry run should succeed - optional fields have defaults"
    );
}

#[test]
fn test_default_values_not_overriding_existing_fields() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  plate_file: "plate.typ"
  description: "Test quill with defaults"

main:
  fields:
    title:
      type: "string"
    status:
      type: "string"
      default: "draft"
"#,
    );

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown =
        "---\nQUILL: test_quill\ntitle: My Document\nstatus: published\n---\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_ok(),
        "Dry run should succeed with explicit values"
    );
}

#[test]
fn test_validation_with_defaults() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  plate_file: "plate.typ"
  description: "Test quill with optional fields"

main:
  fields:
    title:
      type: "string"
      default: "Untitled"
    status:
      type: "string"
      default: "draft"
"#,
    );

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: test_quill\n---\n\n# Content";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let dry_run_result = quill.dry_run(&parsed);
    assert!(
        dry_run_result.is_ok(),
        "Dry run should pass - fields have defaults"
    );
}

#[test]
fn test_validation_fails_without_defaults() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  plate_file: "plate.typ"
  description: "Test quill with required field"

main:
  fields:
    title:
      type: "string"
      required: true
    status:
      type: "string"
      default: "draft"
"#,
    );

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown = "---\nQUILL: test_quill\nstatus: published\n---\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_err(),
        "Should fail validation - title is required"
    );

    let err = result.unwrap_err();
    let mentions_title = err
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("title") || d.path.as_deref() == Some("title"));
    assert!(
        mentions_title,
        "Validation diagnostics should mention missing 'title' field; got {:?}",
        err.diagnostics()
    );
}

#[test]
fn test_extract_defaults_from_quill() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  description: "Test"

main:
  fields:
    author:
      type: "string"
      default: "Anonymous"
    priority:
      type: "number"
      default: 5
    draft:
      type: "boolean"
      default: true
"#,
    )
    .unwrap();

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(quill_path)
        .expect("quill_from_path failed");
    let defaults = quill.source().config().main.defaults();

    assert!(defaults.contains_key("author"));
    assert_eq!(defaults.get("author").unwrap().as_str(), Some("Anonymous"));
    assert!(defaults.contains_key("priority"));
    assert_eq!(defaults.get("priority").unwrap().as_i64(), Some(5));
    assert!(defaults.contains_key("draft"));
    assert_eq!(defaults.get("draft").unwrap().as_bool(), Some(true));
}
