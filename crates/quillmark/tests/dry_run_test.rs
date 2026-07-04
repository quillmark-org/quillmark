//! # Dry Run Validation Tests

use quillmark::Document;
use std::fs;
use tempfile::TempDir;

fn make_test_quill_path(temp_dir: &TempDir, with_required_field: bool) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();

    // `title` has no default → Unendorsed. `author` has a default →
    // Endorsed (absence is OK).
    let fields_section = if with_required_field {
        "main:\n  fields:\n    title:\n      type: \"string\"\n    author:\n      type: \"string\"\n      default: \"\"\n"
    } else {
        ""
    };

    fs::write(
        quill_path.join("Quill.yaml"),
        format!(
            "quill:\n  name: \"test_quill\"\n  version: \"1.0\"\n  backend: \"typst\"\n  description: \"Test\"\n\ntypst:\n  plate_file: plate.typ\n\n{}",
            fields_section
        ),
    ).unwrap();
    fs::write(quill_path.join("plate.typ"), "Title: {{ title }}").unwrap();
    quill_path
}

#[test]
fn test_dry_run_tolerates_must_fill_marker() {
    // A `!must_fill` placeholder never gates render: the marked field zero-fills
    // (null ≡ absent) and dry_run succeeds. The marker surfaces as a non-fatal
    // warning from `validate`, not as a render error.
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, true);

    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: !must_fill\nauthor: Test\n~~~\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_ok(),
        "dry_run should tolerate a !must_fill placeholder (zero-filled): {:?}",
        result
    );
}

#[test]
fn test_dry_run_no_schema() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, false);

    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nrandom_field: anything\n~~~\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(result.is_ok(), "dry_run should succeed without schema");
}
