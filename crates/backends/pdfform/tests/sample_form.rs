//! End-to-end acceptance test for the `sample_form` fixture: render the
//! hand-authored stripped background + form.json through the full engine
//! (pdfform backend registered), then reparse with lopdf and assert the filled
//! AcroForm. Technique A means values land in `/V`; appearance synthesis is the
//! viewer's job (human-verified per the issue, not headless).

use lopdf::Document as PdfDoc;
use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use quillmark_core::RegionKind;

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
    render_with(markdown, false)
}

fn render_with(markdown: &str, flatten: bool) -> quillmark::RenderResult {
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
                flatten,
                ..Default::default()
            },
        )
        .expect("render ok")
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

    // Text: bound scalar.
    let full = widget(&doc, af, "FullName");
    assert_eq!(full.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(full.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert_eq!(
        full.get(b"TU").unwrap().as_str().unwrap(),
        b"Full legal name of the applicant"
    );

    // Multiline text: array joined with newlines.
    let comments = widget(&doc, af, "Comments");
    assert_eq!(
        comments.get(b"Ff").unwrap().as_i64().unwrap() & (1 << 12),
        1 << 12
    );
    assert_eq!(
        decode_pdf_text(comments.get(b"V").unwrap().as_str().unwrap()),
        "First comment line.\nSecond comment line."
    );

    // Checkbox: truthy → on-state.
    let agree = widget(&doc, af, "Agree");
    assert_eq!(agree.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Yes");

    // Choice: matching option bound; combo dropdown.
    let color = widget(&doc, af, "FavoriteColor");
    assert_eq!(color.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    assert_eq!(
        color.get(b"Ff").unwrap().as_i64().unwrap() & (1 << 17),
        1 << 17
    );
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"green");
    assert_eq!(color.get(b"Opt").unwrap().as_array().unwrap().len(), 3);

    // Signature: unbound, no /V.
    let sig = widget(&doc, af, "Signature");
    assert_eq!(sig.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    assert!(sig.get(b"V").is_err());

    // Regions sidecar: one per field, carrying bound values.
    assert_eq!(result.regions.len(), 5);
    let r_full = result
        .regions
        .iter()
        .find(|r| r.name == "FullName")
        .unwrap();
    match &r_full.kind {
        quillmark_core::RegionKind::Field { field_type, value } => {
            assert_eq!(field_type, "text");
            assert_eq!(value.as_deref(), Some("Ada Lovelace"));
        }
    }

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
fn flatten_produces_no_acroform_and_structurally_valid_pdf() {
    let result = render_with(FILLED, true);
    assert_eq!(result.output_format, OutputFormat::Pdf);
    let pdf = &result.artifacts[0].bytes;

    let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse — structurally valid");
    let cat = doc.catalog().expect("catalog");

    // Flatten path MUST NOT produce an AcroForm — values are in content streams.
    assert!(
        cat.get(b"AcroForm").is_err(),
        "flat PDF must not contain /AcroForm"
    );

    // Regions sidecar still present (same geometry, same values).
    assert_eq!(result.regions.len(), 5);
    let r_full = result
        .regions
        .iter()
        .find(|r| r.name == "FullName")
        .unwrap();
    match &r_full.kind {
        RegionKind::Field { field_type, value } => {
            assert_eq!(field_type, "text");
            assert_eq!(value.as_deref(), Some("Ada Lovelace"));
        }
    }
}

#[test]
fn flatten_pdf_contains_helv_font_and_content_streams() {
    let result = render_with(FILLED, true);
    let pdf = &result.artifacts[0].bytes;

    // The flat PDF must contain Helvetica and ZapfDingbats font objects and at
    // least one content stream with PDF text operators.
    let pdf_text = String::from_utf8_lossy(pdf);
    assert!(
        pdf_text.contains("/Helvetica"),
        "flat PDF must declare Helvetica"
    );
    assert!(
        pdf_text.contains("/ZapfDingbats"),
        "flat PDF must declare ZapfDingbats (for checkbox glyph)"
    );
    assert!(
        pdf_text.contains("BT\n") || pdf_text.contains("BT "),
        "flat PDF must contain BT (begin text) operator"
    );
    assert!(
        pdf_text.contains("Tj\n") || pdf_text.contains("Tj "),
        "flat PDF must contain Tj (show text) operator"
    );
    // The field value must appear in the stream.
    assert!(
        pdf_text.contains("Ada Lovelace"),
        "flat PDF must contain the FullName value"
    );
    // The text font declares WinAnsiEncoding so the drawn bytes render correctly.
    assert!(
        pdf_text.contains("/Encoding /WinAnsiEncoding"),
        "flat text font must declare WinAnsiEncoding"
    );
    // Text blocks clip to their field box (`re W n`) so values can't overflow.
    assert!(
        pdf_text.contains(" re W n"),
        "flat text must clip to the field box"
    );
}

#[test]
fn flatten_transcodes_non_ascii_to_winansi() {
    // Accented Latin-1, a CP1252-block em-dash, and a curly quote must reach the
    // flat content stream as their WinAnsi bytes (not raw UTF-8), so a flat
    // rasterizer renders them with the WinAnsi-encoded Helvetica.
    let markdown = "~~~\n\
        $quill: sample_form\n\
        $kind: main\n\
        full_name: \"Caf\u{e9} \u{2014} Se\u{f1}or \u{2019}A\u{2019}\"\n\
        comments: []\n\
        agree: false\n\
        favorite_color: green\n\
        ~~~\n";
    let result = render_with(markdown, true);
    let pdf = &result.artifacts[0].bytes;

    // WinAnsi bytes: é→0xE9, —→0x97, ñ→0xF1, ’→0x92. The drawn literal is
    // `Caf<E9> <97> Se<F1>or <92>A<92>`.
    let want: &[u8] = &[
        b'C', b'a', b'f', 0xE9, b' ', 0x97, b' ', b'S', b'e', 0xF1, b'o', b'r', b' ', 0x92, b'A',
        0x92,
    ];
    assert!(
        pdf.windows(want.len()).any(|w| w == want),
        "flat PDF must contain the WinAnsi-encoded value bytes"
    );
    // The raw UTF-8 multi-byte sequence for é (0xC3 0xA9) must NOT appear in a
    // drawn text literal — that would be the pre-fix corruption.
    assert!(
        !pdf.windows(4).any(|w| w == [b'f', 0xC3, 0xA9, b' ']),
        "value must not be drawn as raw UTF-8"
    );

    // The region sidecar keeps the original Unicode value intact.
    let r = result
        .regions
        .iter()
        .find(|r| r.name == "FullName")
        .unwrap();
    match &r.kind {
        RegionKind::Field { value, .. } => {
            assert_eq!(
                value.as_deref(),
                Some("Caf\u{e9} \u{2014} Se\u{f1}or \u{2019}A\u{2019}")
            );
        }
    }
}
