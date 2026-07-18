//! # Default Values Tests

use quillmark::Document;
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
fn test_nested_null_zero_fills_in_plate() {
    // null ≡ absent at every level: a null typed-dict property and a null
    // array element must zero-fill in the plate projection, never leak a bare
    // null. (Regression for the nested-null leak.)
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  description: "Nested null zero-fill"

main:
  fields:
    addr:
      type: object
      properties:
        street: { type: string }
        city: { type: string }
    tags:
      type: array
      items: { type: string }
"#,
    );
    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");
    let md = "~~~card-yaml\n$quill: test_quill\n$kind: main\n\
              addr:\n  street: !must_fill\n  city: Pittsburgh\n\
              tags:\n  - alpha\n  - null\n  - gamma\n~~~\n\nbody\n";
    let parsed = Document::parse(md).expect("parse failed").document;
    let data = quill
        .compile_data(&parsed)
        .expect("compile_data should succeed");

    let addr = data
        .get("addr")
        .and_then(|v| v.as_object())
        .expect("addr object");
    assert_eq!(
        addr.get("street").and_then(|v| v.as_str()),
        Some(""),
        "null nested property must zero-fill, not leak null: {data}"
    );
    let tags = data
        .get("tags")
        .and_then(|v| v.as_array())
        .expect("tags array");
    assert!(
        !tags.iter().any(|v| v.is_null()),
        "null array element must not leak into the plate: {data}"
    );
}

#[test]
fn test_defaults_applied_when_absent() {
    // An absent Endorsed field (one with a `default:`) resolves to its default
    // in the plate projection — across string and number types — while an
    // authored value still wins over the default. dry_run tolerates the
    // partially-authored document (nothing gates on absence).
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  description: "Test quill with defaults"

main:
  fields:
    title:
      type: "string"
      default: "Untitled"
    status:
      type: "string"
      default: "draft"
    version:
      type: "number"
      default: 1
"#,
    );

    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");

    // `status` is authored; `title` and `version` fall back to their defaults.
    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nstatus: published\n~~~\n\n# Content\n";
    let parsed = Document::parse(markdown).expect("parse failed").document;

    assert!(
        quill.dry_run(&parsed).is_ok(),
        "dry_run should tolerate absent Endorsed fields (defaults apply)"
    );

    let data = quill
        .compile_data(&parsed)
        .expect("compile_data should succeed");
    assert_eq!(
        data.get("title").and_then(|v| v.as_str()),
        Some("Untitled"),
        "absent Endorsed `title` should resolve to its default: {data}"
    );
    assert_eq!(
        data.get("version").and_then(|v| v.as_f64()),
        Some(1.0),
        "absent Endorsed `version` should resolve to its numeric default: {data}"
    );
    assert_eq!(
        data.get("status").and_then(|v| v.as_str()),
        Some("published"),
        "authored value should win over the default: {data}"
    );
}

#[test]
fn test_absent_must_fill_is_zero_filled() {
    // Zero-filled render: an absent Unendorsed field (`title`) is tolerated and
    // filled with its type-empty zero value (`""`) in the plate projection —
    // never persisted. See prose/canon/SCHEMAS.md.
    let temp_dir = TempDir::new().unwrap();
    let quill_path = create_test_quill(
        &temp_dir,
        r#"quill:
  name: "test_quill"
  version: "1.0"
  backend: "typst"
  description: "Test quill with an Unendorsed field"

main:
  fields:
    title:
      type: "string"
    status:
      type: "string"
      default: "draft"
"#,
    );

    let quill = quillmark::quill_from_path(&quill_path).expect("from_path failed");

    let markdown =
        "~~~card-yaml\n$quill: test_quill\n$kind: main\nstatus: published\n~~~\n\n# Content\n";
    let parsed = Document::parse(markdown).expect("parse failed").document;

    // Render does not gate on absence.
    assert!(
        quill.dry_run(&parsed).is_ok(),
        "dry_run should tolerate an absent Unendorsed field (zero-filled)"
    );

    // The plate projection carries the zero value for the absent field.
    let data = quill
        .compile_data(&parsed)
        .expect("compile_data should succeed");
    assert_eq!(
        data.get("title").and_then(|v| v.as_str()),
        Some(""),
        "absent Unendorsed `title` should be zero-filled to \"\" in the projection: {data}"
    );
    assert_eq!(
        data.get("status").and_then(|v| v.as_str()),
        Some("published"),
        "authored value should win over default"
    );
}
