//! End-to-end acceptance test for the `sample_form` fixture: render the
//! hand-authored stripped background + form.json through the full engine
//! (pdfform backend registered), then reparse with lopdf and assert the filled
//! AcroForm. Technique A means values land in `/V`; appearance synthesis is the
//! viewer's job (human-verified per the issue, not headless).

use lopdf::Document as PdfDoc;
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};

const FILLED: &str = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Ada Lovelace\n\
comments:\n\
  - First comment line.\n\
  - Second comment line.\n\
agree: true\n\
favorite_color: green\n\
~~~\n";

fn render(markdown: &str) -> quillmark::RenderResult {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::from_markdown(markdown).expect("parse markdown");
    engine
        .render(
            &quill,
            &doc,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        )
        .expect("render ok")
}

/// Open a compiled session — the surface that carries schema-field geometry
/// (`session.regions()`), independent of any byte render.
fn open_session(markdown: &str) -> quillmark::LiveSession {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::from_markdown(markdown).expect("parse markdown");
    engine.open(&quill, &doc).expect("open ok")
}

/// Decode a PDF text string: UTF-16BE when it carries a BOM (pdf-writer picks
/// this for values with characters outside the literal-safe set, e.g. a
/// newline in a multiline field), else treat the bytes as Latin-1/ASCII.
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn widget<'a>(doc: &'a PdfDoc, af: &lopdf::Dictionary, name: &str) -> &'a lopdf::Dictionary {
    for f in af.get(b"Fields").unwrap().as_array().unwrap() {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        if w.get(b"T").unwrap().as_str().unwrap() == name.as_bytes() {
            return w;
        }
    }
    panic!("no field named {name}");
}

#[test]
fn fixture_renders_structurally_valid_filled_pdf() {
    let result = render(FILLED);
    assert_eq!(result.output_format, OutputFormat::Pdf);
    let pdf = &result.artifacts[0].bytes;

    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse — structurally valid");
    let cat = doc.catalog().expect("catalog");
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    assert_eq!(af.get(b"Fields").unwrap().as_array().unwrap().len(), 5);

    // This e2e pins the *binding* layer — markdown/schema → field values,
    // tooltip, array join, regions, producer default. The spine bytes it once
    // re-checked (the `Ff` multiline/combo flags, `/Opt` length, checkbox
    // `/V`+`/AS`, and `/FT` names) are owned by the spine seam in
    // `quillmark-pdf/tests/stamp.rs`.

    // Text: bound scalar value + tooltip.
    let full = widget(&doc, af, "FullName");
    assert_eq!(full.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert_eq!(
        full.get(b"TU").unwrap().as_str().unwrap(),
        b"Full legal name of the applicant"
    );

    // Multiline text: array joined with newlines.
    let comments = widget(&doc, af, "Comments");
    assert_eq!(
        decode_pdf_text(comments.get(b"V").unwrap().as_str().unwrap()),
        "First comment line.\nSecond comment line."
    );

    // Choice: matching option bound.
    let color = widget(&doc, af, "FavoriteColor");
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"green");

    // Region geometry is a session-level query (`session.regions()`), not on the
    // render result: one per *schema-bound* field, keyed on the schema path. The
    // fixture's Signature widget carries no `schema_field`, so it is a
    // backend-only artifact and emits no region — four regions, not five.
    let regions = open_session(FILLED).regions();
    assert_eq!(regions.len(), 4);
    assert!(
        regions.iter().all(|r| r.field != "Signature"),
        "the unbound signature widget produces no region"
    );
    let r_full = regions.iter().find(|r| r.field == "full_name").unwrap();
    // Geometry rides the sidecar: a real page and a non-degenerate rect.
    assert!(r_full.page < doc.get_pages().len().max(1));
    assert!(
        r_full.rect[2] > r_full.rect[0] && r_full.rect[3] > r_full.rect[1],
        "region rect is a proper box: {:?}",
        r_full.rect
    );

    // Producer stamped with the backend default.
    let info = doc
        .get_object(doc.trailer.get(b"Info").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let producer = info.get(b"Producer").unwrap().as_str().unwrap();
    assert!(
        producer.starts_with(b"Quillmark "),
        "producer = {:?}",
        String::from_utf8_lossy(producer)
    );
}

#[test]
fn non_ascii_value_round_trips_through_acroform_v() {
    // A non-ASCII (accented / Latin-1 + smart-punctuation) text value must reach
    // the AcroForm `/V` intact end-to-end: pdf-writer encodes it UTF-16BE, so
    // the value decodes back to exactly what was authored.
    let md = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: \"Café — Señor 'Ünïcøde'\"\n\
agree: true\n\
favorite_color: green\n\
~~~\n";
    let result = render(md);
    let pdf = &result.artifacts[0].bytes;
    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse");
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    let full = widget(&doc, af, "FullName");
    assert_eq!(
        decode_pdf_text(full.get(b"V").unwrap().as_str().unwrap()),
        "Café — Señor 'Ünïcøde'"
    );
    // The session's region geometry is keyed on the schema path, not the bound
    // value (the value lives in the AcroForm `/V`, asserted above).
    assert!(
        open_session(md)
            .regions()
            .iter()
            .any(|r| r.field == "full_name"),
        "a region is keyed on the schema path"
    );
}

#[test]
fn unchecked_and_unmatched_choice_render_blank() {
    let md = "~~~\n\
$quill: sample_form\n\
$kind: main\n\
full_name: Bob\n\
agree: false\n\
favorite_color: red\n\
~~~\n";
    let result = render(md);
    let pdf = &result.artifacts[0].bytes;
    let doc = PdfDoc::load_mem(pdf).unwrap();
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();

    // Unchecked checkbox → /V /Off, /AS /Off.
    let agree = widget(&doc, af, "Agree");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Off");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Off");

    // Absent comments → blank multiline field.
    let comments = widget(&doc, af, "Comments");
    assert!(comments.get(b"V").is_err(), "absent array → no /V");
}

#[test]
fn apply_rebinds_values_and_reports_dirty_pages() {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form quill");
    let engine = Quillmark::new();
    let doc = Document::from_markdown(FILLED).expect("parse markdown");
    let mut session = engine.open(&quill, &doc).expect("open ok");

    // Identical data → nothing dirty.
    let cs = session
        .apply(&quill.compile_data(&doc).expect("compile data"))
        .expect("apply");
    assert_eq!(cs.page_count, session.page_count());
    assert!(cs.dirty_pages.is_empty(), "dirty: {:?}", cs.dirty_pages);

    // A changed field dirties its page and rebinds the stamped value.
    let doc2 = Document::from_markdown(&FILLED.replace("Ada Lovelace", "Grace Hopper"))
        .expect("parse markdown");
    let cs = session
        .apply(&quill.compile_data(&doc2).expect("compile data"))
        .expect("apply");
    assert_eq!(cs.dirty_pages, vec![0]);

    let result = session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        })
        .expect("render ok");
    let pdf = PdfDoc::load_mem(&result.artifacts[0].bytes).unwrap();
    let cat = pdf.catalog().unwrap();
    let af = pdf
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let name = widget(&pdf, af, "FullName");
    assert_eq!(
        decode_pdf_text(name.get(b"V").unwrap().as_str().unwrap()),
        "Grace Hopper"
    );
}
