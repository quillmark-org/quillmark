//! Append an incremental update to a typst_pdf-produced PDF: one Widget
//! annotation per `SigPlacement`, one indirect `/AcroForm`, an updated
//! catalog and updated pages (with widget refs appended to `/Annots`).
//! Returns the original bytes unchanged when `placements` is empty.

use pdf_writer::types::{FieldType, SigFlags};
use pdf_writer::writers::Form;
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, TextStr};

use quillmark_core::RenderError;
use typst::layout::PagedDocument;

use super::err;
use super::scanner::{
    assert_traditional_xref, extract_outer_dict, find_dict_value, find_object_bytes,
    find_startxref, parse_indirect_ref, parse_traditional_trailer, resolve_page_ids,
};
use super::SigPlacement;

const CODE_PARSE: &str = "typst::sig_overlay_pdf_parse";

pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
) -> Result<Vec<u8>, RenderError> {
    if placements.is_empty() {
        return Ok(pdf);
    }

    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;
    let (catalog_id, size, encrypted) = parse_traditional_trailer(&pdf, xref_offset)?;
    if encrypted {
        return Err(err(
            "typst::sig_overlay_encrypted",
            "PDF is encrypted; signature inject does not handle encrypted PDFs",
        ));
    }
    let next_id = size;

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

    let widget_ids: Vec<Ref> = (0..placements.len())
        .map(|i| Ref::new((next_id + i as u32) as i32))
        .collect();
    let acroform_id = Ref::new((next_id + placements.len() as u32) as i32);

    let mut widgets_by_page: Vec<Vec<Ref>> = vec![Vec::new(); page_count];
    for (p, wref) in placements.iter().zip(&widget_ids) {
        widgets_by_page[p.page].push(*wref);
    }

    let mut chunk = Chunk::new();
    for (placement, wref) in placements.iter().zip(&widget_ids) {
        let page_h = page_heights_pt[placement.page];
        // Typst top-left → PDF bottom-left.
        let [x0, y0, x1, y1] = placement.rect_typst_pt;
        let page_ref = Ref::new(page_ids[placement.page] as i32);
        let mut field = chunk.form_field(*wref);
        field
            .field_type(FieldType::Signature)
            .partial_name(TextStr(&placement.name));
        // `Field::into_annotation` already writes `/Subtype /Widget`; calling
        // `.subtype()` again produces a duplicate `/Subtype` key (malformed).
        let mut ann = field.into_annotation();
        ann.rect(Rect::new(x0, page_h - y1, x1, page_h - y0))
            .page(page_ref);
        ann.finish();
    }
    {
        let mut form: Form<'_> = chunk.indirect(acroform_id).start::<Form>();
        form.fields(widget_ids.iter().copied())
            .sig_flags(SigFlags::SIGNATURES_EXIST | SigFlags::APPEND_ONLY);
        form.pair(Name(b"NeedAppearances"), true);
        form.finish();
    }

    let chunk_bytes = chunk.as_bytes();
    let offset_in_chunk = |id: i32| -> Result<usize, RenderError> {
        let marker = format!("{} 0 obj", id);
        chunk_bytes
            .windows(marker.len())
            .position(|w| w == marker.as_bytes())
            .ok_or_else(|| err(CODE_PARSE, format!("emitted object {id} not located in chunk")))
    };

    let (cs, ce) =
        find_object_bytes(&pdf, catalog_id).ok_or_else(|| err(CODE_PARSE, "catalog not found"))?;
    let cat_dict = extract_outer_dict(&pdf[cs..ce])
        .ok_or_else(|| err(CODE_PARSE, "catalog dict not parseable"))?;
    let mut updated_catalog: Vec<u8> = Vec::new();
    updated_catalog.extend_from_slice(format!("{} 0 obj\n<< ", catalog_id).as_bytes());
    updated_catalog.extend_from_slice(cat_dict);
    updated_catalog.extend_from_slice(
        format!(" /AcroForm {} 0 R >>\nendobj\n", acroform_id.get()).as_bytes(),
    );

    let mut updated_pages: Vec<(u32, Vec<u8>)> = Vec::new();
    for (page_idx, widget_refs) in widgets_by_page.iter().enumerate() {
        if widget_refs.is_empty() {
            continue;
        }
        let page_obj_id = page_ids[page_idx];
        let (s, e) = find_object_bytes(&pdf, page_obj_id)
            .ok_or_else(|| err(CODE_PARSE, format!("page node {page_obj_id} not found")))?;
        let pg_dict = extract_outer_dict(&pdf[s..e]).ok_or_else(|| {
            err(CODE_PARSE, format!("page node {page_obj_id} dict not parseable"))
        })?;
        let updated = rewrite_page_with_annots(pg_dict, widget_refs)?;
        let mut buf = Vec::new();
        buf.extend_from_slice(format!("{} 0 obj\n<< ", page_obj_id).as_bytes());
        buf.extend_from_slice(&updated);
        buf.extend_from_slice(b" >>\nendobj\n");
        updated_pages.push((page_obj_id, buf));
    }

    let mut out = pdf;
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let widget_chunk_off = out.len();
    out.extend_from_slice(chunk_bytes);

    let mut entries: Vec<(u32, usize)> = Vec::new();
    for wref in &widget_ids {
        entries.push((wref.get() as u32, widget_chunk_off + offset_in_chunk(wref.get())?));
    }
    entries.push((
        acroform_id.get() as u32,
        widget_chunk_off + offset_in_chunk(acroform_id.get())?,
    ));
    let new_catalog_off = out.len();
    out.extend_from_slice(&updated_catalog);
    entries.push((catalog_id, new_catalog_off));
    for (page_obj_id, buf) in &updated_pages {
        let off = out.len();
        out.extend_from_slice(buf);
        entries.push((*page_obj_id, off));
    }

    let new_xref_off = out.len();
    entries.sort_by_key(|(id, _)| *id);
    out.extend_from_slice(b"xref\n");
    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        out.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
        for &(_, off) in &entries[i..=j] {
            out.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
        }
        i = j + 1;
    }

    let new_size = next_id + placements.len() as u32 + 1;
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {new_size} /Root {catalog_id} 0 R /Prev {xref_offset} >>\nstartxref\n{new_xref_off}\n%%EOF\n"
        )
        .as_bytes(),
    );
    Ok(out)
}

/// Three cases for the existing `/Annots`: absent (write a fresh array);
/// inline array (splice widget refs before `]`); indirect reference (hard
/// error — typst-pdf 0.14 doesn't emit this shape).
fn rewrite_page_with_annots(pg_dict: &[u8], widget_refs: &[Ref]) -> Result<Vec<u8>, RenderError> {
    let widgets_str = widget_refs
        .iter()
        .map(|r| format!("{} 0 R", r.get()))
        .collect::<Vec<_>>()
        .join(" ");

    match find_dict_value(pg_dict, "Annots") {
        None => {
            let mut out = pg_dict.to_vec();
            out.extend_from_slice(format!(" /Annots [{}]", widgets_str).as_bytes());
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
                let merged =
                    format!("[{} {}]", String::from_utf8_lossy(inner).trim(), widgets_str);
                let key = b"/Annots";
                let key_at = pg_dict
                    .windows(key.len())
                    .position(|w| w == key)
                    .ok_or_else(|| err(CODE_PARSE, "/Annots key relocated mid-rewrite"))?;
                let value_start = key_at + key.len();
                let value_end = find_value_end(pg_dict, value_start)
                    .ok_or_else(|| err(CODE_PARSE, "/Annots value end not found"))?;
                let mut out = Vec::new();
                out.extend_from_slice(&pg_dict[..key_at]);
                out.extend_from_slice(b"/Annots ");
                out.extend_from_slice(merged.as_bytes());
                out.extend_from_slice(&pg_dict[value_end..]);
                Ok(out)
            } else if parse_indirect_ref(existing).is_some() {
                Err(err(
                    "typst::sig_overlay_indirect_annots",
                    "/Annots is an indirect reference; only inline arrays are supported \
                     (typst-pdf 0.14 emits inline)",
                ))
            } else {
                Err(err(CODE_PARSE, "/Annots is neither array nor indirect ref"))
            }
        }
    }
}

fn find_value_end(dict: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    let mut depth_dict = 0i32;
    let mut depth_array = 0i32;
    while i < dict.len() {
        if dict[i..].starts_with(b"<<") {
            depth_dict += 1;
            i += 2;
            continue;
        }
        if dict[i..].starts_with(b">>") {
            if depth_dict == 0 {
                return Some(i);
            }
            depth_dict -= 1;
            i += 2;
            continue;
        }
        match dict[i] {
            b'[' => {
                depth_array += 1;
                i += 1;
            }
            b']' => {
                depth_array -= 1;
                i += 1;
            }
            b'/' if depth_dict == 0 && depth_array == 0 && i > start => return Some(i),
            _ => i += 1,
        }
    }
    Some(i)
}
