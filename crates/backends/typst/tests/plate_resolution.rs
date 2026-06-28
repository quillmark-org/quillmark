//! The Typst backend resolves its own plate from the `typst.plate_file`
//! setting in the quill's config, reading the file from the quill's bundle.
//! Core no longer reads any template at load time, so a missing plate is a
//! render-time (`open`) error rather than a load-time one.

use std::collections::HashMap;

use quillmark_core::{Backend, FileTreeNode, Quill};
use quillmark_typst::TypstBackend;

fn quill(yaml: &str, files: &[(&str, &[u8])]) -> Quill {
    let mut map = HashMap::new();
    map.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    for (name, bytes) in files {
        map.insert(
            (*name).to_string(),
            FileTreeNode::File {
                contents: bytes.to_vec(),
            },
        );
    }
    Quill::from_tree(FileTreeNode::Directory { files: map }).expect("load quill")
}

const YAML: &str = "quill:\n  name: t\n  version: \"1.0\"\n  backend: typst\n  \
                    description: d\n\ntypst:\n  plate_file: plate.typ\n";

#[test]
fn plate_file_is_resolved_from_the_typst_section() {
    let q = quill(
        YAML,
        &[("plate.typ", b"#set page(width: 100pt, height: 100pt)\n= Hi\n")],
    );
    let session = TypstBackend
        .open(&q, &serde_json::json!({}))
        .expect("open should resolve typst.plate_file and compile");
    assert!(session.page_count() >= 1);
}

#[test]
fn missing_plate_file_errors_at_open_not_load() {
    // Loads fine: core reads no backend template at load time.
    let q = quill(YAML, &[]);
    // Opening resolves the plate and fails because the declared file is absent.
    let err = match TypstBackend.open(&q, &serde_json::json!({})) {
        Ok(_) => panic!("a missing plate file must fail at open"),
        Err(e) => e,
    };
    let diags = err.into_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::plate_missing")),
        "expected a typst::plate_missing diagnostic, got {:?}",
        diags.iter().map(|d| d.code.as_deref()).collect::<Vec<_>>()
    );
}
