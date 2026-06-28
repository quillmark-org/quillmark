//! Acceptance tests for the `/Info` `/Producer` metadata stamp (`overlay`).
//!
//! Compiles plates to PDF, reparses with lopdf, and asserts the `/Producer`
//! string — the default `Quillmark <version>`, a caller override (including
//! escaping), preservation of Typst's `/Creator`, and correct composition
//! with the signature-field overlay in the same incremental update.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lopdf::Object;
use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
use quillmark_typst::TypstBackend;

fn host_tree() -> FileTreeNode {
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
    let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.2.0");
    walk(&quill_path).expect("walk fixture")
}

/// Build the host quill with its `plate.typ` replaced by `plate`; the fixture's
/// `typst.plate_file: plate.typ` makes the backend read this override.
fn source_with_plate(plate: &str) -> Quill {
    let mut tree = host_tree();
    if let FileTreeNode::Directory { files } = &mut tree {
        files.insert(
            "plate.typ".to_string(),
            FileTreeNode::File {
                contents: plate.as_bytes().to_vec(),
            },
        );
    }
    Quill::from_tree(tree).expect("load source")
}

const PLATE: &str = "#set page(width: 400pt, height: 300pt)\n= Hello\n";

/// Render a plate to PDF bytes via the public `Backend`/`RenderSession` path.
fn render_pdf(plate: &str) -> Vec<u8> {
    let source = source_with_plate(plate);
    let session = TypstBackend
        .open(&source, &serde_json::json!({}))
        .expect("open session");
    let result = session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        })
        .expect("render ok");
    result.artifacts[0].bytes.clone()
}

/// Extract the decoded `/Info` `/Producer` string from a PDF.
fn producer_of(pdf: &[u8]) -> Vec<u8> {
    info_string(pdf, b"Producer")
}

fn info_string(pdf: &[u8], key: &[u8]) -> Vec<u8> {
    let doc = lopdf::Document::load_mem(pdf).expect("reparse pdf");
    let info_ref = doc
        .trailer
        .get(b"Info")
        .expect("/Info in trailer")
        .as_reference()
        .expect("/Info is a reference");
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    match info.get(key).expect("key present in /Info") {
        Object::String(bytes, _) => bytes.clone(),
        other => panic!("{key:?} not a string: {other:?}"),
    }
}

#[test]
fn default_producer_is_quillmark_version() {
    let pdf = render_pdf(PLATE);
    let expected = format!("Quillmark {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(producer_of(&pdf), expected.as_bytes());
}

#[test]
fn default_pass_preserves_typst_creator() {
    let pdf = render_pdf(PLATE);
    let creator = info_string(&pdf, b"Creator");
    assert!(
        creator.starts_with(b"Typst"),
        "expected Typst /Creator, got {:?}",
        String::from_utf8_lossy(&creator)
    );
}

#[test]
fn producer_override_via_render_options() {
    let source = source_with_plate(PLATE);
    let backend = TypstBackend;
    let session = backend
        .open(&source, &serde_json::json!({}))
        .expect("open session");
    // Includes (), and \\ to exercise PDF literal-string escaping.
    let override_str = r"ACME (PDF) \ Tool 2.0";
    let result = session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            producer: Some(override_str.to_string()),
            ..Default::default()
        })
        .expect("render ok");
    let pdf = &result.artifacts[0].bytes;
    assert_eq!(producer_of(pdf), override_str.as_bytes());
}

#[test]
fn producer_override_non_ascii_roundtrips() {
    // A non-ASCII override takes the UTF-16BE+BOM hex-string branch of
    // pdf_text_string; assert it decodes back to the original.
    let source = source_with_plate(PLATE);
    let backend = TypstBackend;
    let session = backend
        .open(&source, &serde_json::json!({}))
        .expect("open session");
    let override_str = "Quillmark 日本語 ✒";
    let result = session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            producer: Some(override_str.to_string()),
            ..Default::default()
        })
        .expect("render ok");
    // lopdf hands back the decoded hex bytes: a UTF-16BE BOM then the code units.
    let bytes = producer_of(&result.artifacts[0].bytes);
    assert_eq!(&bytes[..2], &[0xFE, 0xFF], "expected UTF-16BE BOM");
    let units: Vec<u16> = bytes[2..]
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect();
    assert_eq!(String::from_utf16(&units).unwrap(), override_str);
}

#[test]
fn producer_composes_with_signature_field() {
    // One incremental update must carry both: a signature widget AND the
    // /Producer stamp.
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#signature-field("a")
"#;
    let pdf = render_pdf(plate);

    // /Producer present.
    let expected = format!("Quillmark {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(producer_of(&pdf), expected.as_bytes());

    // AcroForm signature field still present and resolvable.
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1, "expected one signature field");
}
