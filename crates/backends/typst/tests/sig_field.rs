//! End-to-end acceptance tests for the unsigned-SigField overlay feature.
//!
//! Compiles each main file through `compile_to_pdf`, parses the output with
//! lopdf, and asserts the AcroForm structure. Manual Acrobat verification
//! still required per the spec — see `prose/...` or the PR description.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{FileTreeNode, QuillSource, RenderError};
use quillmark_typst::compile::compile_to_pdf;

/// Load a fixture quill from disk. Reuses `usaf_memo@0.1.0` as the host
/// — `signature-field` doesn't depend on any quill-specific config so we
/// just need a valid quill skeleton, and `usaf_memo` is the fixture the
/// spikes validated.
fn host_source() -> QuillSource {
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
        .join("0.1.0");
    let tree = walk(&quill_path).expect("walk fixture");
    QuillSource::from_tree(tree).expect("load source")
}

/// Empty data payload — our main files don't reference any fields.
const MIN_JSON: &str = r#"{}"#;

fn compile(main: &str) -> Result<Vec<u8>, RenderError> {
    compile_to_pdf(&host_source(), main, MIN_JSON)
}

// ─── regression: each widget has exactly one /Subtype entry ──────────────────

/// `pdf-writer::Field::into_annotation` already emits `/Subtype /Widget`; an
/// earlier draft called `.subtype()` on the resulting Annotation too,
/// producing a malformed widget dict with `/Subtype` written twice. lopdf
/// silently tolerates the duplication; stricter validators (qpdf, MuPDF)
/// reject it. This test fences the regression at the byte level.
#[test]
fn regression_widget_dict_has_exactly_one_subtype() {
    let main = r#"
#import "@local/quillmark-helper:0.1.0": signature-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#signature-field("a")
"#;
    let pdf = compile(main).expect("compile ok");

    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1);
    let widget_ref = fields[0].as_reference().unwrap();

    // Locate the widget object's raw byte range and count /Subtype occurrences.
    let header = format!("{} 0 obj", widget_ref.0);
    let h = header.as_bytes();
    let start = pdf
        .windows(h.len())
        .position(|w| w == h)
        .expect("widget header in PDF bytes");
    let after = &pdf[start..];
    let endobj = after
        .windows(b"endobj".len())
        .position(|w| w == b"endobj")
        .expect("endobj after widget");
    let body = &after[..endobj];
    let count = body
        .windows(b"/Subtype".len())
        .filter(|w| *w == b"/Subtype")
        .count();
    assert_eq!(
        count,
        1,
        "widget dict must declare /Subtype exactly once, got {count}:\n{}",
        String::from_utf8_lossy(body)
    );
}

// ─── case 1: two pages, two fields ────────────────────────────────────────────

#[test]
fn acceptance_two_pages_two_fields() {
    let main = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)

Page 1.
#signature-field("a")

#pagebreak()

Page 2.
#signature-field("b")
"#;
    let pdf = compile(main).expect("compile ok");
    fs::write("/tmp/qm_sig_two_pages.pdf", &pdf).ok();

    let doc = lopdf::Document::load_mem(&pdf).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");

    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 3);
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());

    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2);
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 2);

    let to_f64 = |o: &lopdf::Object| -> f64 {
        o.as_float()
            .map(|f| f as f64)
            .or_else(|_| o.as_i64().map(|i| i as f64))
            .unwrap()
    };
    let page_refs: Vec<(u32, u16)> = pages.iter().map(|(_, &id)| (id.0, id.1)).collect();

    for f in fields {
        let widget = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name =
            String::from_utf8_lossy(widget.get(b"T").unwrap().as_str().unwrap()).into_owned();
        assert_eq!(widget.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
        assert_eq!(
            widget.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Widget"
        );

        let page_ref = widget.get(b"P").unwrap().as_reference().unwrap();
        let page_index = page_refs.iter().position(|&p| p == page_ref).unwrap();
        let expected = if name == "a" { 0 } else { 1 };
        assert_eq!(page_index, expected, "field {name} on wrong page");

        let rect = widget.get(b"Rect").unwrap().as_array().unwrap();
        let (llx, lly, urx, ury) = (
            to_f64(&rect[0]),
            to_f64(&rect[1]),
            to_f64(&rect[2]),
            to_f64(&rect[3]),
        );
        assert!(
            (urx - llx - 200.0).abs() < 1.0,
            "field {name} width: {}",
            urx - llx
        );
        assert!(
            (ury - lly - 50.0).abs() < 1.0,
            "field {name} height: {}",
            ury - lly
        );
        assert!(
            llx >= 0.0 && urx <= 600.0 && lly >= 0.0 && ury <= 400.0,
            "field {name} rect outside page: [{llx}, {lly}, {urx}, {ury}]"
        );
    }
}

// ─── case 2: duplicate field name ─────────────────────────────────────────────

#[test]
fn acceptance_duplicate_name_errors() {
    let main = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#signature-field("a")
#signature-field("a")
"#;
    let err = compile(main).expect_err("expected duplicate-name error");
    let RenderError::CompilationFailed { diags } = &err else {
        panic!("expected CompilationFailed, got {:?}", err);
    };
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::duplicate_signature_field")),
        "expected typst::duplicate_signature_field diagnostic, got {:?}",
        diags
    );
    let msg = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("typst::duplicate_signature_field"))
        .unwrap()
        .message
        .as_str();
    assert!(
        msg.contains("\"a\"") || msg.contains("'a'") || msg.contains("a"),
        "diagnostic message should name the offending field: {msg}"
    );
}

// ─── case: user metadata with same label is ignored, real field still found ──

/// A user could plausibly attach their own `<__qm_sig__>` label to unrelated
/// metadata. The extractor's `kind` field check should filter such metadata
/// out without raising and without losing the real signature-field call.
#[test]
fn user_metadata_on_reserved_label_does_not_clobber() {
    let main = r#"
#import "@local/quillmark-helper:0.1.0": signature-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#metadata((kind: "something-else", note: "user's own metadata")) <__qm_sig__>
#signature-field("real_field")
"#;
    let pdf = compile(main).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let cat = doc.catalog().unwrap();
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(
        fields.len(),
        1,
        "expected exactly 1 real field, got {}",
        fields.len()
    );
    let widget = doc
        .get_object(fields[0].as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(
        widget.get(b"T").unwrap().as_str().unwrap(),
        b"real_field",
        "wrong field name survived extraction"
    );
}

// ─── case 3: no fields → output identical to typst_pdf ────────────────────────

#[test]
fn acceptance_no_fields_no_overlay() {
    let main = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)

Just a doc.
"#;
    let pdf = compile(main).expect("compile ok");
    // No AcroForm key in the catalog.
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let cat = doc.catalog().unwrap();
    assert!(
        !cat.has(b"AcroForm"),
        "expected no /AcroForm in catalog for sig-field-free main file"
    );

    // Sanity: only one startxref (i.e. no incremental update appended) and
    // no `/Prev` key in the trailer.
    let startxref_count = pdf
        .windows(b"startxref\n".len())
        .filter(|w| *w == b"startxref\n")
        .count();
    assert_eq!(
        startxref_count, 1,
        "expected exactly 1 startxref marker (no incremental update); got {}",
        startxref_count
    );
    assert!(
        !pdf.windows(b"/Prev".len()).any(|w| w == b"/Prev"),
        "fresh typst-pdf output should not declare /Prev in the trailer"
    );
}
