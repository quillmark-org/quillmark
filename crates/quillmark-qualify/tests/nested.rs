//! Field-tree shapes a real AcroForm carries that the spine never produces,
//! built with lopdf directly:
//!
//! - **Nested + inherited `/FT`**: a parent field node carries `/FT /Tx` and a
//!   child terminal widget omits `/FT` but has `/T` + `/Rect`. The child must
//!   resolve to `Text` via the inherited `/FT`, and its FQ name must be
//!   `parent.child`.
//! - **Field/widget split** (the dominant Acrobat/gov-form shape): a field node
//!   with `/T` + `/FT` but NO `/Rect`, whose widget-annotation kid (`/Subtype
//!   /Widget`, has `/Rect` + `/P`, NO `/T`) carries the geometry. Qualify must
//!   read geometry from the widget kid, not error.

use lopdf::{dictionary, Document, Object, StringFormat};
use quillmark_pdfform::{FieldKind, FormSpec};
use quillmark_qualify::qualify;

fn pdf_str(s: &str) -> Object {
    Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
}

/// Build a one-page traditional-xref PDF whose AcroForm has a parent field node
/// (`/T (section)`, `/FT /Tx`) with a single child terminal widget (`/T (name)`,
/// no `/FT`, has `/Rect`).
fn build_nested() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");

    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let parent_id = doc.new_object_id();
    let child_id = doc.new_object_id();

    // Child terminal widget — omits /FT, inherits /Tx from the parent.
    doc.set_object(
        child_id,
        dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "Parent" => Object::Reference(parent_id),
            "T" => pdf_str("name"),
            "Rect" => Object::Array(vec![
                Object::Integer(100),
                Object::Integer(700),
                Object::Integer(300),
                Object::Integer(720),
            ]),
            "P" => Object::Reference(page_id),
        },
    );

    // Parent field node — carries /FT /Tx and the partial name `section`.
    doc.set_object(
        parent_id,
        dictionary! {
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => pdf_str("section"),
            "Kids" => Object::Array(vec![Object::Reference(child_id)]),
        },
    );

    let acroform_id = doc.add_object(dictionary! {
        "Fields" => Object::Array(vec![Object::Reference(parent_id)]),
        "NeedAppearances" => Object::Boolean(true),
    });

    doc.set_object(
        page_id,
        dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => Object::Array(vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ]),
            "Annots" => Object::Array(vec![Object::Reference(child_id)]),
        },
    );

    doc.set_object(
        pages_id,
        dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => Object::Array(vec![Object::Reference(page_id)]),
            "Count" => Object::Integer(1),
        },
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => Object::Reference(acroform_id),
    });

    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save nested form");
    buf
}

#[test]
fn nested_child_inherits_ft_and_fq_name() {
    let pdf = build_nested();
    let qualified = qualify(&pdf, None).expect("qualify nested");
    let spec = FormSpec::parse(&qualified.form_json).expect("parse qualified form.json");

    assert_eq!(spec.fields.len(), 1, "exactly one terminal field");
    let f = &spec.fields[0];

    // Inherited /FT /Tx → Text.
    assert_eq!(
        f.kind,
        FieldKind::Text { multiline: false },
        "child must resolve to Text via inherited /FT"
    );
    // Fully-qualified name joins parent.child.
    assert_eq!(f.name, "section.name", "FQ name must be parent.child");
    assert_eq!(f.schema_field.as_deref(), Some("section_name"));

    // Geometry: rect [100 700 300 720] bottom-left on a 792-tall zero-origin
    // page → top-left {x:100, y:792-720=72, w:200, h:20}.
    assert!((f.rect.x - 100.0).abs() < 0.01);
    assert!((f.rect.y - 72.0).abs() < 0.01);
    assert!((f.rect.w - 200.0).abs() < 0.01);
    assert!((f.rect.h - 20.0).abs() < 0.01);
}

/// Build a one-page SPLIT-shape AcroForm: a field node (`/T (FullName)`,
/// `/FT /Tx`, NO `/Rect`, `/Kids [widget]`) whose widget kid carries the
/// geometry (`/Subtype /Widget`, `/Rect`, `/P (page)`, NO `/T`). The widget is
/// in the page `/Annots`; the FIELD NODE is in `/AcroForm /Fields`.
fn build_split() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");

    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let field_id = doc.new_object_id();
    let widget_id = doc.new_object_id();

    // Widget-annotation kid: geometry only, no /T, no /FT.
    doc.set_object(
        widget_id,
        dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "Parent" => Object::Reference(field_id),
            "Rect" => Object::Array(vec![
                Object::Integer(150),
                Object::Integer(600),
                Object::Integer(450),
                Object::Integer(620),
            ]),
            "P" => Object::Reference(page_id),
        },
    );

    // Field node: /T + /FT + /TU, NO /Rect, single widget kid.
    doc.set_object(
        field_id,
        dictionary! {
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => pdf_str("FullName"),
            "TU" => pdf_str("Full legal name of the applicant"),
            "Kids" => Object::Array(vec![Object::Reference(widget_id)]),
        },
    );

    let acroform_id = doc.add_object(dictionary! {
        "Fields" => Object::Array(vec![Object::Reference(field_id)]),
        "NeedAppearances" => Object::Boolean(true),
    });

    doc.set_object(
        page_id,
        dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => Object::Array(vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ]),
            // The page annotation is the WIDGET, not the field node.
            "Annots" => Object::Array(vec![Object::Reference(widget_id)]),
        },
    );

    doc.set_object(
        pages_id,
        dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => Object::Array(vec![Object::Reference(page_id)]),
            "Count" => Object::Integer(1),
        },
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => Object::Reference(acroform_id),
    });

    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save split form");
    buf
}

#[test]
fn split_field_reads_geometry_from_widget_kid() {
    let pdf = build_split();
    let qualified = qualify(&pdf, None).expect("qualify split form");
    let spec = FormSpec::parse(&qualified.form_json).expect("parse qualified form.json");

    // Exactly ONE field — the widget kid must NOT become a second field.
    assert_eq!(spec.fields.len(), 1, "split shape yields exactly one field");
    let f = &spec.fields[0];

    assert_eq!(f.name, "FullName", "field name from the field node /T");
    assert_eq!(
        f.kind,
        FieldKind::Text { multiline: false },
        "/FT /Tx → Text"
    );
    assert_eq!(f.page, 0, "owning page resolved from the widget /P");
    assert_eq!(f.schema_field.as_deref(), Some("full_name"));
    assert_eq!(
        f.tooltip.as_deref(),
        Some("Full legal name of the applicant"),
        "/TU read from the field node"
    );

    // Geometry inverted from the WIDGET's /Rect [150 600 450 620] on a 792-tall
    // zero-origin page → top-left {x:150, y:792-620=172, w:300, h:20}.
    assert!((f.rect.x - 150.0).abs() < 0.01, "x = {}", f.rect.x);
    assert!((f.rect.y - 172.0).abs() < 0.01, "y = {}", f.rect.y);
    assert!((f.rect.w - 300.0).abs() < 0.01, "w = {}", f.rect.w);
    assert!((f.rect.h - 20.0).abs() < 0.01, "h = {}", f.rect.h);

    // Stripped background: no /AcroForm, no page /Annots, widget deleted.
    let doc = Document::load_mem(&qualified.form_pdf).expect("reparse stripped");
    assert!(
        doc.catalog().unwrap().get(b"AcroForm").is_err(),
        "no /AcroForm"
    );
    for (_, pid) in doc.get_pages() {
        let page = doc.get_object(pid).and_then(|o| o.as_dict()).unwrap();
        assert!(page.get(b"Annots").is_err(), "no page /Annots");
    }
}
