//! Append a single incremental update to a typst_pdf-produced PDF that carries
//! both overlays in one revision:
//!
//! - the `/Info` `/Producer` stamp (always — `Quillmark <version>` or a caller
//!   override), rewriting the existing `/Info` object so Typst's `/Creator` is
//!   preserved; and
//! - one Widget annotation per [`SigPlacement`], an indirect `/AcroForm`, an
//!   updated catalog and updated pages (only when `placements` is non-empty).
//!
//! Emitting both in a single incremental update means one appended xref/trailer
//! and one `/Prev` link, and `/Info`/`/ID` are forwarded exactly once.

use pdf_writer::types::{AnnotationFlags, FieldType, SigFlags};
use pdf_writer::writers::Form;
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, TextStr};

use quillmark_core::RenderError;
use typst::layout::PagedDocument;

use crate::pdf_scan::{
    append_incremental_update, assert_traditional_xref, err, extract_outer_dict, find_dict_value,
    find_object_bytes, find_startxref, find_trailer_dict, parse_indirect_ref, resolve_page_ids,
    UpdatedObject,
};

use super::SigPlacement;

const CODE_PARSE: &str = "typst::overlay_pdf_parse";

/// Default `/Producer` value: `Quillmark <crate-version>`.
pub(crate) fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
    producer: &str,
) -> Result<Vec<u8>, RenderError> {
    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;

    let trailer = find_trailer_dict(&pdf, xref_offset)?;
    if find_dict_value(trailer, "Encrypt").is_some() {
        return Err(err(
            "typst::overlay_encrypted",
            "PDF is encrypted; overlay does not handle encrypted PDFs",
        ));
    }
    let (catalog_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(CODE_PARSE, "/Root missing or malformed in trailer"))?;
    let size = find_dict_value(trailer, "Size")
        .and_then(|v| std::str::from_utf8(v.trim_ascii()).ok())
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(|| err(CODE_PARSE, "/Size missing or malformed in trailer"))?;
    let info_ref = find_dict_value(trailer, "Info").and_then(parse_indirect_ref);

    // Object ids are handed out from a single counter so the (defensive) new
    // `/Info` object can't collide with signature widget ids.
    let mut next_id = size;
    let mut objects: Vec<UpdatedObject> = Vec::new();
    let mut extra_info_ref = None;

    // ─── /Info /Producer (always) ────────────────────────────────────────────
    let literal = pdf_text_string(producer);
    match info_ref {
        Some((info_id, _)) => {
            let (s, e) = find_object_bytes(&pdf, info_id)
                .ok_or_else(|| err(CODE_PARSE, format!("/Info object {info_id} not found")))?;
            let info_dict = extract_outer_dict(&pdf[s..e])
                .ok_or_else(|| err(CODE_PARSE, "/Info dict not parseable"))?;
            objects.push(dict_object(info_id, &upsert_producer(info_dict, &literal)));
        }
        None => {
            let info_id = next_id;
            next_id += 1;
            let mut inner = b"/Producer ".to_vec();
            inner.extend_from_slice(&literal);
            objects.push(dict_object(info_id, &inner));
            extra_info_ref = Some(info_id);
        }
    }

    // ─── signature widgets + AcroForm (optional) ─────────────────────────────
    if !placements.is_empty() {
        let page_ids = resolve_page_ids(&pdf, catalog_id)?;
        let page_count = page_ids.len();
        if doc.pages.len() != page_count {
            return Err(err(
                CODE_PARSE,
                format!(
                    "page count mismatch: typst document has {} pages, PDF has {}",
                    doc.pages.len(),
                    page_count
                ),
            ));
        }
        let page_heights_pt: Vec<f32> = doc
            .pages
            .iter()
            .map(|p| p.frame.size().y.to_pt() as f32)
            .collect();

        let widget_ids: Vec<u32> = placements
            .iter()
            .map(|_| {
                let id = next_id;
                next_id += 1;
                id
            })
            .collect();
        let acroform_id = next_id;
        next_id += 1;

        let mut widgets_by_page: Vec<Vec<u32>> = vec![Vec::new(); page_count];
        for (placement, &wid) in placements.iter().zip(&widget_ids) {
            widgets_by_page[placement.page].push(wid);

            let page_h = page_heights_pt[placement.page];
            // Typst top-left → PDF bottom-left.
            let [x0, y0, x1, y1] = placement.rect_typst_pt;
            let page_ref = Ref::new(page_ids[placement.page] as i32);

            let mut chunk = Chunk::new();
            {
                let mut field = chunk.form_field(Ref::new(wid as i32));
                field
                    .field_type(FieldType::Signature)
                    .partial_name(TextStr(&placement.name));
                // `Field::into_annotation` already writes `/Subtype /Widget`;
                // calling `.subtype()` again produces a duplicate key.
                let mut ann = field.into_annotation();
                ann.rect(Rect::new(x0, page_h - y1, x1, page_h - y0))
                    .page(page_ref)
                    .flags(AnnotationFlags::PRINT);
                ann.finish();
            }
            objects.push(UpdatedObject {
                id: wid,
                bytes: chunk.as_bytes().to_vec(),
            });
        }

        let mut chunk = Chunk::new();
        {
            let mut form: Form<'_> = chunk.indirect(Ref::new(acroform_id as i32)).start::<Form>();
            form.fields(widget_ids.iter().map(|&id| Ref::new(id as i32)))
                .sig_flags(SigFlags::SIGNATURES_EXIST);
            form.pair(Name(b"NeedAppearances"), true);
            form.finish();
        }
        objects.push(UpdatedObject {
            id: acroform_id,
            bytes: chunk.as_bytes().to_vec(),
        });

        // A widget is surfaced as a fillable field only if it's reachable both
        // ways: the catalog's `/AcroForm /Fields` (the document-wide form
        // registry, added here) and the page's `/Annots` (added below). Either
        // alone leaves the field invisible or unlisted.
        let (cs, ce) = find_object_bytes(&pdf, catalog_id)
            .ok_or_else(|| err(CODE_PARSE, "catalog not found"))?;
        let cat_dict = extract_outer_dict(&pdf[cs..ce])
            .ok_or_else(|| err(CODE_PARSE, "catalog dict not parseable"))?;
        let mut cat_inner = cat_dict.to_vec();
        cat_inner.extend_from_slice(format!(" /AcroForm {acroform_id} 0 R").as_bytes());
        objects.push(dict_object(catalog_id, &cat_inner));

        // The page-side half of the above: `/Annots` is what actually paints
        // each widget on its page.
        for (page_idx, widget_refs) in widgets_by_page.iter().enumerate() {
            if widget_refs.is_empty() {
                continue;
            }
            let page_obj_id = page_ids[page_idx];
            let (s, e) = find_object_bytes(&pdf, page_obj_id)
                .ok_or_else(|| err(CODE_PARSE, format!("page node {page_obj_id} not found")))?;
            let pg_dict = extract_outer_dict(&pdf[s..e]).ok_or_else(|| {
                err(
                    CODE_PARSE,
                    format!("page node {page_obj_id} dict not parseable"),
                )
            })?;
            objects.push(dict_object(
                page_obj_id,
                &rewrite_page_with_annots(pg_dict, widget_refs)?,
            ));
        }
    }

    let new_size = next_id;
    append_incremental_update(
        pdf,
        xref_offset,
        catalog_id,
        new_size,
        extra_info_ref,
        &objects,
    )
}

/// Serialize one indirect object from its inner dict bytes:
/// `<id> 0 obj\n<< <inner> >>\nendobj\n`.
fn dict_object(id: u32, inner: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< ").into_bytes();
    bytes.extend_from_slice(inner);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}

/// Replace `/Producer`'s value if present, else append the entry. typst_pdf
/// 0.14 never emits `/Producer`, so the append branch is the live path; the
/// replace branch guards future typst versions and idempotent re-runs.
fn upsert_producer(info_dict: &[u8], literal: &[u8]) -> Vec<u8> {
    let key = b"/Producer";
    match find_dict_value(info_dict, "Producer") {
        None => {
            let mut out = info_dict.to_vec();
            out.extend_from_slice(b" /Producer ");
            out.extend_from_slice(literal);
            out
        }
        Some(value) => {
            // `value` is a subslice of `info_dict` whose start is exactly the
            // byte after the matched `/Producer` key (find_dict_value's
            // contract), so the key span is `[value_start - key.len, value_end)`.
            // Deriving it this way — rather than a naive forward search for
            // `/Producer` — avoids matching the token inside an earlier key or
            // string value.
            let value_start = value.as_ptr() as usize - info_dict.as_ptr() as usize;
            let value_end = value_start + value.len();
            let key_at = value_start - key.len();
            let mut out = Vec::new();
            out.extend_from_slice(&info_dict[..key_at]);
            out.extend_from_slice(b"/Producer ");
            out.extend_from_slice(literal);
            out.extend_from_slice(&info_dict[value_end..]);
            out
        }
    }
}

/// Encode `s` as a PDF text string. ASCII uses a literal `( … )` with `(`, `)`
/// and `\` escaped; anything else uses a UTF-16BE hex string with a BOM, the
/// portable encoding for non-Latin producer overrides.
fn pdf_text_string(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        let mut out = Vec::with_capacity(s.len() + 2);
        out.push(b'(');
        for &b in s.as_bytes() {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b')');
        out
    } else {
        let mut out = Vec::new();
        out.push(b'<');
        out.extend_from_slice(b"FEFF");
        for unit in s.encode_utf16() {
            out.extend_from_slice(format!("{unit:04X}").as_bytes());
        }
        out.push(b'>');
        out
    }
}

/// Three cases for the existing `/Annots`: absent (write a fresh array);
/// inline array (splice widget refs before `]`); indirect reference (hard
/// error — typst-pdf 0.14 doesn't emit this shape).
fn rewrite_page_with_annots(pg_dict: &[u8], widget_refs: &[u32]) -> Result<Vec<u8>, RenderError> {
    let widgets_str = widget_refs
        .iter()
        .map(|r| format!("{r} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");

    match find_dict_value(pg_dict, "Annots") {
        None => {
            let mut out = pg_dict.to_vec();
            out.extend_from_slice(format!(" /Annots [{widgets_str}]").as_bytes());
            Ok(out)
        }
        Some(existing) => {
            let trimmed = existing.trim_ascii();
            if trimmed.starts_with(b"[") {
                let end = trimmed
                    .iter()
                    .rposition(|&b| b == b']')
                    .ok_or_else(|| err(CODE_PARSE, "/Annots array missing ]"))?;
                let inner = &trimmed[1..end];
                let merged = format!(
                    "[{} {}]",
                    String::from_utf8_lossy(inner).trim(),
                    widgets_str
                );
                // `existing` is find_dict_value's slice: its start is the byte
                // after `/Annots`, and its length spans the whole value. So the
                // key+value occupies `[value_start - key.len, value_end)` — no
                // re-scan needed, and no risk of matching `/Annots` elsewhere.
                let key = b"/Annots";
                let value_start = existing.as_ptr() as usize - pg_dict.as_ptr() as usize;
                let key_at = value_start - key.len();
                let value_end = value_start + existing.len();
                let mut out = Vec::new();
                out.extend_from_slice(&pg_dict[..key_at]);
                out.extend_from_slice(b"/Annots ");
                out.extend_from_slice(merged.as_bytes());
                out.extend_from_slice(&pg_dict[value_end..]);
                Ok(out)
            } else if parse_indirect_ref(existing).is_some() {
                Err(err(
                    "typst::overlay_indirect_annots",
                    "/Annots is an indirect reference; only inline arrays are supported \
                     (typst-pdf 0.14 emits inline)",
                ))
            } else {
                Err(err(CODE_PARSE, "/Annots is neither array nor indirect ref"))
            }
        }
    }
}
