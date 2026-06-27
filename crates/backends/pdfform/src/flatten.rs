//! Value-flatten path for the pdfform backend.
//!
//! Draws field values as PDF content stream operators instead of AcroForm
//! widgets. The result is visible in ALL viewers including non-interactive
//! rasterizers such as headless Chromium / pdfium or Ghostscript — unlike
//! Technique A (AcroForm with `/NeedAppearances`), which requires an
//! interactive viewer to synthesize appearances.
//!
//! Entry point: [`flatten`].

use quillmark_pdf::{
    reader::{
        append_incremental_update, assert_traditional_xref, err, extract_outer_dict,
        find_dict_value, find_object_bytes, find_startxref, find_trailer_dict, parse_indirect_ref,
        resolve_page_ids, UpdatedObject,
    },
    regions_of, FieldSpec, FieldType, PdfError, StampOptions, StampResult, CHECKBOX_ON_STATE,
};

const CODE_PARSE: &str = "pdf::flatten_parse";

/// Flatten `fields` onto `base` PDF by drawing values as PDF content stream
/// operators. Unlike the stamp/Technique-A path, the result is visible in all
/// rasterizers. Returns the flat PDF bytes and the same [`RenderedRegion`]
/// sidecar the stamp path produces.
pub fn flatten(
    base: Vec<u8>,
    fields: &[FieldSpec],
    opts: &StampOptions,
) -> Result<StampResult, PdfError> {
    let regions = regions_of(fields);

    if fields.is_empty() && opts.producer.is_none() {
        return Ok(StampResult { pdf: base, regions });
    }

    let pdf = base;
    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;
    let trailer = find_trailer_dict(&pdf, xref_offset)?;

    if find_dict_value(trailer, "Encrypt").is_some() {
        return Err(err(
            "pdf::encrypted",
            "PDF is encrypted; the flatten path does not handle encrypted PDFs",
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

    let mut next_id = size;
    let mut objects: Vec<UpdatedObject> = Vec::new();
    let mut extra_info_ref: Option<u32> = None;

    // ─── /Info /Producer (when requested) ────────────────────────────────────
    if let Some(producer) = opts.producer.as_deref() {
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
                let info_id = alloc_id(&mut next_id)?;
                let mut inner = b"/Producer ".to_vec();
                inner.extend_from_slice(&literal);
                objects.push(dict_object(info_id, &inner));
                extra_info_ref = Some(info_id);
            }
        }
    }

    if fields.is_empty() {
        let new_size = next_id;
        let flat = append_incremental_update(
            pdf,
            xref_offset,
            catalog_id,
            new_size,
            extra_info_ref,
            &objects,
        )?;
        return Ok(StampResult { pdf: flat, regions });
    }

    let page_ids = resolve_page_ids(&pdf, catalog_id)?;
    let page_count = page_ids.len();

    for spec in fields {
        if spec.page >= page_count {
            return Err(err(
                CODE_PARSE,
                format!(
                    "field {:?} targets page {} but the PDF has {page_count} page(s)",
                    spec.name, spec.page
                ),
            ));
        }
    }

    // Standard Type1 fonts: Helvetica for text/choice, ZapfDingbats for checkboxes.
    // Both are among the 14 standard PDF fonts every conforming reader provides.
    let helv_id = alloc_id(&mut next_id)?;
    let zadb_id = alloc_id(&mut next_id)?;
    objects.push(type1_font_object(helv_id, "Helvetica"));
    objects.push(type1_font_object(zadb_id, "ZapfDingbats"));

    // Group fields by page.
    let mut fields_by_page: Vec<Vec<&FieldSpec>> = vec![Vec::new(); page_count];
    for spec in fields {
        fields_by_page[spec.page].push(spec);
    }

    // For each page with drawable fields: emit a content stream and rewrite the page dict.
    for (page_idx, page_fields) in fields_by_page.iter().enumerate() {
        let drawable: Vec<&FieldSpec> = page_fields
            .iter()
            .copied()
            .filter(|s| has_drawable_value(s))
            .collect();
        if drawable.is_empty() {
            continue;
        }

        let stream_id = alloc_id(&mut next_id)?;
        objects.push(content_stream_object(
            stream_id,
            &build_content_stream(&drawable),
        ));

        let page_obj_id = page_ids[page_idx];
        let (s, e) = find_object_bytes(&pdf, page_obj_id)
            .ok_or_else(|| err(CODE_PARSE, format!("page object {page_obj_id} not found")))?;
        let pg_dict = extract_outer_dict(&pdf[s..e])
            .ok_or_else(|| err(CODE_PARSE, "page dict not parseable"))?;

        let new_pg = rewrite_page_for_flatten(pg_dict, helv_id, zadb_id, stream_id)?;
        objects.push(dict_object(page_obj_id, &new_pg));
    }

    let new_size = next_id;
    let flat = append_incremental_update(
        pdf,
        xref_offset,
        catalog_id,
        new_size,
        extra_info_ref,
        &objects,
    )?;
    Ok(StampResult { pdf: flat, regions })
}

// ── Drawing helpers ───────────────────────────────────────────────────────────

/// Whether a field has a value we render visually on the flat path.
fn has_drawable_value(spec: &FieldSpec) -> bool {
    match &spec.field_type {
        FieldType::Signature => false,
        FieldType::Checkbox => spec.value.as_deref() == Some(CHECKBOX_ON_STATE),
        _ => spec.value.is_some(),
    }
}

/// Build a PDF content stream drawing all `fields` for one page.
fn build_content_stream(fields: &[&FieldSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    for spec in fields {
        let [x0, y0, x1, y1] = spec.rect;
        let w = x1 - x0;
        let h = y1 - y0;
        match &spec.field_type {
            FieldType::Signature => {}
            FieldType::Checkbox => {
                // Glyph 0x34 ("4") in ZapfDingbats is the filled check mark — the
                // same glyph the AcroForm stamp path declares via /MK /CA (4).
                let size = (h * 0.75).clamp(4.0, 12.0);
                // ZapfDingbats check glyphs are roughly square; centre in the box.
                let x_pos = x0 + (w - size * 0.6) * 0.5;
                let y_pos = y0 + (h - size) * 0.5;
                write_zadb_char(&mut out, b'4', x_pos, y_pos, size);
            }
            FieldType::Text { .. } => {
                if let Some(value) = &spec.value {
                    let size = (h * 0.65).clamp(4.0, 12.0);
                    let x_pos = x0 + 2.0;
                    // First baseline just inside the top edge.
                    let y_top = y1 - size - 1.0;
                    let lines: Vec<&str> = value.lines().collect();
                    write_text_block(&mut out, &lines, x_pos, y_top, size);
                }
            }
            FieldType::Choice { .. } => {
                if let Some(value) = &spec.value {
                    let size = (h * 0.65).clamp(4.0, 12.0);
                    let x_pos = x0 + 2.0;
                    let y_pos = y0 + (h - size) * 0.5;
                    write_text_block(&mut out, &[value.as_str()], x_pos, y_pos, size);
                }
            }
        }
    }
    out
}

/// Write a `q/Q`-wrapped `BT/ET` block for one or more lines of text using `/Helv`.
fn write_text_block(out: &mut Vec<u8>, lines: &[&str], x: f32, y: f32, size: f32) {
    if lines.is_empty() {
        return;
    }
    let line_h = size * 1.2;
    out.extend_from_slice(b"q\nBT\n/Helv ");
    push_f32(out, size);
    out.extend_from_slice(b" Tf\n");
    push_f32(out, x);
    out.push(b' ');
    push_f32(out, y);
    out.extend_from_slice(b" Td\n");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b"0 ");
            push_f32(out, -line_h);
            out.extend_from_slice(b" Td\n");
        }
        out.push(b'(');
        pdf_escape(out, line.as_bytes());
        out.extend_from_slice(b") Tj\n");
    }
    out.extend_from_slice(b"ET\nQ\n");
}

/// Write a single ZapfDingbats character (as its raw byte code) in a `BT/ET` block.
/// Glyph 0x34 (`'4'`) is the filled check mark — identical to the `/MK /CA (4)`
/// glyph the AcroForm stamp path declares for checked checkboxes.
fn write_zadb_char(out: &mut Vec<u8>, glyph: u8, x: f32, y: f32, size: f32) {
    out.extend_from_slice(b"q\nBT\n/ZaDb ");
    push_f32(out, size);
    out.extend_from_slice(b" Tf\n");
    push_f32(out, x);
    out.push(b' ');
    push_f32(out, y);
    out.extend_from_slice(b" Td\n(");
    if matches!(glyph, b'(' | b')' | b'\\') {
        out.push(b'\\');
    }
    out.push(glyph);
    out.extend_from_slice(b") Tj\nET\nQ\n");
}

// ── Page dict rewriting ───────────────────────────────────────────────────────

/// Rewrite the page's inner dict bytes to (a) add `/Helv` and `/ZaDb` to
/// `/Resources /Font` and (b) append `stream_id` to `/Contents`.
fn rewrite_page_for_flatten(
    pg_dict: &[u8],
    helv_id: u32,
    zadb_id: u32,
    stream_id: u32,
) -> Result<Vec<u8>, PdfError> {
    let with_stream = add_content_stream(pg_dict, stream_id)?;
    let with_helv = add_font_resource(&with_stream, "Helv", helv_id)?;
    add_font_resource(&with_helv, "ZaDb", zadb_id)
}

/// Append `stream_id` to the page's `/Contents` (wrapping a bare ref in an array
/// as needed, or creating `/Contents` when absent).
fn add_content_stream(pg_dict: &[u8], stream_id: u32) -> Result<Vec<u8>, PdfError> {
    let ref_str = format!("{stream_id} 0 R");
    match find_dict_value(pg_dict, "Contents") {
        None => {
            let mut out = pg_dict.to_vec();
            out.extend_from_slice(format!(" /Contents [{ref_str}]").as_bytes());
            Ok(out)
        }
        Some(existing) => {
            let trimmed = existing.trim_ascii();
            let new_val = if trimmed.starts_with(b"[") {
                let end = trimmed
                    .iter()
                    .rposition(|&b| b == b']')
                    .ok_or_else(|| err(CODE_PARSE, "/Contents array missing ]"))?;
                let inner = String::from_utf8_lossy(&trimmed[1..end]);
                format!("[{} {ref_str}]", inner.trim())
            } else {
                // Single indirect ref — wrap in array.
                format!("[{} {ref_str}]", String::from_utf8_lossy(trimmed).trim())
            };
            let key = b"/Contents";
            let value_start = existing.as_ptr() as usize - pg_dict.as_ptr() as usize;
            let key_at = value_start - key.len();
            let value_end = value_start + existing.len();
            let mut out = Vec::new();
            out.extend_from_slice(&pg_dict[..key_at]);
            out.extend_from_slice(b"/Contents ");
            out.extend_from_slice(new_val.as_bytes());
            out.extend_from_slice(&pg_dict[value_end..]);
            Ok(out)
        }
    }
}

/// Inject `/<name> <font_id> 0 R` into the page's `/Resources /Font` dict,
/// creating intermediate dicts as needed.
///
/// When `/Resources` is an indirect reference (uncommon in our fixture
/// contract), font injection is skipped — the 14 standard PDF Type1 fonts
/// are available to conforming readers without explicit resource declaration.
fn add_font_resource(pg_dict: &[u8], name: &str, font_id: u32) -> Result<Vec<u8>, PdfError> {
    let helv_entry = format!("/{name} {font_id} 0 R");

    match find_dict_value(pg_dict, "Resources") {
        None => {
            let mut out = pg_dict.to_vec();
            out.extend_from_slice(format!(" /Resources << /Font << {helv_entry} >> >>").as_bytes());
            Ok(out)
        }
        Some(res_val) => {
            if !res_val.trim_ascii().starts_with(b"<<") {
                // Indirect resources ref — skip injection, rely on standard fonts.
                return Ok(pg_dict.to_vec());
            }
            let res_inner = extract_outer_dict(res_val)
                .ok_or_else(|| err(CODE_PARSE, "page /Resources dict not parseable"))?;

            // Build new_res_inner with /Helv injected into /Font.
            let new_res_inner: Vec<u8> = match find_dict_value(res_inner, "Font") {
                None => {
                    let mut out = res_inner.to_vec();
                    out.extend_from_slice(format!(" /Font << {helv_entry} >>").as_bytes());
                    out
                }
                Some(font_val) => {
                    if !font_val.trim_ascii().starts_with(b"<<") {
                        // Indirect font resources ref — rely on standard fonts.
                        return Ok(pg_dict.to_vec());
                    }
                    let font_inner = extract_outer_dict(font_val).ok_or_else(|| {
                        err(CODE_PARSE, "page /Resources /Font dict not parseable")
                    })?;
                    // Build new font dict value.
                    let mut new_font_val = b"<< ".to_vec();
                    new_font_val.extend_from_slice(font_inner);
                    new_font_val.extend_from_slice(format!(" {helv_entry} >>").as_bytes());

                    // Splice new_font_val into res_inner replacing /Font <font_val>.
                    let fv_start = font_val.as_ptr() as usize - res_inner.as_ptr() as usize;
                    let key_at = fv_start - b"/Font".len();
                    let fv_end = fv_start + font_val.len();
                    let mut out = Vec::new();
                    out.extend_from_slice(&res_inner[..key_at]);
                    out.extend_from_slice(b"/Font ");
                    out.extend_from_slice(&new_font_val);
                    out.extend_from_slice(&res_inner[fv_end..]);
                    out
                }
            };

            // Build new_res_val and splice it into pg_dict replacing /Resources <res_val>.
            let mut new_res_val = b"<< ".to_vec();
            new_res_val.extend_from_slice(&new_res_inner);
            new_res_val.extend_from_slice(b" >>");

            let rv_start = res_val.as_ptr() as usize - pg_dict.as_ptr() as usize;
            let key_at = rv_start - b"/Resources".len();
            let rv_end = rv_start + res_val.len();

            let mut out = Vec::new();
            out.extend_from_slice(&pg_dict[..key_at]);
            out.extend_from_slice(b"/Resources ");
            out.extend_from_slice(&new_res_val);
            out.extend_from_slice(&pg_dict[rv_end..]);
            Ok(out)
        }
    }
}

// ── PDF object builders ───────────────────────────────────────────────────────

fn type1_font_object(id: u32, base_font: &str) -> UpdatedObject {
    UpdatedObject {
        id,
        bytes: format!(
            "{id} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} >>\nendobj\n"
        )
        .into_bytes(),
    }
}

fn content_stream_object(id: u32, content: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< /Length {} >>\nstream\n", content.len()).into_bytes();
    bytes.extend_from_slice(content);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    UpdatedObject { id, bytes }
}

fn dict_object(id: u32, inner: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< ").into_bytes();
    bytes.extend_from_slice(inner);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}

fn alloc_id(next: &mut u32) -> Result<u32, PdfError> {
    let id = *next;
    *next = id.checked_add(1).ok_or_else(|| {
        err(
            CODE_PARSE,
            "PDF object id space exhausted (/Size too large)",
        )
    })?;
    Ok(id)
}

// ── PDF text helpers ──────────────────────────────────────────────────────────

/// Append `v` as a compact `%.2f` float, stripping trailing zeros and dot.
fn push_f32(out: &mut Vec<u8>, v: f32) {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(s.as_bytes());
}

/// Escape bytes for a PDF literal string `( … )`: `(`, `)`, `\` → `\x`.
fn pdf_escape(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
}

/// Encode `s` as a PDF text string: ASCII → literal `(…)`, else UTF-16BE `<FEFF…>`.
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
