//! # Dry Run Validation Tests

use quillmark::{Document, Quillmark};
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
            "quill:\n  name: \"test_quill\"\n  version: \"1.0\"\n  backend: \"typst\"\n  plate_file: \"plate.typ\"\n  description: \"Test\"\n\n{}",
            fields_section
        ),
    ).unwrap();
    fs::write(quill_path.join("plate.typ"), "Title: {{ title }}").unwrap();
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

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: My Document\nauthor: Test\n~~~\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(result.is_ok(), "dry_run should succeed: {:?}", result);
}

#[test]
fn test_dry_run_missing_must_fill_field_is_tolerated() {
    // Zero-filled render: a merely *incomplete* document (Unendorsed `title`
    // absent) is no longer a hard error — `title` is zero-filled in the plate
    // projection. Only a *malformed* document (a surviving `<must-fill>`
    // sentinel) still fails. See prose/canon/SCHEMAS.md.
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, true);

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nauthor: Test\n~~~\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_ok(),
        "dry_run should tolerate an absent Unendorsed field (zero-filled): {:?}",
        result
    );
}

#[test]
fn test_dry_run_surviving_sentinel_still_fails() {
    // A surviving `<must-fill>` sentinel is *malformed* — it always errors,
    // even though mere absence does not.
    let temp_dir = TempDir::new().unwrap();
    let quill_path = make_test_quill_path(&temp_dir, true);

    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: <must-fill>\nauthor: Test\n~~~\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(
        result.is_err(),
        "dry_run should reject a surviving <must-fill> sentinel"
    );
    let err_str = format!("{:?}", result.unwrap_err());
    assert!(
        err_str.contains("must_fill_sentinel") || err_str.contains("sentinel"),
        "error should be the sentinel diagnostic: {err_str}"
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

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nrandom_field: anything\n~~~\n\n# Content\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");

    let result = quill.dry_run(&parsed);
    assert!(result.is_ok(), "dry_run should succeed without schema");
}
