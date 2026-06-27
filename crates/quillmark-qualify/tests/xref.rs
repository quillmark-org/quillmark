//! Force-traditional-xref: build a form saved as an xref-STREAM PDF (lopdf
//! `CrossReferenceStream`), qualify it, and assert the stripped output is
//! traditional xref — `quillmark_pdf::page_media_boxes` (which calls
//! `assert_traditional_xref` internally) must accept it, and the raw bytes must
//! carry an `xref` table marker, not an `/Type /XRef` stream.

use lopdf::{dictionary, Document, Object, StringFormat};
use quillmark_pdf::page_media_boxes;
use quillmark_qualify::qualify;

fn pdf_str(s: &str) -> Object {
    Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
}

/// A one-page AcroForm PDF saved with a cross-reference STREAM.
fn build_xref_stream_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");

    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let field_id = doc.new_object_id();

    doc.set_object(
        field_id,
        dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => pdf_str("FullName"),
            "Rect" => Object::Array(vec![
                Object::Integer(100), Object::Integer(700),
                Object::Integer(400), Object::Integer(720),
            ]),
            "P" => Object::Reference(page_id),
        },
    );

    let acroform_id = doc.add_object(dictionary! {
        "Fields" => Object::Array(vec![Object::Reference(field_id)]),
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
            "Annots" => Object::Array(vec![Object::Reference(field_id)]),
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
    // Save as a cross-reference STREAM (the shape the spine's reader rejects).
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceStream;

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save xref-stream pdf");
    buf
}

#[test]
fn xref_stream_input_yields_traditional_xref_output() {
    let input = build_xref_stream_pdf();

    // Sanity: the input really is an xref stream (no `xref` table; has /XRef).
    let input_str = String::from_utf8_lossy(&input);
    assert!(
        input_str.contains("/Type /XRef") || input_str.contains("/Type/XRef"),
        "input must be an xref stream"
    );

    let qualified = qualify(&input, None).expect("qualify xref-stream pdf");

    // The spine's geometry reader calls `assert_traditional_xref` internally;
    // accepting the output proves it is traditional xref.
    let boxes = page_media_boxes(&qualified.form_pdf)
        .expect("stripped output must be traditional-xref (spine-readable)");
    assert_eq!(boxes.len(), 1);

    // And the raw bytes carry a traditional `xref` table marker (not a stream).
    let out_str = String::from_utf8_lossy(&qualified.form_pdf);
    assert!(
        out_str.contains("\nxref\n") || out_str.contains("\rxref\r") || out_str.contains("xref\n"),
        "output must contain a traditional xref table marker"
    );
    assert!(
        !out_str.contains("/Type /XRef") && !out_str.contains("/Type/XRef"),
        "output must NOT contain an xref stream"
    );
}
