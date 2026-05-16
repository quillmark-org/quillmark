use quillmark_core::{normalize::normalize_document, quill::QuillConfig, Document};

#[test]
fn test_markdown_field_schema_emission() {
    let config = QuillConfig::from_yaml(
        r#"
quill:
  name: markdown_schema
  version: "1.0"
  backend: typst
  description: markdown schema test

cards:
  main:
    fields:
      description:
        type: markdown
"#,
    )
    .unwrap();

    let yaml = config.schema_yaml().unwrap();
    let value: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        value
            .get("cards")
            .and_then(|v| v.get("main"))
            .and_then(|v| v.get("fields"))
            .and_then(|v| v.get("description"))
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("markdown")
    );
}

#[test]
fn test_markdown_field_normalization() {
    // Create a document via from_markdown
    let md = "---\nQUILL: test\nmarkdown_field: This has <<guillemets>>\nstring_field: This has <<stripped>>\n---\n";
    let doc = Document::from_markdown(md).unwrap();

    // Normalize
    let normalized = normalize_document(doc).expect("Failed to normalize document");
    let fm = normalized.main().frontmatter();

    // Both fields pass through unchanged (no stripping on YAML fields)
    assert_eq!(
        fm.get("markdown_field").unwrap().as_str().unwrap(),
        "This has <<guillemets>>"
    );
    assert_eq!(
        fm.get("string_field").unwrap().as_str().unwrap(),
        "This has <<stripped>>"
    );
}

#[test]
fn test_normalize_document_body_is_str_not_option() {
    // body() now returns &str (not Option<&str>)
    let doc = Document::from_markdown("---\nQUILL: t\n---\n\nHello body.").unwrap();
    let normalized = normalize_document(doc).unwrap();
    assert!(normalized.main().body().contains("Hello body."));
}
