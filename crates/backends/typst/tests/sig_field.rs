//! End-to-end acceptance tests for the unsigned-SigField overlay feature.
//!
//! Compiles each plate through the public `Backend`/`LiveSession` path,
//! parses the output with lopdf, and asserts the AcroForm structure. Manual
//! Acrobat verification still required per the spec — see `prose/...` or the
//! PR description.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderError, RenderOptions};
use quillmark_typst::TypstBackend;

/// Walk the `usaf_memo@0.2.0` fixture into an in-memory tree. Reused as a host
/// because `signature-field` doesn't depend on any quill-specific config —
/// any valid quill skeleton (fonts, packages) works.
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

/// Build the host quill with its `plate.typ` replaced by `plate`. The fixture
/// declares `typst.plate_file: plate.typ`, so the backend reads this override.
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

fn compile(plate: &str) -> Result<Vec<u8>, RenderError> {
    // Our plates don't reference data fields, so an empty payload suffices.
    compile_with_data(plate, &serde_json::json!({}))
}

/// Like [`compile`] but threads `json_data` to the plate's `data` binding, so a
/// plate can exercise value binding via `data.*`.
fn compile_with_data(plate: &str, json_data: &serde_json::Value) -> Result<Vec<u8>, RenderError> {
    let source = source_with_plate(plate);
    let session = TypstBackend.open(&source, json_data)?;
    let result = session.render(&RenderOptions {
        output_format: Some(OutputFormat::Pdf),
        ..Default::default()
    })?;
    Ok(result.artifacts[0].bytes.clone())
}

/// Render `plate` (with optional `json_data`) and return the parsed lopdf
/// document plus a map from field name (`/T`) to its AcroForm widget dict.
fn acroform_widgets(
    plate: &str,
    json_data: &serde_json::Value,
) -> (
    lopdf::Document,
    std::collections::HashMap<String, lopdf::Dictionary>,
) {
    let pdf = compile_with_data(plate, json_data).expect("compile ok");
    let doc = lopdf::Document::load_mem(&pdf).expect("reparse");
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    let mut by_name = std::collections::HashMap::new();
    for f in fields {
        let widget = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name =
            String::from_utf8_lossy(widget.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, widget.clone());
    }
    (doc, by_name)
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

    let doc = lopdf::Document::load_mem(&pdf).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");

    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
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
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field

#set page(width: 600pt, height: 400pt, margin: 50pt)
#signature-field("a")
#signature-field("a")
"#;
    let err = compile(plate).expect_err("expected duplicate-name error");
    let diags = err.diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_deref() == Some("typst::duplicate_form_field")),
        "expected typst::duplicate_form_field diagnostic, got {:?}",
        diags
    );
    let msg = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("typst::duplicate_form_field"))
        .unwrap()
        .message
        .as_str();
    assert!(
        msg.contains("\"a\"") || msg.contains("'a'") || msg.contains("a"),
        "diagnostic message should name the offending field: {msg}"
    );
}

// ─── case: user metadata with same label is ignored, real field still found ──

/// A user could plausibly attach their own `<__qm_field__>` label to unrelated
/// metadata. The extractor's `kind` field check should filter such metadata
/// out without raising and without losing the real form-field call.
#[test]
fn user_metadata_on_reserved_label_does_not_clobber() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": signature-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#metadata((kind: "something-else", note: "user's own metadata")) <__qm_field__>
#signature-field("real_field")
"#;
    let pdf = compile(plate).expect("compile ok");
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

    // The signature overlay is skipped (no fields), but the always-on
    // `/Producer` metadata pass appends exactly one incremental update — so
    // expect two startxref markers and a single `/Prev` chain entry.
    let startxref_count = pdf
        .windows(b"startxref\n".len())
        .filter(|w| *w == b"startxref\n")
        .count();
    assert_eq!(
        startxref_count, 2,
        "expected 2 startxref markers (one Producer-metadata incremental update); got {}",
        startxref_count
    );
    assert_eq!(
        pdf.windows(b"/Prev".len())
            .filter(|w| *w == b"/Prev")
            .count(),
        1,
        "expected exactly one /Prev (the Producer-metadata incremental update)"
    );
}

// ─── generalized form-field types ────────────────────────────────────────────
//
// These assert the typst→spec *mapping* (the bound `/V`, checkbox truthiness,
// choice option-matching). The spine bytes they used to re-check — the
// MULTILINE/COMBO `Ff` flag bits and the `/MK /CA (4)` checkbox glyph — are
// owned by `quillmark-pdf/tests/stamp.rs` at the spine seam.

/// case: text fields — single-line and multiline; the bound `/V` string lands
/// on the widget.
#[test]
fn form_field_text_single_and_multiline() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("single", type: "text", value: "hello")
#form-field("multi", type: "text", value: "a\nb", multiline: true)
"#;
    let (_doc, widgets) = acroform_widgets(plate, &serde_json::json!({}));

    let single = widgets.get("single").expect("single field");
    assert_eq!(single.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(single.get(b"V").unwrap().as_str().unwrap(), b"hello");

    let multi = widgets.get("multi").expect("multi field");
    assert_eq!(multi.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
}

/// case: checkbox — `/FT /Btn`; `/V` and `/AS` are `/Yes` when bound truthy and
/// `/Off` when not.
#[test]
fn form_field_checkbox_checked_and_unchecked() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("agree", type: "checkbox", value: true)
#form-field("decline", type: "checkbox", value: false)
"#;
    let (_doc, widgets) = acroform_widgets(plate, &serde_json::json!({}));

    let on = widgets.get("agree").expect("agree field");
    assert_eq!(on.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(on.get(b"V").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(on.get(b"AS").unwrap().as_name().unwrap(), b"Yes");

    let off = widgets.get("decline").expect("decline field");
    assert_eq!(off.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(off.get(b"V").unwrap().as_name().unwrap(), b"Off");
    assert_eq!(off.get(b"AS").unwrap().as_name().unwrap(), b"Off");
}

/// case: choice — `/FT /Ch`; `/Opt` carries the options; `/V` carries the
/// chosen option when it matches, and is absent when it does not.
#[test]
fn form_field_choice_options_and_value_matching() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("color", type: "choice", options: ("Red", "Green", "Blue"), value: "Green")
#form-field("bad", type: "choice", options: ("Red", "Green", "Blue"), value: "Purple")
"#;
    let (_doc, widgets) = acroform_widgets(plate, &serde_json::json!({}));

    let color = widgets.get("color").expect("color field");
    assert_eq!(color.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    let opts = color.get(b"Opt").unwrap().as_array().unwrap();
    let opt_strs: Vec<String> = opts
        .iter()
        .map(|o| String::from_utf8_lossy(o.as_str().unwrap()).into_owned())
        .collect();
    assert_eq!(opt_strs, vec!["Red", "Green", "Blue"]);
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"Green");

    // A value matching no option is dropped: no /V (or an empty one).
    let bad = widgets.get("bad").expect("bad field");
    assert_eq!(bad.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    match bad.get(b"V") {
        Err(_) => {}
        Ok(lopdf::Object::String(s, _)) => assert!(
            s.is_empty(),
            "non-matching choice value should be blank, got {:?}",
            String::from_utf8_lossy(s)
        ),
        Ok(other) => panic!("unexpected /V on non-matching choice: {other:?}"),
    }
}

/// case: signature via the general helper — `/FT /Sig`, value-free, unchanged
/// from the dedicated `signature-field`.
#[test]
fn form_field_signature_via_general_helper() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("sig", type: "signature")
"#;
    let (doc, widgets) = acroform_widgets(plate, &serde_json::json!({}));
    let sig = widgets.get("sig").expect("sig field");
    assert_eq!(sig.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    assert!(sig.get(b"V").is_err(), "signature field must carry no /V");

    // /SigFlags still asserted on the form when a signature is present.
    let cat = doc.catalog().unwrap();
    let af_ref = cat.get(b"AcroForm").unwrap().as_reference().unwrap();
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
}

/// case: value binding from real `json_data` — the plate reads `data.*` and the
/// bound values land in each widget's `/V`.
#[test]
fn form_field_value_binding_from_data() {
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": data, form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("name", type: "text", value: data.full_name)
#form-field("agree", type: "checkbox", value: data.agreed)
#form-field("color", type: "choice", options: ("Red", "Green", "Blue"), value: data.color)
#form-field("count", type: "text", value: str(data.count))
"#;
    let json = serde_json::json!({
        "full_name": "Ada Lovelace",
        "agreed": true,
        "color": "Blue",
        "count": 7,
    });
    let (_doc, widgets) = acroform_widgets(plate, &json);

    assert_eq!(
        widgets
            .get("name")
            .unwrap()
            .get(b"V")
            .unwrap()
            .as_str()
            .unwrap(),
        b"Ada Lovelace"
    );
    assert_eq!(
        widgets
            .get("agree")
            .unwrap()
            .get(b"V")
            .unwrap()
            .as_name()
            .unwrap(),
        b"Yes"
    );
    assert_eq!(
        widgets
            .get("color")
            .unwrap()
            .get(b"V")
            .unwrap()
            .as_str()
            .unwrap(),
        b"Blue"
    );
    assert_eq!(
        widgets
            .get("count")
            .unwrap()
            .get(b"V")
            .unwrap()
            .as_str()
            .unwrap(),
        b"7"
    );
}

/// case: `session.regions()` exposes a region only for a `field:`-bound widget,
/// keyed on that schema path (of any field type), each carrying page+geometry. A
/// widget that binds no schema field has only a `/T` name — not a schema
/// address — and surfaces nothing. A session-level query, not a render output.
/// `field:` validates against the schema like `tagged`, so the test owns its
/// schema (declaring every bound field) rather than borrowing the host
/// fixture's field inventory — a fixture edit cannot break a widget test.
#[test]
fn form_field_regions_key_on_bound_schema_field() {
    const YAML: &str = r#"
quill:
  name: widget_regions
  version: 0.1.0
  backend: typst
  description: form-field region binding test
typst:
  plate_file: plate.typ
main:
  fields:
    f_txt:
      type: string
      description: text widget binding
    f_chk:
      type: boolean
      description: checkbox widget binding
    f_cho:
      type: string
      description: choice widget binding
    f_sig:
      type: string
      description: signature widget binding
"#;
    let plate = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("txt", type: "text", value: "hi", field: "f_txt")
#form-field("chk", type: "checkbox", value: true, field: "f_chk")
#form-field("cho", type: "choice", options: ("A", "B"), value: "B", field: "f_cho")
#form-field("sig", type: "signature", field: "f_sig")
#form-field("unbound", type: "text", value: "x")
"#;
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: YAML.as_bytes().to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate.as_bytes().to_vec(),
        },
    );
    let source = Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill");
    let session = TypstBackend
        .open(&source, &serde_json::json!({}))
        .expect("open");
    let regions = session.regions();

    let fields: std::collections::HashMap<&str, &quillmark_core::RenderedRegion> =
        regions.iter().map(|r| (r.field.as_str(), r)).collect();

    for field in ["f_txt", "f_chk", "f_cho", "f_sig"] {
        let r = fields
            .get(field)
            .unwrap_or_else(|| panic!("region keyed on bound schema field {field:?}"));
        assert_eq!(r.page, 0);
        assert!(
            r.rect[2] > r.rect[0] && r.rect[3] > r.rect[1],
            "region {field:?} rect is a proper box: {:?}",
            r.rect
        );
    }
    // The unbound widget (no `field:`) is not schema-addressable: no region, and
    // its `/T` name never leaks as a region key.
    assert!(
        !fields.contains_key("unbound"),
        "an unbound widget exposes no region: {:?}",
        fields.keys().collect::<Vec<_>>()
    );
    // A bound widget keys only on its schema path — its `/T` name must not also
    // leak as a region key.
    for t_name in ["txt", "chk", "cho", "sig"] {
        assert!(
            !fields.contains_key(t_name),
            "a bound widget must not also leak its `/T` name {t_name:?}: {:?}",
            fields.keys().collect::<Vec<_>>()
        );
    }
}
