use quillmark_core::{normalize::normalize_document, quill::QuillConfig, Document};

#[test]
fn test_markdown_type_is_a_load_error() {
    // `type: markdown` was a deprecated alias for block `richtext`; PR-G retires
    // it outright. A Quill.yaml that still declares it fails to load — no silent
    // alias, no parallel accepted spelling.
    let err = QuillConfig::from_yaml(
        r#"
quill:
  name: markdown_schema
  version: "1.0"
  backend: typst
  description: markdown schema test

main:
  fields:
    description:
      type: markdown
"#,
    )
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("markdown"),
        "load error should name the offending type: {msg}"
    );
}

#[test]
fn test_richtext_field_schema_emission() {
    let config = QuillConfig::from_yaml(
        r#"
quill:
  name: richtext_schema
  version: "1.0"
  backend: typst
  description: richtext schema test

main:
  fields:
    description:
      type: richtext
"#,
    )
    .unwrap();

    let yaml = config.schema_yaml().unwrap();
    let value: serde_json::Value = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        value
            .get("main")
            .and_then(|v| v.get("fields"))
            .and_then(|v| v.get("description"))
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("richtext")
    );
}

#[test]
fn test_markdown_field_normalization() {
    // Create a document via from_markdown
    let md = "~~~card-yaml\n$quill: test\n$kind: main\nmarkdown_field: This has <<guillemets>>\nstring_field: This has <<stripped>>\n~~~\n";
    let doc = Document::from_markdown(md).unwrap();

    // Normalize
    let normalized = normalize_document(doc).expect("Failed to normalize document");
    let fm = normalized.main().payload();

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
