//! Tests for [`quillmark::Quill::validate`] — the editor-facing validation
//! surface.

use std::collections::HashMap;

use quillmark::{Document, Quill};
use quillmark_core::quill::FileTreeNode;

/// Build a minimal quill from inline `Quill.yaml` with no filesystem deps.
fn quill_from_yaml(yaml: &str) -> quillmark::Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    let root = FileTreeNode::Directory { files };
    Quill::from_tree(root).expect("quill_from_yaml: from_tree failed")
}

const SIMPLE: &str = r#"
quill:
  name: validate_test
  version: "1.0"
  backend: typst
  description: Validate surface test

main:
  fields:
    title:
      type: string
    status:
      type: string
      default: draft
    count:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: string
"#;

#[test]
fn validate_clean_document_has_no_diagnostics() {
    let quill = quill_from_yaml(SIMPLE);
    // All Unendorsed fields supplied; `status` falls back to its default.
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: \"T\"\ncount: 1\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    assert!(
        quill.validate(&doc).is_empty(),
        "a complete, well-formed document should produce no diagnostics"
    );
}

#[test]
fn validate_forwards_type_mismatch_with_path_and_hint() {
    let quill = quill_from_yaml(SIMPLE);
    // `count` is a string, not an integer.
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: \"T\"\ncount: \"not-a-number\"\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    let diags = quill.validate(&doc);
    let diag = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::type_mismatch"))
        .expect("expected a type_mismatch diagnostic");
    assert_eq!(diag.path.as_deref(), Some("count"));
    assert!(diag.hint.is_some(), "type_mismatch should carry a hint");
}

#[test]
fn validate_reports_unknown_card_kind() {
    let quill = quill_from_yaml(SIMPLE);
    // A card whose `$kind` is not declared in the schema. (The form view used
    // to drop the card and emit `form::unknown_card_kind`; validation already
    // reports the identical condition under `validation::unknown_card`.)
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\ntitle: \"T\"\ncount: 1\n~~~\n\n\
              ~~~card-yaml\n$kind: ghost\nbody: \"B\"\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    let diags = quill.validate(&doc);
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("validation::unknown_card")),
        "expected validation::unknown_card; got: {:?}",
        diags.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn validate_includes_field_absent_completeness_signal() {
    let quill = quill_from_yaml(SIMPLE);
    // `title` and `count` are Unendorsed (no default) and absent. Unlike render
    // (which demotes this to a non-fatal zero-fill), `validate` surfaces it as
    // the per-field completeness hint.
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    let diags = quill.validate(&doc);
    let absent: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("validation::field_absent"))
        .filter_map(|d| d.path.clone())
        .collect();
    assert!(
        absent.contains(&"title".to_string()) && absent.contains(&"count".to_string()),
        "field_absent should flag both absent Unendorsed fields; got paths: {absent:?}"
    );
}

#[test]
fn validate_flags_surviving_must_fill_sentinel() {
    let quill = quill_from_yaml(SIMPLE);
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: <must-fill>\ncount: 1\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    let diags = quill.validate(&doc);
    let diag = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::must_fill_sentinel"))
        .expect("expected must_fill_sentinel diagnostic");
    assert_eq!(diag.path.as_deref(), Some("title"));
}
