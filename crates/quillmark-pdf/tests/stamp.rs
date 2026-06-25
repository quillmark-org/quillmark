//! Acceptance tests for [`quillmark_pdf::stamp`]: build a minimal traditional-xref
//! PDF, stamp every field type onto it, then reparse with lopdf and assert the
//! AcroForm structure and the returned regions sidecar.

use lopdf::Object;
use quillmark_pdf::{
    stamp, Appearance, ChoiceOption, FieldSpec, FieldType, RegionKind, StampOptions,
};

/// A minimal single-page PDF with a traditional xref table and no `/Info`.
fn minimal_pdf() -> Vec<u8> {
    let bodies: [&[u8]; 3] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref_off = pdf.len();
    pdf.extend_from_slice(b"xref\n");
    pdf.extend_from_slice(format!("0 {}\n", bodies.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    pdf.extend_from_slice(b"trailer\n");
    pdf.extend_from_slice(format!("<< /Size {} /Root 1 0 R >>\n", bodies.len() + 1).as_bytes());
    pdf.extend_from_slice(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    pdf
}

fn text_field(max_len: u32, value: &str) -> FieldSpec {
    let mut f = FieldSpec::new("full_name", 0, [72.0, 700.0, 272.0, 720.0], FieldType::Text);
    f.max_len = Some(max_len);
    f.value = Some(value.to_string());
    f.tooltip = Some("Full legal name".to_string());
    f.appearance = Appearance {
        da: Some("/Helv 11 Tf 0 g".to_string()),
        border_color: Some([0.0, 0.0, 0.0]),
        background_color: None,
    };
    f
}

fn all_fields() -> Vec<FieldSpec> {
    vec![
        text_field(64, "Ada Lovelace"),
        FieldSpec::new(
            "agree",
            0,
            [72.0, 670.0, 92.0, 690.0],
            FieldType::Checkbox {
                on_state: "Yes".to_string(),
                checked: true,
            },
        ),
        FieldSpec::new(
            "color",
            0,
            [72.0, 640.0, 272.0, 660.0],
            FieldType::Choice {
                options: vec![ChoiceOption::new("red"), ChoiceOption::new("blue")],
                combo: true,
            },
        ),
        FieldSpec::new("sig", 0, [72.0, 600.0, 272.0, 640.0], FieldType::Signature),
    ]
}

fn acroform(doc: &lopdf::Document) -> &lopdf::Dictionary {
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    doc.get_object(af_ref).unwrap().as_dict().unwrap()
}

fn widget_by_name<'a>(doc: &'a lopdf::Document, name: &str) -> &'a lopdf::Dictionary {
    let fields = acroform(doc).get(b"Fields").unwrap().as_array().unwrap();
    for f in fields {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        if w.get(b"T").unwrap().as_str().unwrap() == name.as_bytes() {
            return w;
        }
    }
    panic!("no widget named {name}");
}

#[test]
fn stamps_all_field_types_with_structure() {
    let res = stamp(minimal_pdf(), &all_fields(), &StampOptions::default()).expect("stamp ok");
    let doc = lopdf::Document::load_mem(&res.pdf).expect("reparse");

    let af = acroform(&doc);
    assert_eq!(af.get(b"Fields").unwrap().as_array().unwrap().len(), 4);
    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    // A signature is present, so SigFlags is set.
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    // Non-signature fields are present, so /DA and /DR fonts are registered.
    assert!(af.has(b"DA"));
    let dr = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"Helv") && fonts.has(b"ZaDb"));

    // Text
    let t = widget_by_name(&doc, "full_name");
    assert_eq!(t.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(t.get(b"MaxLen").unwrap().as_i64().unwrap(), 64);
    assert_eq!(t.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert_eq!(t.get(b"TU").unwrap().as_str().unwrap(), b"Full legal name");

    // Checkbox
    let c = widget_by_name(&doc, "agree");
    assert_eq!(c.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(c.get(b"AS").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(c.get(b"V").unwrap().as_name().unwrap(), b"Yes");

    // Choice (combo bit set in /Ff)
    let ch = widget_by_name(&doc, "color");
    assert_eq!(ch.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    let ff = ch.get(b"Ff").unwrap().as_i64().unwrap();
    assert_ne!(ff & (1 << 17), 0, "combo bit must be set");
    assert_eq!(ch.get(b"Opt").unwrap().as_array().unwrap().len(), 2);

    // Signature: exactly one /Subtype, FT Sig
    let s = widget_by_name(&doc, "sig");
    assert_eq!(s.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    assert_eq!(s.get(b"Subtype").unwrap().as_name().unwrap(), b"Widget");
}

#[test]
fn returns_regions_for_every_field() {
    let res = stamp(minimal_pdf(), &all_fields(), &StampOptions::default()).expect("stamp ok");
    assert_eq!(res.regions.len(), 4);
    let kinds: Vec<&str> = res
        .regions
        .iter()
        .map(|r| match &r.kind {
            RegionKind::Field { field_type, .. } => field_type.as_str(),
        })
        .collect();
    assert_eq!(kinds, ["text", "checkbox", "choice", "signature"]);
    let sig = res.regions.iter().find(|r| r.name == "sig").unwrap();
    assert_eq!(sig.rect, [72.0, 600.0, 272.0, 640.0]);
    assert_eq!(sig.page, 0);
}

#[test]
fn producer_is_stamped_and_default() {
    let res = stamp(minimal_pdf(), &[], &StampOptions::default()).expect("stamp ok");
    let doc = lopdf::Document::load_mem(&res.pdf).expect("reparse");

    // No fields → no AcroForm, but a /Producer /Info was appended.
    assert!(!doc.catalog().unwrap().has(b"AcroForm"));
    let info_ref = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    let producer = match info.get(b"Producer").unwrap() {
        Object::String(b, _) => b.clone(),
        other => panic!("not a string: {other:?}"),
    };
    let expected = format!("Quillmark {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(producer, expected.as_bytes());
}

#[test]
fn producer_override_applies() {
    let res = stamp(
        minimal_pdf(),
        &[],
        &StampOptions {
            producer: Some("ACME Forms 9".to_string()),
        },
    )
    .expect("stamp ok");
    let doc = lopdf::Document::load_mem(&res.pdf).expect("reparse");
    let info_ref = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    let producer = match info.get(b"Producer").unwrap() {
        Object::String(b, _) => b.clone(),
        other => panic!("not a string: {other:?}"),
    };
    assert_eq!(producer, b"ACME Forms 9");
}

#[test]
fn field_on_missing_page_errors() {
    let mut f = FieldSpec::new("x", 5, [0.0, 0.0, 10.0, 10.0], FieldType::Signature);
    f.value = None;
    let err = stamp(minimal_pdf(), &[f], &StampOptions::default()).expect_err("should error");
    let msg = format!("{err:?}");
    assert!(msg.contains("page 5"), "unexpected error: {msg}");
}
