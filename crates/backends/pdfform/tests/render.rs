//! End-to-end acceptance test for the `pdfform` backend: load the hand-authored
//! `simple_form` quill, fill it through the public `Backend`/`RenderSession`
//! path, then reparse the PDF with lopdf and assert the reconstructed AcroForm
//! and bound values.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
use quillmark_pdfform::PdfformBackend;

fn fixture_quill() -> Quill {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p: PathBuf = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/simple_form/0.1.0");
    Quill::from_tree(walk(&dir).expect("walk fixture")).expect("load simple_form quill")
}

fn render(json: serde_json::Value) -> Vec<u8> {
    let quill = fixture_quill();
    let session = PdfformBackend
        .open("", &quill, &json)
        .expect("open pdfform session");
    let result = session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        })
        .expect("render ok");
    result.artifacts[0].bytes.clone()
}

fn acroform(doc: &lopdf::Document) -> &lopdf::Dictionary {
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    doc.get_object(af_ref).unwrap().as_dict().unwrap()
}

fn widget_by_name<'a>(doc: &'a lopdf::Document, name: &str) -> &'a lopdf::Dictionary {
    let fields = acroform(doc).get(b"Fields").unwrap().as_array().unwrap();
    for f in fields {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        if w.get(b"T").unwrap().as_str().unwrap() == name.as_bytes() {
            return w;
        }
    }
    panic!("no widget named {name}");
}

#[test]
fn fills_form_from_document_values() {
    let pdf = render(serde_json::json!({
        "full_name": "Ada Lovelace",
        "agree": true,
        "favorite_color": "blue",
    }));
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");

    // Four reconstructed fields hung off both the AcroForm and the page.
    let af = acroform(&doc);
    assert_eq!(af.get(b"Fields").unwrap().as_array().unwrap().len(), 4);
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    // The page's /Annots references the widgets (single page).
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(page.get(b"Annots").unwrap().as_array().unwrap().len(), 4);

    // Text value bound from the document.
    let name = widget_by_name(&doc, "FullName");
    assert_eq!(name.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(name.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert_eq!(name.get(b"MaxLen").unwrap().as_i64().unwrap(), 64);
    assert_eq!(name.get(b"TU").unwrap().as_str().unwrap(), b"Full legal name of the applicant");

    // Checkbox checked (agree == true) → on-state.
    let agree = widget_by_name(&doc, "Agree");
    assert_eq!(agree.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Yes");

    // Choice with the bound value and combo flag.
    let color = widget_by_name(&doc, "FavoriteColor");
    assert_eq!(color.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"blue");
    assert_eq!(color.get(b"Opt").unwrap().as_array().unwrap().len(), 3);
    assert_ne!(color.get(b"Ff").unwrap().as_i64().unwrap() & (1 << 17), 0);

    // Signature: structural, no value.
    let sig = widget_by_name(&doc, "Signature");
    assert_eq!(sig.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    assert!(sig.get(b"V").is_err());
}

#[test]
fn unchecked_checkbox_is_off() {
    let pdf = render(serde_json::json!({
        "full_name": "Grace Hopper",
        "agree": false,
        "favorite_color": "red",
    }));
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let agree = widget_by_name(&doc, "Agree");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Off");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Off");
}

#[test]
fn empty_values_still_reconstruct_fields() {
    // No document values: fields are still rebuilt (blank), so the form is fillable.
    let pdf = render(serde_json::json!({}));
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    assert_eq!(
        acroform(&doc).get(b"Fields").unwrap().as_array().unwrap().len(),
        4
    );
    // A text field with no bound value carries no /V.
    let name = widget_by_name(&doc, "FullName");
    assert!(name.get(b"V").is_err());
}

#[test]
fn reports_single_page() {
    let quill = fixture_quill();
    let session = PdfformBackend
        .open("", &quill, &serde_json::json!({}))
        .expect("open");
    assert_eq!(session.page_count(), 1);
}
