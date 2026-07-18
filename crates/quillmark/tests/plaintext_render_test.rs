//! End-to-end proof that a `plaintext` field lowers through the typst backend
//! with **zero backend edits**: it rides the same `contentMediaType` as
//! richtext, so `content_field_names` classifies it as content and the shared
//! content-lowering path emits it. The literal codec means markdown delimiters
//! stay verbatim rather than becoming markup.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use std::fs;
use tempfile::TempDir;

fn plaintext_quill(temp_dir: &TempDir) -> std::path::PathBuf {
    let quill_path = temp_dir.path().join("plain_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        r#"quill:
  name: "plain_quill"
  version: "1.0"
  backend: "typst"
  description: "plaintext lowering"

main:
  body:
    enabled: false
  fields:
    subject:
      type: plaintext
      default: ""
"#,
    )
    .unwrap();
    // The backend pre-lowers a content field into the `data` dict, so the plate
    // places the lowered content by referencing it directly.
    fs::write(
        quill_path.join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data\n= Doc\n#data.subject\n",
    )
    .unwrap();
    quill_path
}

#[test]
fn plaintext_field_lowers_through_typst_backend() {
    let temp_dir = TempDir::new().unwrap();
    let quill_path = plaintext_quill(&temp_dir);

    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quill_path).expect("load quill");
    // Markdown delimiters in the value must be treated as literal characters by
    // the plaintext codec, and the content must lower cleanly to typst content.
    let md = "~~~card-yaml\n$quill: plain_quill\n$kind: main\n\
              subject: \"a *literal* subject with _no_ markup\"\n~~~\n";
    let parsed = Document::parse(md).expect("parse").document;

    let result = engine.render(
        &quill,
        &parsed,
        &RenderOptions {
            output_format: Some(OutputFormat::Svg),
            ..Default::default()
        },
    );
    assert!(
        result.is_ok(),
        "plaintext field should lower and render through the typst backend, got: {:?}",
        result.err()
    );
}
