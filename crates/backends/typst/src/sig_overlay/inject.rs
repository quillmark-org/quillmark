//! Append an incremental update to a typst_pdf-produced PDF that adds one
//! SigField widget per `SigPlacement`, an `/AcroForm` indirect object with
//! `/SigFlags 3` and `/NeedAppearances true`, an `/AcroForm` reference on
//! the existing catalog, and the widget refs appended to each page's
//! existing `/Annots` (or `/Annots` added if absent) — all in a single
//! traditional `xref` section + trailer.
//!
//! Returns the original bytes unchanged when `placements` is empty.

use pdf_writer::types::{AnnotationFlags, AnnotationType, FieldType, SigFlags};
use pdf_writer::writers::Form;
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, TextStr};

use typst::layout::PagedDocument;

use super::scanner::{
    assert_traditional_xref, extract_outer_dict, find_dict_value, find_object_bytes,
    find_startxref, parse_indirect_ref, parse_traditional_trailer, resolve_page_ids,
};
use super::{SigOverlayError, SigPlacement};

pub(crate) fn inject(
    pdf: Vec<u8>,
    doc: &PagedDocument,
    placements: &[SigPlacement],
) -> Result<Vec<u8>, SigOverlayError> {
    if placements.is_empty() {
        return Ok(pdf);
    }

    // ── scan ──
    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;
    let (catalog_id, size, encrypted) = parse_traditional_trailer(&pdf, xref_offset)?;
    if encrypted {
        return Err(SigOverlayError::EncryptedPdfUnsupported);
    }
    let next_id = size;

    let page_ids = resolve_page_ids(&pdf, catalog_id)?;
    if page_ids.is_empty() {
        return Err(SigOverlayError::NoPages);
    }
    let page_count = page_ids.len();

    // Page heights from the in-memory Typst document (for Y-flip).
    if doc.pages.len() != page_count {
        return Err(SigOverlayError::PageCountMismatch {
            typst: doc.pages.len(),
            pdf: page_count,
        });
    }
    let page_heights_pt: Vec<f32> = doc
        .pages
        .iter()
        .map(|p| p.frame.size().y.to_pt() as f32)
        .collect();

    // ── allocate IDs ──
    // widget IDs first, then one acroform ID after.
    let widget_ids: Vec<Ref> = placements
        .iter()
        .enumerate()
        .map(|(i, _)| Ref::new((next_id + i as u32) as i32))
        .collect();
    let acroform_id = Ref::new((next_id + placements.len() as u32) as i32);

    // Group placements by page index → list of widget Refs on that page.
    let mut widgets_by_page: Vec<Vec<Ref>> = vec![Vec::new(); page_count];
    for (p, wref) in placements.iter().zip(&widget_ids) {
        if p.page >= page_count {
            return Err(SigOverlayError::PagePlacementOutOfRange {
                page: p.page,
                page_count,
            });
        }
        widgets_by_page[p.page].push(*wref);
    }

    // ── build widgets + acroform via pdf-writer Chunk ──
    let mut chunk = Chunk::new();
    for (placement, wref) in placements.iter().zip(&widget_ids) {
        let page_h = page_heights_pt[placement.page];
        // Convert [x0_t, y0_t, x1_t, y1_t] (Typst top-left) → [llx, lly, urx, ury] (PDF bottom-left).
        let [x0, y0, x1, y1] = placement.rect_typst_pt;
        let llx = x0;
        let lly = page_h - y1;
        let urx = x1;
        let ury = page_h - y0;
        let page_ref = Ref::new(page_ids[placement.page] as i32);
        let mut field = chunk.form_field(*wref);
        field
            .field_type(FieldType::Signature)
            .partial_name(TextStr(&placement.name));
        let mut ann = field.into_annotation();
        ann.subtype(AnnotationType::Widget)
            .rect(Rect::new(llx, lly, urx, ury))
            .page(page_ref)
            .flags(AnnotationFlags::PRINT);
        ann.finish();
    }
    {
        let mut form: Form<'_> = chunk.indirect(acroform_id).start::<Form>();
        form.fields(widget_ids.iter().copied())
            .sig_flags(SigFlags::SIGNATURES_EXIST | SigFlags::APPEND_ONLY);
        form.pair(Name(b"NeedAppearances"), true);
        form.finish();
    }

    // Locate each emitted object's offset within the chunk bytes so we can
    // record absolute offsets in the new xref subsection.
    let chunk_bytes = chunk.as_bytes();
    fn offset_in_chunk(chunk_bytes: &[u8], id: i32) -> Result<usize, SigOverlayError> {
        let marker = format!("{} 0 obj", id);
        chunk_bytes
            .windows(marker.len())
            .position(|w| w == marker.as_bytes())
            .ok_or(SigOverlayError::PdfWriterChunkScan { id: id as u32 })
    }

    // ── splice updated catalog (existing keys + /AcroForm ref) ──
    let (cs, ce) = find_object_bytes(&pdf, catalog_id).ok_or(SigOverlayError::MissingCatalog)?;
    let cat_dict = extract_outer_dict(&pdf[cs..ce]).ok_or(SigOverlayError::MissingCatalog)?;
    let mut updated_catalog: Vec<u8> = Vec::new();
    updated_catalog.extend_from_slice(format!("{} 0 obj\n<< ", catalog_id).as_bytes());
    if find_dict_value(cat_dict, "AcroForm").is_some() {
        // Catalog already declares an /AcroForm — bail rather than risk a
        // duplicate key or silently dropping signatures we don't own.
        return Err(SigOverlayError::PreExistingAcroForm);
    }
    updated_catalog.extend_from_slice(cat_dict);
    updated_catalog.extend_from_slice(
        format!(" /AcroForm {} 0 R >>\nendobj\n", acroform_id.get()).as_bytes(),
    );

    // ── splice updated page dicts (one per page that received a widget) ──
    let mut updated_pages: Vec<(u32, Vec<u8>)> = Vec::new();
    for (page_idx, widget_refs) in widgets_by_page.iter().enumerate() {
        if widget_refs.is_empty() {
            continue;
        }
        let page_obj_id = page_ids[page_idx];
        let (s, e) = find_object_bytes(&pdf, page_obj_id).ok_or(
            SigOverlayError::MissingPageNode { id: page_obj_id },
        )?;
        let pg_dict =
            extract_outer_dict(&pdf[s..e]).ok_or(SigOverlayError::MissingPageNode {
                id: page_obj_id,
            })?;

        let updated = rewrite_page_with_annots(pg_dict, widget_refs)?;
        let mut buf = Vec::new();
        buf.extend_from_slice(format!("{} 0 obj\n<< ", page_obj_id).as_bytes());
        buf.extend_from_slice(&updated);
        buf.extend_from_slice(b" >>\nendobj\n");
        updated_pages.push((page_obj_id, buf));
    }

    // ── assemble incremental update bytes ──
    let mut out = pdf;
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    let widget_chunk_off = out.len();
    out.extend_from_slice(chunk_bytes);

    let mut entries: Vec<(u32, usize)> = Vec::new();
    for wref in &widget_ids {
        entries.push((
            wref.get() as u32,
            widget_chunk_off + offset_in_chunk(chunk_bytes, wref.get())?,
        ));
    }
    entries.push((
        acroform_id.get() as u32,
        widget_chunk_off + offset_in_chunk(chunk_bytes, acroform_id.get())?,
    ));

    let new_catalog_off = out.len();
    out.extend_from_slice(&updated_catalog);
    entries.push((catalog_id, new_catalog_off));

    for (page_obj_id, buf) in &updated_pages {
        let off = out.len();
        out.extend_from_slice(buf);
        entries.push((*page_obj_id, off));
    }

    // ── xref subsection ──
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

    // ── trailer ──
    let new_size = next_id + placements.len() as u32 + 1; // +1 acroform
    out.extend_from_slice(b"trailer\n<< ");
    out.extend_from_slice(format!("/Size {} ", new_size).as_bytes());
    out.extend_from_slice(format!("/Root {} 0 R ", catalog_id).as_bytes());
    out.extend_from_slice(format!("/Prev {} ", xref_offset).as_bytes());
    out.extend_from_slice(b">>\n");
    out.extend_from_slice(b"startxref\n");
    out.extend_from_slice(format!("{}\n", new_xref_off).as_bytes());
    out.extend_from_slice(b"%%EOF\n");

    Ok(out)
}

/// Rewrite a page dict so it contains `/Annots [<existing entries> <widget refs>]`.
///
/// Handles three cases for the existing `/Annots`:
/// - Absent: append a fresh `/Annots [widget_refs...]`.
/// - Inline array `[N G R ...]`: splice widget refs before `]`.
/// - Indirect reference `N G R`: per probe findings, typst-pdf 0.14 does not
///   emit this shape; we hard-error rather than guess at the resolution.
fn rewrite_page_with_annots(
    pg_dict: &[u8],
    widget_refs: &[Ref],
) -> Result<Vec<u8>, SigOverlayError> {
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
                    .ok_or(SigOverlayError::MalformedAnnotsArray)?;
                let inner = &trimmed[1..end];
                let merged = format!("[{} {}]", String::from_utf8_lossy(inner).trim(), widgets_str);
                let key = b"/Annots";
                let key_at = pg_dict
                    .windows(key.len())
                    .position(|w| w == key)
                    .ok_or(SigOverlayError::MalformedAnnotsArray)?;
                let value_start = key_at + key.len();
                let value_end = find_value_end(pg_dict, value_start)
                    .ok_or(SigOverlayError::MalformedAnnotsArray)?;
                let mut out = Vec::new();
                out.extend_from_slice(&pg_dict[..key_at]);
                out.extend_from_slice(b"/Annots ");
                out.extend_from_slice(merged.as_bytes());
                out.extend_from_slice(&pg_dict[value_end..]);
                Ok(out)
            } else if parse_indirect_ref(existing).is_some() {
                Err(SigOverlayError::IndirectAnnotsUnsupported)
            } else {
                Err(SigOverlayError::MalformedAnnotsArray)
            }
        }
    }
}

/// Find the end byte of the value that begins at `start` inside a dict body.
/// Tracks dict and array nesting depths; terminates at the next top-level `/`
/// or at the end of the slice.
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
