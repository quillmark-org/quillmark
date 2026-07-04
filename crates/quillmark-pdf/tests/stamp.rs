//! Acceptance tests for the stamp spine: build a tiny traditional-xref base
//! PDF with pdf-writer, stamp all four field types, reparse with lopdf, and
//! assert the AcroForm structure. Technique A means no `/AP` is baked — values
//! land in `/V` and the viewer synthesizes appearances.

use pdf_writer::{Content, Pdf, Rect, Ref};
use quillmark_pdf::{regions_of, stamp, FieldSpec, FieldType, StampOptions};

/// Build an `n`-page base PDF (612×792, traditional xref) with a trivial
/// content stream per page. This is exactly the input contract the spine
/// requires and mirrors how the hand-authored fixture background is produced.
/// A US-Letter origin-zero specialization of [`build_base_pdf_origin`].
fn build_base_pdf(n: usize) -> Vec<u8> {
    build_base_pdf_origin(n, [0.0, 0.0, 612.0, 792.0])
}

fn all_four_fields() -> Vec<FieldSpec> {
    vec![
        FieldSpec {
            name: "FullName".into(),
            schema_field: Some("full_name".into()),
            page: 0,
            rect: [180.0, 700.0, 520.0, 720.0],
            field_type: FieldType::Text { multiline: false },
            value: Some("Ada Lovelace".into()),
            tooltip: Some("Full legal name".into()),
        },
        FieldSpec {
            name: "Comments".into(),
            schema_field: Some("comments".into()),
            page: 0,
            rect: [180.0, 600.0, 520.0, 680.0],
            field_type: FieldType::Text { multiline: true },
            value: None,
            tooltip: None,
        },
        FieldSpec {
            name: "Agree".into(),
            schema_field: Some("agree".into()),
            page: 0,
            rect: [180.0, 560.0, 194.0, 574.0],
            field_type: FieldType::Checkbox,
            value: Some(quillmark_pdf::CHECKBOX_ON_STATE.into()),
            tooltip: None,
        },
        FieldSpec {
            name: "FavoriteColor".into(),
            schema_field: Some("favorite_color".into()),
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

    let doc = lopdf::Document::load_mem(&result).expect("lopdf reparse");
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
            .windows(header.len())
            .position(|w| w == header.as_bytes())
            .expect("widget header");
        let after = &result[start..];
        let endobj = after.windows(6).position(|w| w == b"endobj").unwrap();
        let body = &after[..endobj];
        let count = body.windows(8).filter(|w| *w == b"/Subtype").count();
        assert_eq!(count, 1, "exactly one /Subtype in widget {}", r.0);
    }

    // Region geometry (a session-level query over the same specs): one per
    // field, keyed on the schema path, geometry matches.
    let regions = regions_of(&all_four_fields());
    assert_eq!(regions.len(), 4);
    let agree_region = regions.iter().find(|r| r.field == "agree").unwrap();
    assert_eq!(agree_region.rect, [180.0, 560.0, 194.0, 574.0]);
}

#[test]
fn signature_field_sets_sigflags() {
    let base = build_base_pdf(2);
    let fields = vec![FieldSpec {
        name: "Signature".into(),
        schema_field: Some("signature".into()),
        page: 1,
        rect: [180.0, 100.0, 520.0, 140.0],
        field_type: FieldType::Signature,
        value: None,
        tooltip: None,
    }];
    let result = stamp(base, &fields, &StampOptions::default()).expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("reparse");
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
    assert_eq!(result, before, "no-op stamp returns base unchanged");
    assert!(regions_of(&[]).is_empty(), "no fields → no regions");
}

#[test]
fn producer_only_no_fields_stamps_info_producer() {
    // producer=Some + no fields is NOT the identity short-circuit: it runs a
    // minimal `/Info`-only incremental append (no AcroForm). Assert the success
    // envelope reparses with the new /Producer and adds no /AcroForm.
    let base = build_base_pdf(1);
    let result = stamp(
        base,
        &[],
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("lopdf reparse");
    assert!(
        doc.catalog().unwrap().get(b"AcroForm").is_err(),
        "producer-only stamp must not add an /AcroForm"
    );
    let info_ref = doc
        .trailer
        .get(b"Info")
        .expect("trailer /Info")
        .as_reference()
        .expect("/Info indirect");
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    assert_eq!(
        info.get(b"Producer").unwrap().as_str().unwrap(),
        b"Quillmark test"
    );
}

#[test]
fn rotated_page_rejected_cleanly() {
    // A base whose target page carries a non-zero /Rotate is rejected rather
    // than mis-stamped (widget geometry is written in unrotated user space).
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    pdf.catalog(catalog_id).pages(page_tree_id);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids([page_id])
            .count(1)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0));
    }
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0))
            .rotate(90)
            .contents(content_id);
    }
    let mut content = Content::new();
    content.set_line_width(1.0);
    content.rect(72.0, 700.0, 200.0, 20.0);
    content.stroke();
    pdf.stream(content_id, &content.finish());
    let base = pdf.finish();

    let fields = vec![FieldSpec {
        name: "FullName".into(),
        schema_field: Some("full_name".into()),
        page: 0,
        rect: [180.0, 700.0, 520.0, 720.0],
        field_type: FieldType::Text { multiline: false },
        value: Some("Ada".into()),
        tooltip: None,
    }];
    let err = stamp(base, &fields, &StampOptions::default()).expect_err("rotated page rejected");
    assert_eq!(err.code, "pdf::rotated_page");
}

#[test]
fn implausible_size_errors_cleanly_without_panic() {
    // A base PDF whose trailer declares a near-u32::MAX /Size must yield a clean
    // PdfError (id space exhausted) rather than panic on overflow (debug) or
    // silently wrap into colliding object ids (release).
    let base = build_base_pdf(1);
    // Byte-level splice (the PDF binary-marker comment isn't valid UTF-8).
    let needle = b"/Size 5";
    let at = base
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("trailer /Size");
    let mut tampered = base[..at].to_vec();
    tampered.extend_from_slice(b"/Size 4294967295");
    tampered.extend_from_slice(&base[at + needle.len()..]);
    let err = stamp(
        tampered,
        &[],
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect_err("near-u32::MAX /Size should error");
    assert!(err.message.contains("id space"), "{}", err.message);
}

#[test]
fn field_targeting_missing_page_errors() {
    let base = build_base_pdf(1);
    let fields = vec![FieldSpec {
        name: "X".into(),
        schema_field: Some("x".into()),
        page: 5,
        rect: [0.0, 0.0, 10.0, 10.0],
        field_type: FieldType::Signature,
        value: None,
        tooltip: None,
    }];
    let err = stamp(base, &fields, &StampOptions::default()).expect_err("out of range");
    assert!(err.message.contains("page"), "{}", err.message);
}

// ───────────────────────── out-of-contract input rejection ─────────────────
//
// The spine's stance is "reject out-of-contract input cleanly" rather than
// emit a malformed-but-readable PDF. These exercise the hard-error branches.

/// Build an `n`-page base whose pages carry the given `/MediaBox`, so a test can
/// assert a non-zero page origin flows through `page_media_boxes`.
fn build_base_pdf_origin(n: usize, mb: [f32; 4]) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    pdf.catalog(catalog_id).pages(page_tree_id);

    let mut page_ids = Vec::new();
    let mut content_ids = Vec::new();
    let mut next = 3i32;
    for _ in 0..n {
        page_ids.push(Ref::new(next));
        next += 1;
        content_ids.push(Ref::new(next));
        next += 1;
    }
    let rect = Rect::new(mb[0], mb[1], mb[2], mb[3]);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids(page_ids.iter().copied())
            .count(n as i32)
            .media_box(rect);
    }
    for i in 0..n {
        pdf.page(page_ids[i])
            .parent(page_tree_id)
            .media_box(rect)
            .contents(content_ids[i]);
        let mut content = Content::new();
        content.set_line_width(1.0);
        content.rect(72.0, 700.0, 200.0, 20.0);
        content.stroke();
        pdf.stream(content_ids[i], &content.finish());
    }
    pdf.finish()
}

/// A one-page base whose page already carries an inline `/Annots [ref]`, so the
/// stamp's array-splice branch is exercised. Returns `(pdf, existing_annot_id)`.
fn build_base_with_inline_annot() -> (Vec<u8>, i32) {
    use pdf_writer::types::AnnotationType;
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    let annot_id = Ref::new(5);
    pdf.catalog(catalog_id).pages(page_tree_id);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids([page_id])
            .count(1)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0));
    }
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0))
            .contents(content_id);
        page.annotations([annot_id]);
    }
    {
        let mut content = Content::new();
        content.set_line_width(1.0);
        content.rect(72.0, 700.0, 200.0, 20.0);
        content.stroke();
        pdf.stream(content_id, &content.finish());
    }
    {
        pdf.annotation(annot_id)
            .subtype(AnnotationType::Text)
            .rect(Rect::new(10.0, 10.0, 30.0, 30.0));
    }
    (pdf.finish(), 5)
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("needle {:?} not found", String::from_utf8_lossy(needle)))
}

/// Equal-length in-place replacement of the first `needle`.
fn replace_first(pdf: &mut [u8], needle: &[u8], replacement: &[u8]) {
    assert_eq!(
        needle.len(),
        replacement.len(),
        "in-place replace keeps length"
    );
    let at = find_sub(pdf, needle);
    pdf[at..at + needle.len()].copy_from_slice(replacement);
}

/// Insert `insertion` immediately after the first `needle`.
fn insert_after(pdf: &[u8], needle: &[u8], insertion: &[u8]) -> Vec<u8> {
    let at = find_sub(pdf, needle) + needle.len();
    let mut out = pdf[..at].to_vec();
    out.extend_from_slice(insertion);
    out.extend_from_slice(&pdf[at..]);
    out
}

#[test]
fn nonzero_generation_catalog_rejected_cleanly() {
    // A base whose catalog lives at a non-zero generation parses fine for the
    // reader but would silently corrupt an incremental update (the writer
    // re-emits at gen 0 and points /Root at gen 0). Reject it.
    let mut base = build_base_pdf(1);
    // Bump the catalog (object 1) header and the trailer /Root ref to gen 2.
    replace_first(&mut base, b"1 0 obj", b"1 2 obj");
    replace_first(&mut base, b"/Root 1 0 R", b"/Root 1 2 R");
    let err = stamp(
        base,
        &[],
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect_err("non-zero generation catalog rejected");
    assert_eq!(err.code, "pdf::nonzero_generation");
    assert!(err.message.contains("generation 2"), "{}", err.message);
}

#[test]
fn nonzero_generation_page_rejected_cleanly() {
    // Same guard, reached via a page node a field targets (object 3).
    let mut base = build_base_pdf(1);
    replace_first(&mut base, b"3 0 obj", b"3 4 obj");
    let fields = vec![FieldSpec {
        name: "X".into(),
        schema_field: Some("x".into()),
        page: 0,
        rect: [10.0, 10.0, 100.0, 30.0],
        field_type: FieldType::Text { multiline: false },
        value: Some("hi".into()),
        tooltip: None,
    }];
    let err = stamp(base, &fields, &StampOptions::default())
        .expect_err("non-zero generation page rejected");
    assert_eq!(err.code, "pdf::nonzero_generation");
    assert!(err.message.contains("page"), "{}", err.message);
}

#[test]
fn encrypted_pdf_rejected_cleanly() {
    // Splice an /Encrypt entry into the trailer (after the xref table, so the
    // startxref offset stays valid). The spine must hard-error.
    let base = build_base_pdf(1);
    let tampered = insert_after(&base, b"/Root 1 0 R", b" /Encrypt 1 0 R");
    let err = stamp(
        tampered,
        &[],
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect_err("encrypted PDF rejected");
    assert_eq!(err.code, "pdf::encrypted");
}

#[test]
fn xref_stream_rejected_cleanly() {
    // Corrupt the traditional `xref` table marker in place so the reader sees a
    // non-`xref` byte run at the startxref offset — the xref-stream rejection
    // path. (`xref\n0 ` heads the table; `startxref\n<n>` never matches it.)
    let mut base = build_base_pdf(1);
    replace_first(&mut base, b"xref\n0", b"1 0 \n0");
    let err = stamp(
        base,
        &[],
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect_err("xref stream rejected");
    assert_eq!(err.code, "pdf::xref_stream");
}

#[test]
fn nonzero_mediabox_origin_flows_through() {
    // page_media_boxes returns the full origin-bearing rect, not just w/h, so a
    // caller can honour a non-zero page origin when flipping to bottom-left.
    let base = build_base_pdf_origin(1, [10.0, 20.0, 622.0, 812.0]);
    let boxes = quillmark_pdf::page_media_boxes(&base).expect("media boxes");
    assert_eq!(boxes, vec![[10.0, 20.0, 622.0, 812.0]]);
}

#[test]
fn inline_annots_are_merged_not_replaced() {
    // A page that already has an inline /Annots array must keep its existing
    // entries; the new widget refs are spliced in alongside them.
    let (base, existing) = build_base_with_inline_annot();
    let fields = vec![FieldSpec {
        name: "X".into(),
        schema_field: Some("x".into()),
        page: 0,
        rect: [10.0, 10.0, 100.0, 30.0],
        field_type: FieldType::Text { multiline: false },
        value: Some("hi".into()),
        tooltip: None,
    }];
    let result = stamp(base, &fields, &StampOptions::default()).expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("reparse");
    let pages = doc.get_pages();
    let page = doc
        .get_object(*pages.get(&1).unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let annots = page.get(b"Annots").unwrap().as_array().unwrap();
    let ids: Vec<u32> = annots
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .map(|(id, _)| id)
        .collect();
    assert!(
        ids.contains(&(existing as u32)),
        "existing annot {existing} preserved, got {ids:?}"
    );
    assert!(
        ids.len() >= 2,
        "widget appended alongside existing: {ids:?}"
    );
}

#[test]
fn indirect_annots_rejected_cleanly() {
    // The input contract requires inline /Annots; an indirect reference is a
    // hard error rather than a silently-dropped merge.
    let base = build_base_pdf(1);
    // Insert ` /Annots 99 0 R` before the page object's closing `>>`, then fix
    // the trailing startxref offset for the bytes inserted ahead of the table.
    let page_start = find_sub(&base, b"3 0 obj");
    let close = page_start + find_sub(&base[page_start..], b">>");
    let insertion = b" /Annots 99 0 R";
    let mut tampered = base[..close].to_vec();
    tampered.extend_from_slice(insertion);
    tampered.extend_from_slice(&base[close..]);
    // Re-point startxref past the inserted bytes (the table itself moved).
    {
        let marker = b"startxref\n";
        let pos = tampered
            .windows(marker.len())
            .rposition(|w| w == marker)
            .unwrap()
            + marker.len();
        let mut end = pos;
        while end < tampered.len() && tampered[end].is_ascii_digit() {
            end += 1;
        }
        let off: usize = std::str::from_utf8(&tampered[pos..end])
            .unwrap()
            .parse()
            .unwrap();
        let fixed = (off + insertion.len()).to_string();
        let mut out = tampered[..pos].to_vec();
        out.extend_from_slice(fixed.as_bytes());
        out.extend_from_slice(&tampered[end..]);
        tampered = out;
    }
    let fields = vec![FieldSpec {
        name: "X".into(),
        schema_field: Some("x".into()),
        page: 0,
        rect: [10.0, 10.0, 100.0, 30.0],
        field_type: FieldType::Text { multiline: false },
        value: Some("hi".into()),
        tooltip: None,
    }];
    let err =
        stamp(tampered, &fields, &StampOptions::default()).expect_err("indirect /Annots rejected");
    assert_eq!(err.code, "pdf::indirect_annots");
}

#[test]
fn xref_emits_multiple_subsections_when_ids_have_gaps() {
    // Overwriting low ids (catalog 1, page 3) while allocating fresh high ids
    // leaves gaps in the changed-id set, so the appended xref must coalesce into
    // multiple `<first> <count>` subsections rather than one contiguous run.
    let base = build_base_pdf(1);
    let result = stamp(
        base,
        &all_four_fields(),
        &StampOptions {
            producer: Some("Quillmark test".into()),
        },
    )
    .expect("stamp ok");

    // The appended update's xref table is the last standalone `\nxref\n`
    // ("startxref" never matches it). Count `<first> <count>` header lines
    // (two numeric tokens) up to the trailer; entries have three tokens.
    let table_marker = b"\nxref\n";
    let pos = result
        .windows(table_marker.len())
        .rposition(|w| w == table_marker)
        .expect("appended xref")
        + table_marker.len();
    let section_end = pos + find_sub(&result[pos..], b"trailer");
    let headers = result[pos..section_end]
        .split(|&b| b == b'\n')
        .filter(|line| {
            let toks: Vec<&[u8]> = line
                .split(|&b| b == b' ')
                .filter(|t| !t.is_empty())
                .collect();
            toks.len() == 2 && toks.iter().all(|t| t.iter().all(u8::is_ascii_digit))
        })
        .count();
    assert!(
        headers >= 2,
        "expected multiple xref subsections, found {headers}"
    );
}
