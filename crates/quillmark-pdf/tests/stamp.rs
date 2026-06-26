//! Acceptance tests for the stamp spine: build a tiny traditional-xref base
//! PDF with pdf-writer, stamp all four field types, reparse with lopdf, and
//! assert the AcroForm structure. Technique A means no `/AP` is baked — values
//! land in `/V` and the viewer synthesizes appearances.

use pdf_writer::{Content, Pdf, Rect, Ref};
use quillmark_pdf::{stamp, FieldSpec, FieldType, StampOptions};

/// Build an `n`-page base PDF (612×792, traditional xref) with a trivial
/// content stream per page. This is exactly the input contract the spine
/// requires and mirrors how the hand-authored fixture background is produced.
fn build_base_pdf(n: usize) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    pdf.catalog(catalog_id).pages(page_tree_id);

    let mut page_ids = Vec::new();
    let mut next = 3i32;
    // Pre-allocate page + content ids.
    let mut content_ids = Vec::new();
    for _ in 0..n {
        page_ids.push(Ref::new(next));
        next += 1;
        content_ids.push(Ref::new(next));
        next += 1;
    }

    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids(page_ids.iter().copied())
            .count(n as i32)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0));
    }

    for i in 0..n {
        {
            pdf.page(page_ids[i])
                .parent(page_tree_id)
                .media_box(Rect::new(0.0, 0.0, 612.0, 792.0))
                .contents(content_ids[i]);
        }
        // A resource-free content stream: stroke a box so the page has marks
        // without referencing an undefined font.
        let mut content = Content::new();
        content.set_line_width(1.0);
        content.rect(72.0, 700.0, 200.0, 20.0);
        content.stroke();
        pdf.stream(content_ids[i], &content.finish());
    }

    pdf.finish()
}

fn all_four_fields() -> Vec<FieldSpec> {
    vec![
        FieldSpec {
            name: "FullName".into(),
            page: 0,
            rect: [180.0, 700.0, 520.0, 720.0],
            field_type: FieldType::Text { multiline: false },
            value: Some("Ada Lovelace".into()),
            tooltip: Some("Full legal name".into()),
        },
        FieldSpec {
            name: "Comments".into(),
            page: 0,
            rect: [180.0, 600.0, 520.0, 680.0],
            field_type: FieldType::Text { multiline: true },
            value: None,
            tooltip: None,
        },
        FieldSpec {
            name: "Agree".into(),
            page: 0,
            rect: [180.0, 560.0, 194.0, 574.0],
            field_type: FieldType::Checkbox,
            value: Some(quillmark_pdf::CHECKBOX_ON_STATE.into()),
            tooltip: None,
        },
        FieldSpec {
            name: "FavoriteColor".into(),
            page: 0,
            rect: [180.0, 520.0, 520.0, 540.0],
            field_type: FieldType::Choice {
                options: vec!["red".into(), "green".into(), "blue".into()],
            },
            value: Some("green".into()),
            tooltip: None,
        },
    ]
}

#[test]
fn stamps_all_four_field_types_into_valid_acroform() {
    let base = build_base_pdf(1);
    let result = stamp(
        base,
        &all_four_fields(),
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result.pdf).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();

    // NeedAppearances + a signature-free form has no SigFlags.
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert!(af.get(b"SigFlags").is_err(), "no signature → no SigFlags");

    // /DR /Font /Helv registered.
    let dr = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"Helv"), "house font Helv registered in /DR");

    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 4);

    // Index widgets by /T.
    let mut by_name = std::collections::HashMap::new();
    for f in fields {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name = String::from_utf8_lossy(w.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, w);
    }

    let full = by_name.get("FullName").unwrap();
    assert_eq!(full.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(full.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert!(full.get(b"DA").is_ok(), "text field carries /DA");
    assert_eq!(
        full.get(b"TU").unwrap().as_str().unwrap(),
        b"Full legal name"
    );
    assert_eq!(full.get(b"Subtype").unwrap().as_name().unwrap(), b"Widget");

    let comments = by_name.get("Comments").unwrap();
    let ff = comments.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(ff & (1 << 12), 1 << 12, "multiline flag set");
    assert!(comments.get(b"V").is_err(), "blank field has no /V");

    let agree = by_name.get("Agree").unwrap();
    assert_eq!(agree.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Yes");

    let color = by_name.get("FavoriteColor").unwrap();
    assert_eq!(color.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    let cff = color.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(cff & (1 << 17), 1 << 17, "combo flag set");
    let opts = color.get(b"Opt").unwrap().as_array().unwrap();
    assert_eq!(opts.len(), 3);
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"green");

    // Exactly one /Subtype per widget (regression: into_annotation writes it).
    for f in fields {
        let r = f.as_reference().unwrap();
        let header = format!("{} 0 obj", r.0);
        let start = result
            .pdf
            .windows(header.len())
            .position(|w| w == header.as_bytes())
            .expect("widget header");
        let after = &result.pdf[start..];
        let endobj = after.windows(6).position(|w| w == b"endobj").unwrap();
        let body = &after[..endobj];
        let count = body.windows(8).filter(|w| *w == b"/Subtype").count();
        assert_eq!(count, 1, "exactly one /Subtype in widget {}", r.0);
    }

    // Regions sidecar: one per field, geometry matches.
    assert_eq!(result.regions.len(), 4);
    let agree_region = result.regions.iter().find(|r| r.name == "Agree").unwrap();
    assert_eq!(agree_region.rect, [180.0, 560.0, 194.0, 574.0]);
}

#[test]
fn signature_field_sets_sigflags() {
    let base = build_base_pdf(2);
    let fields = vec![FieldSpec {
        name: "Signature".into(),
        page: 1,
        rect: [180.0, 100.0, 520.0, 140.0],
        field_type: FieldType::Signature,
        value: None,
        tooltip: None,
    }];
    let result = stamp(base, &fields, &StampOptions::default()).expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result.pdf).expect("reparse");
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    let w = doc
        .get_object(
            af.get(b"Fields").unwrap().as_array().unwrap()[0]
                .as_reference()
                .unwrap(),
        )
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(w.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    // Widget is attached to the second page's /Annots.
    let pages = doc.get_pages();
    let page2 = doc
        .get_object(*pages.get(&2).unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(
        page2.has(b"Annots"),
        "signature widget added to page 2 /Annots"
    );
}

#[test]
fn no_producer_no_fields_is_identity() {
    let base = build_base_pdf(1);
    let before = base.clone();
    let result = stamp(base, &[], &StampOptions::default()).expect("stamp ok");
    assert_eq!(result.pdf, before, "no-op stamp returns base unchanged");
    assert!(result.regions.is_empty());
}

#[test]
fn field_targeting_missing_page_errors() {
    let base = build_base_pdf(1);
    let fields = vec![FieldSpec {
        name: "X".into(),
        page: 5,
        rect: [0.0, 0.0, 10.0, 10.0],
        field_type: FieldType::Signature,
        value: None,
        tooltip: None,
    }];
    let err = stamp(base, &fields, &StampOptions::default()).expect_err("out of range");
    assert!(err.message.contains("page"), "{}", err.message);
}
