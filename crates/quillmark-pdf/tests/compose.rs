//! Acceptance tests for the page-merge primitive (`concat_pdf_pages`, behind
//! the `compose` feature). They merge real single-page bases (the `gov_form`
//! fixture's `form.pdf`), then validate the merged document against the stamp
//! spine's input contract and prove the core "compose content first, **stamp
//! last** over a combined page set" mechanism: a widget stamped on an offset
//! page that exists ONLY because of the merge lands on the right page.
#![cfg(feature = "compose")]

use lopdf::{dictionary, Document, Object, ObjectId};
use quillmark_pdf::{
    concat_pdf_pages, page_media_boxes, reader, stamp, FieldSpec, FieldType, StampOptions,
};

/// The hand-authored single-page gov_form background: a traditional-xref PDF
/// whose `/MediaBox` is INHERITED from the `/Pages` root (not on the page dict),
/// so the merge must resolve geometry down for the box to survive re-parenting.
const GOV_FORM_PDF: &[u8] =
    include_bytes!("../../fixtures/resources/quills/gov_form/0.1.0/form.pdf");

/// The single-page box of the gov_form fixture.
fn single_box() -> [f32; 4] {
    let boxes = page_media_boxes(GOV_FORM_PDF).expect("base media boxes");
    assert_eq!(boxes.len(), 1, "fixture is single-page");
    boxes[0]
}

/// Reparse merged bytes with lopdf and return the document.
fn reparse(bytes: &[u8]) -> Document {
    Document::load_mem(bytes).expect("merged PDF reparses with lopdf")
}

/// The `/Page` object ids in document order, from a reparsed merged doc.
fn page_ids(doc: &Document) -> Vec<ObjectId> {
    doc.get_pages().into_values().collect()
}

#[test]
fn two_way_concat_page_count_and_geometry() {
    let merged = concat_pdf_pages(&[GOV_FORM_PDF, GOV_FORM_PDF]).expect("merge two bases");

    // Two pages, each with the original single-page box — geometry survived the
    // re-parent (it was INHERITED in the source).
    let boxes = page_media_boxes(&merged).expect("merged media boxes");
    assert_eq!(boxes.len(), 2, "two inputs → two pages");
    let expected = single_box();
    assert_eq!(boxes[0], expected, "page 0 box preserved");
    assert_eq!(boxes[1], expected, "page 1 box preserved");

    // The spine's input contract: traditional xref, resolvable through the
    // crate's own reader (the exact path `stamp` walks).
    let xref = reader::find_startxref(&merged).expect("startxref");
    reader::assert_traditional_xref(&merged, xref).expect("traditional xref");

    // No /AcroForm, no page /Annots, no /Encrypt in the merged background.
    let doc = reparse(&merged);
    assert!(
        doc.catalog().unwrap().get(b"AcroForm").is_err(),
        "merged base must have no /AcroForm"
    );
    assert!(doc.trailer.get(b"Encrypt").is_err(), "no /Encrypt");
    for pid in page_ids(&doc) {
        let page = doc.get_object(pid).unwrap().as_dict().unwrap();
        assert!(
            !page.has(b"Annots"),
            "merged page {pid:?} must have no /Annots"
        );
        // Geometry resolved DOWN onto the page dict.
        assert!(
            page.has(b"MediaBox"),
            "inherited /MediaBox must be resolved onto page {pid:?}"
        );
    }
}

#[test]
fn stamp_last_over_combined_set_lands_per_page() {
    // Merge two bases, then stamp ONE field on page 0 and ONE on page 1. Page 1
    // exists only because of the merge — this is the headless proof of "stamp
    // last over a combined page set with mixed geometry sources."
    let merged = concat_pdf_pages(&[GOV_FORM_PDF, GOV_FORM_PDF]).expect("merge");
    let mb = single_box();

    // A simple rect near the top of each page (bottom-left origin).
    let rect = [mb[0] + 72.0, mb[3] - 100.0, mb[0] + 272.0, mb[3] - 80.0];
    let fields = vec![
        FieldSpec {
            name: "OnPage0".into(),
            page: 0,
            rect,
            field_type: FieldType::Text { multiline: false },
            value: Some("page zero".into()),
            tooltip: None,
        },
        FieldSpec {
            name: "OnPage1".into(),
            page: 1,
            rect,
            field_type: FieldType::Text { multiline: false },
            value: Some("page one".into()),
            tooltip: None,
        },
    ];

    let stamped = stamp(merged, &fields, &StampOptions::default()).expect("stamp combined set");
    let doc = reparse(&stamped.pdf);
    let pages = page_ids(&doc);
    assert_eq!(pages.len(), 2, "still two pages after stamping");

    // For each page, collect the field names reachable through its /Annots, via
    // each widget's /T. Assert OnPage0 ∈ page 0 only, OnPage1 ∈ page 1 only.
    let names_on = |page_idx: usize| -> Vec<String> {
        let page = doc.get_object(pages[page_idx]).unwrap().as_dict().unwrap();
        let annots = page
            .get(b"Annots")
            .ok()
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default();
        annots
            .iter()
            .filter_map(|o| o.as_reference().ok())
            .filter_map(|id| doc.get_object(id).ok().and_then(|o| o.as_dict().ok()))
            .filter_map(|w| w.get(b"T").ok().and_then(|o| o.as_str().ok()))
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect()
    };

    let p0 = names_on(0);
    let p1 = names_on(1);
    assert!(
        p0.contains(&"OnPage0".to_string()),
        "OnPage0 on page 0: {p0:?}"
    );
    assert!(
        !p0.contains(&"OnPage1".to_string()),
        "OnPage1 must NOT be on page 0: {p0:?}"
    );
    assert!(
        p1.contains(&"OnPage1".to_string()),
        "OnPage1 on page 1: {p1:?}"
    );
    assert!(
        !p1.contains(&"OnPage0".to_string()),
        "OnPage0 must NOT be on page 1: {p1:?}"
    );

    // And each widget's own /P points at its page object.
    for (page_idx, want) in [(0usize, "OnPage0"), (1, "OnPage1")] {
        let page = doc.get_object(pages[page_idx]).unwrap().as_dict().unwrap();
        let annots = page.get(b"Annots").unwrap().as_array().unwrap();
        let mut found = false;
        for o in annots {
            let wid = o.as_reference().unwrap();
            let w = doc.get_object(wid).unwrap().as_dict().unwrap();
            let t = w.get(b"T").ok().and_then(|o| o.as_str().ok());
            if t == Some(want.as_bytes()) {
                let p = w.get(b"P").unwrap().as_reference().unwrap();
                assert_eq!(p, pages[page_idx], "{want} /P points at its own page");
                found = true;
            }
        }
        assert!(found, "{want} widget present on page {page_idx}");
    }
}

#[test]
fn single_input_round_trips_to_one_page() {
    let merged = concat_pdf_pages(&[GOV_FORM_PDF]).expect("merge single");
    let boxes = page_media_boxes(&merged).expect("media boxes");
    assert_eq!(boxes.len(), 1, "single input → one page");
    assert_eq!(boxes[0], single_box(), "geometry preserved");

    let xref = reader::find_startxref(&merged).expect("startxref");
    reader::assert_traditional_xref(&merged, xref).expect("traditional xref");

    // Still stampable as a clean 1-page base.
    let doc = reparse(&merged);
    assert_eq!(page_ids(&doc).len(), 1);
    assert!(doc.catalog().unwrap().get(b"AcroForm").is_err());
}

#[test]
fn three_way_concat_page_count() {
    let merged =
        concat_pdf_pages(&[GOV_FORM_PDF, GOV_FORM_PDF, GOV_FORM_PDF]).expect("merge three");
    let boxes = page_media_boxes(&merged).expect("media boxes");
    assert_eq!(boxes.len(), 3, "three inputs → three pages");
    for b in &boxes {
        assert_eq!(*b, single_box(), "each page keeps the box");
    }

    // The flat /Pages root reports the right /Count.
    let doc = reparse(&merged);
    let pages_root = doc
        .catalog()
        .unwrap()
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    let count = doc
        .get_object(pages_root)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Count")
        .unwrap()
        .as_i64()
        .unwrap();
    assert_eq!(count, 3, "/Pages /Count == 3");

    // Empty-input is an error.
    assert!(concat_pdf_pages(&[]).is_err(), "empty input → error");
}

#[test]
fn inherited_indirect_resources_survive_merge() {
    // A base whose `/Resources` is an INDIRECT ref on the `/Pages` root, inherited
    // by a page that declares none. The merge must resolve it down AND offset its
    // reference with the rest of the input's objects, or the merged page's
    // `/Resources` would dangle (point at a since-renumbered id) — which the stamp
    // path tolerates (it reads only `/MediaBox`) but a renderer of the merged page
    // would not. This guards the inherited-indirect case the gov_form fixture
    // (direct `/MediaBox` array) does not exercise.
    let mut doc = Document::new();
    doc.version = "1.7".to_string();
    let font_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Font".to_vec()),
        "Subtype" => Object::Name(b"Type1".to_vec()),
        "BaseFont" => Object::Name(b"Helvetica".to_vec()),
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => Object::Dictionary(dictionary! { "F1" => Object::Reference(font_id) }),
    });
    let page_id = doc.new_object_id();
    let pages_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Pages".to_vec()),
        "Kids" => Object::Array(vec![Object::Reference(page_id)]),
        "Count" => Object::Integer(1),
        "MediaBox" => Object::Array(vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ]),
        // Inherited, INDIRECT — the case under test.
        "Resources" => Object::Reference(resources_id),
    });
    doc.set_object(
        page_id,
        dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            // no /Resources, no /MediaBox — both inherited.
        },
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    let mut base = Vec::new();
    doc.save_to(&mut base).expect("save synthetic base");

    let merged = concat_pdf_pages(&[&base]).expect("merge base with inherited indirect resources");
    let out = reparse(&merged);
    let pid = page_ids(&out)[0];
    let page = out.get_object(pid).unwrap().as_dict().unwrap();

    // /Resources resolved down as an indirect ref that points at a REAL object.
    let res_ref = page
        .get(b"Resources")
        .expect("page inherited /Resources")
        .as_reference()
        .expect("/Resources stayed an indirect ref");
    let res = out
        .get_object(res_ref)
        .expect("/Resources ref resolves after merge (not dangling)");
    // The nested font ref inside the inherited resources resolves too.
    let font_ref = res
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"F1")
        .unwrap()
        .as_reference()
        .unwrap();
    out.get_object(font_ref)
        .expect("nested font ref inside inherited /Resources resolves");
}
