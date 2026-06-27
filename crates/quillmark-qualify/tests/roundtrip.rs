//! HEADLINE round-trip oracle: stamp the gov_form fixture into a real AcroForm
//! PDF with the spine, then `qualify` it back and assert the reconstructed
//! `FormSpec` reproduces the fixture's — same fields, geometry, types, tooltips,
//! and snake_case bindings — and that the stripped background satisfies the
//! spine's input contract.
//!
//! Approach: we build the AcroForm PDF with `quillmark_pdf::stamp` directly
//! (the cleanest robust route per the spec). The fixture's `form.json` is the
//! oracle on both ends: `FieldSpec`s are built from it (replicating pdfform's
//! 4-line top-left→bottom-left flip inline, with empty data so values are
//! blank), stamped onto the fixture's `form.pdf`, and the qualify output is
//! compared field-by-field back against it.

use quillmark_pdf::{page_media_boxes, stamp, FieldSpec, FieldType, StampOptions};
use quillmark_pdfform::{FieldKind, FormField, FormSpec, Rect};
use quillmark_qualify::qualify;

const EPS: f32 = 0.01;

fn fixture_dir() -> std::path::PathBuf {
    quillmark_fixtures::quills_path("gov_form")
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture_dir().join(name)).expect("read fixture")
}

/// The exact top-left → bottom-left flip from pdfform's `resolve::flip_rect`
/// (replicated inline, 4 lines, to avoid a render-pipeline dep in the test).
fn flip_rect(r: Rect, mb: [f32; 4]) -> [f32; 4] {
    let left = mb[0];
    let top = mb[3];
    [left + r.x, top - (r.y + r.h), left + r.x + r.w, top - r.y]
}

fn field_type_of(kind: &FieldKind) -> FieldType {
    match kind {
        FieldKind::Text { multiline } => FieldType::Text {
            multiline: *multiline,
        },
        FieldKind::Checkbox => FieldType::Checkbox,
        FieldKind::Choice { options } => FieldType::Choice {
            options: options.clone(),
        },
        FieldKind::Signature => FieldType::Signature,
    }
}

/// Build a blank-valued AcroForm PDF from the fixture's form.pdf + form.json.
fn build_stamped() -> Vec<u8> {
    let base = read_fixture("form.pdf");
    let form_json = read_fixture("form.json");
    let spec = FormSpec::parse(&form_json).expect("parse fixture form.json");
    let boxes = page_media_boxes(&base).expect("media boxes");

    let specs: Vec<FieldSpec> = spec
        .fields
        .iter()
        .map(|f| FieldSpec {
            name: f.name.clone(),
            page: f.page,
            rect: flip_rect(f.rect, boxes[f.page]),
            field_type: field_type_of(&f.kind),
            value: None, // blank: qualification is value-free
            tooltip: f.tooltip.clone(),
        })
        .collect();

    stamp(base, &specs, &StampOptions::default())
        .expect("stamp")
        .pdf
}

fn find<'a>(spec: &'a FormSpec, name: &str) -> &'a FormField {
    spec.fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("qualified spec missing field {name:?}"))
}

fn rect_eq(a: Rect, b: Rect) {
    assert!(
        (a.x - b.x).abs() <= EPS
            && (a.y - b.y).abs() <= EPS
            && (a.w - b.w).abs() <= EPS
            && (a.h - b.h).abs() <= EPS,
        "rect mismatch: {a:?} vs {b:?}"
    );
}

#[test]
fn roundtrip_reproduces_gov_form_spec() {
    let stamped = build_stamped();
    let oracle = FormSpec::parse(&read_fixture("form.json")).expect("parse oracle");

    let qualified = qualify(&stamped, None).expect("qualify");
    let got = FormSpec::parse(&qualified.form_json).expect("parse qualified form.json");

    // Same set of fields by name.
    let mut got_names: Vec<&str> = got.fields.iter().map(|f| f.name.as_str()).collect();
    let mut want_names: Vec<&str> = oracle.fields.iter().map(|f| f.name.as_str()).collect();
    got_names.sort_unstable();
    want_names.sort_unstable();
    assert_eq!(
        got_names, want_names,
        "field name set must match the oracle"
    );

    // Field-by-field equality.
    for want in &oracle.fields {
        let g = find(&got, &want.name);
        assert_eq!(g.page, want.page, "page mismatch for {}", want.name);
        assert_eq!(g.kind, want.kind, "kind mismatch for {}", want.name);
        assert_eq!(
            g.tooltip, want.tooltip,
            "tooltip mismatch for {}",
            want.name
        );
        assert_eq!(
            g.schema_field, want.schema_field,
            "schema_field mismatch for {}",
            want.name
        );
        rect_eq(g.rect, want.rect);
    }

    // schema_field == snake_case(name) for the bound (non-signature) fields, and
    // that equals the oracle's hand-authored values.
    assert_eq!(
        find(&got, "FullName").schema_field.as_deref(),
        Some("full_name")
    );
    assert_eq!(
        find(&got, "Comments").schema_field.as_deref(),
        Some("comments")
    );
    assert_eq!(find(&got, "Agree").schema_field.as_deref(), Some("agree"));
    assert_eq!(
        find(&got, "FavoriteColor").schema_field.as_deref(),
        Some("favorite_color")
    );
    // Signature is unbound.
    assert_eq!(find(&got, "Signature").schema_field, None);
}

#[test]
fn stripped_background_satisfies_spine_contract() {
    let stamped = build_stamped();
    let qualified = qualify(&stamped, None).expect("qualify");
    let form_pdf = &qualified.form_pdf;

    // Passes the spine's geometry reader (traditional-xref + resolvable pages).
    let boxes = page_media_boxes(form_pdf).expect("page_media_boxes on stripped bg");
    assert_eq!(boxes.len(), 1, "gov_form is one page");
    assert_eq!(boxes[0], [0.0, 0.0, 612.0, 792.0]);

    // Re-stamping the stripped background must succeed — the ultimate proof it
    // satisfies the spine's input contract (which calls `assert_traditional_xref`
    // and requires no `/Encrypt` and inline-or-absent `/Annots` internally).
    let restamp = stamp(
        form_pdf.clone(),
        &[FieldSpec {
            name: "Probe".into(),
            page: 0,
            rect: [10.0, 10.0, 100.0, 30.0],
            field_type: FieldType::Text { multiline: false },
            value: None,
            tooltip: None,
        }],
        &StampOptions::default(),
    );
    assert!(
        restamp.is_ok(),
        "stripped background must satisfy the spine input contract: {:?}",
        restamp.err()
    );

    // Reparse with lopdf: NO /AcroForm, NO page /Annots, NO /Encrypt.
    let doc = lopdf::Document::load_mem(form_pdf).expect("lopdf reparse");
    assert!(!doc.is_encrypted(), "stripped bg must be unencrypted");
    assert!(
        doc.trailer.get(b"Encrypt").is_err(),
        "trailer must have no /Encrypt"
    );
    let catalog = doc.catalog().expect("catalog");
    assert!(
        catalog.get(b"AcroForm").is_err(),
        "catalog must have no /AcroForm"
    );
    for (_, page_id) in doc.get_pages() {
        let page = doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .expect("page dict");
        assert!(
            page.get(b"Annots").is_err(),
            "page {page_id:?} must have no /Annots"
        );
    }
}
