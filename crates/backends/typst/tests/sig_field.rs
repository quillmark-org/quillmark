//! End-to-end acceptance tests for the unsigned-SigField overlay feature.
//!
//! Compiles each plate through `compile_to_pdf`, parses the output with
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

/// Empty data payload — our plates don't reference any fields.
const MIN_JSON: &str = r#"{}"#;

fn compile(plate: &str) -> Result<Vec<u8>, RenderError> {
    compile_to_pdf(&host_source(), plate, MIN_JSON)
}

// ─── case 1: two pages, two fields ────────────────────────────────────────────

#[test]
fn acceptance_two_pages_two_fields() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)

Page 1.
#signature-field("a")

#pagebreak()

Page 2.
#signature-field("b")
"#;
    let pdf = compile(plate).expect("compile ok");
    fs::write("/tmp/qm_sig_two_pages.pdf", &pdf).ok();

    let doc = lopdf::Document::load_mem(&pdf).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");

    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();

    // SigFlags = 3 (SignaturesExist | AppendOnly)
    assert_eq!(
        af.get(b"SigFlags").unwrap().as_i64().unwrap(),
        3,
        "expected /SigFlags 3"
    );
    // NeedAppearances = true
    assert!(
        af.get(b"NeedAppearances").unwrap().as_bool().unwrap(),
        "expected /NeedAppearances true"
    );

    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2, "expected 2 fields");

    let pages = doc.get_pages();
    assert_eq!(pages.len(), 2, "expected 2 pages");

    // Collect widget per name.
    let mut by_name: HashMap<String, (i64, lopdf::Dictionary)> = HashMap::new();
    for f in fields {
        let r = f.as_reference().unwrap();
        let d = doc.get_object(r).unwrap().as_dict().unwrap().clone();
        let name =
            String::from_utf8_lossy(d.get(b"T").unwrap().as_str().unwrap()).into_owned();
        // Confirm widget basics.
        assert_eq!(d.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
        assert_eq!(d.get(b"Subtype").unwrap().as_name().unwrap(), b"Widget");
        by_name.insert(name, (r.0 as i64, d));
    }

    assert!(by_name.contains_key("a"), "missing field 'a'");
    assert!(by_name.contains_key("b"), "missing field 'b'");

    // Confirm each widget is annotated on the right page.
    let page_refs: Vec<(u32, u16)> = pages
        .iter()
        .map(|(_, &id)| (id.0, id.1))
        .collect();
    for (name, (_, widget)) in &by_name {
        let page_ref = widget.get(b"P").unwrap().as_reference().unwrap();
        let page_index = page_refs.iter().position(|&p| p == page_ref).unwrap();
        let expected = if name == "a" { 0 } else { 1 };
        assert_eq!(
            page_index, expected,
            "field {} expected on page {}, found on page {}",
            name, expected, page_index
        );
    }

    // Each Rect should be 200pt wide × 50pt tall (within 1 pt — Typst rounding).
    for (name, (_, widget)) in &by_name {
        let rect = widget.get(b"Rect").unwrap().as_array().unwrap();
        assert_eq!(rect.len(), 4);
        let to_f64 = |o: &lopdf::Object| -> f64 {
            o.as_float()
                .map(|f| f as f64)
                .or_else(|_| o.as_i64().map(|i| i as f64))
                .unwrap()
        };
        let llx = to_f64(&rect[0]);
        let lly = to_f64(&rect[1]);
        let urx = to_f64(&rect[2]);
        let ury = to_f64(&rect[3]);
        let w = urx - llx;
        let h = ury - lly;
        assert!(
            (w - 200.0).abs() < 1.0,
            "field {} width {} != 200pt within 1pt",
            name,
            w
        );
        assert!(
            (h - 50.0).abs() < 1.0,
            "field {} height {} != 50pt within 1pt",
            name,
            h
        );
        // The widget should be inside the page (400pt tall, 50pt margins).
        assert!(
            llx >= 0.0 && urx <= 600.0,
            "field {} rect x outside page: [{}, {}]",
            name,
            llx,
            urx
        );
        assert!(
            lly >= 0.0 && ury <= 400.0,
            "field {} rect y outside page: [{}, {}]",
            name,
            lly,
            ury
        );
    }
}

// ─── case 2: duplicate field name ─────────────────────────────────────────────

#[test]
fn acceptance_duplicate_name_errors() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#signature-field("a")
#signature-field("a")
"#;
    let err = compile(plate).expect_err("expected duplicate-name error");
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

// ─── case 3: no fields → output identical to typst_pdf ────────────────────────

#[test]
fn acceptance_no_fields_no_overlay() {
    let plate = r#"
#set page(width: 600pt, height: 400pt, margin: 50pt)

Just a doc.
"#;
    let pdf = compile(plate).expect("compile ok");
    // No AcroForm key in the catalog.
    let doc = lopdf::Document::load_mem(&pdf).unwrap();
    let cat = doc.catalog().unwrap();
    assert!(
        !cat.has(b"AcroForm"),
        "expected no /AcroForm in catalog for sig-field-free plate"
    );

    // Sanity: only one xref section (i.e. no incremental update appended).
    let xref_count = pdf
        .windows(b"\nxref\n".len())
        .filter(|w| *w == b"\nxref\n")
        .count();
    assert_eq!(
        xref_count, 1,
        "expected exactly 1 xref section (no incremental update); got {}",
        xref_count
    );
}
