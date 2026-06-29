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
      label:
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
    // A card whose `$kind` is not declared in the schema emits
    // `validation::unknown_card`.
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
fn validate_does_not_surface_field_absence() {
    let quill = quill_from_yaml(SIMPLE);
    // `title` and `count` are Unendorsed (no default) and absent. Field absence
    // is not a well-formedness error — it zero-fills at render — so an
    // incomplete-but-well-formed document produces no diagnostics.
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    assert!(
        quill.validate(&doc).is_empty(),
        "absence is not surfaced; an incomplete document validates clean"
    );
}

#[test]
fn validate_warns_on_must_fill_marker() {
    let quill = quill_from_yaml(SIMPLE);
    // The `!must_fill` placeholder surfaces as a non-fatal warning, regardless
    // of whether it carries a suggested value — and across the main card and
    // every composable card (the contract is "root and nested, main and cards").
    let md = "~~~card-yaml\n$quill: validate_test\n$kind: main\n\
              title: !must_fill Draft\ncount: !must_fill\n~~~\n\n\
              ~~~card-yaml\n$kind: note\nlabel: !must_fill\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    let diags = quill.validate(&doc);
    let marked: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("validation::must_fill"))
        .inspect(|d| assert_eq!(d.severity, quillmark_core::Severity::Warning))
        .filter_map(|d| d.path.clone())
        .collect();
    assert!(
        marked.contains(&"title".to_string())
            && marked.contains(&"count".to_string())
            && marked.contains(&"cards.note[0].label".to_string()),
        "main-card and composable-card !must_fill markers should all warn; \
         got paths: {marked:?}"
    );
}
