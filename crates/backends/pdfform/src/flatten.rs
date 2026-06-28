//! Value-flatten path for the pdfform backend.
//!
//! Draws field values as PDF content stream operators instead of AcroForm
//! widgets. The result is visible in ALL viewers including non-interactive
//! rasterizers such as headless Chromium / pdfium or Ghostscript — unlike
//! Technique A (AcroForm with `/NeedAppearances`), which requires an
//! interactive viewer to synthesize appearances.
//!
//! Because the values are drawn directly (no viewer appearance synthesis), this
//! path commits to a byte encoding: text is transcoded to WinAnsi
//! ([`winansi_encode`](quillmark_pdf::writer::winansi_encode)) and shown with a
//! `WinAnsiEncoding` Helvetica, and each value is clipped to its field box.
//!
//! The drawing stream is appended last to the page `/Contents` and positions
//! text with absolute `Td` coordinates, i.e. it assumes the **identity CTM** of
//! the page default user space. A well-formed background restores its graphics
//! state (balanced `q`/`Q`, no dangling `cm`), which the qualifier-produced and
//! Typst-rendered bases this consumes always do; a base that left a non-identity
//! CTM in effect would shift the flattened values. This path is preview-only, so
//! the impact is confined to the raster preview, never a shipped deliverable.
//!
//! Entry point: [`flatten`].

use quillmark_pdf::{
    reader::{err, extract_outer_dict, find_dict_value, find_object_bytes, UpdatedObject},
    regions_of,
    writer::{alloc_id, dict_object, pdf_escape, winansi_encode},
    FieldSpec, FieldType, PdfError, PdfUpdate, StampOptions, StampResult, CHECKBOX_ON_STATE,
};

use crate::typography;

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
    // Shared envelope: validate the input contract, read the trailer, seed the
    // id counter, apply the optional `/Info` `/Producer` stamp.
    let mut up = PdfUpdate::begin(&pdf, opts.producer.as_deref())?;

    if !fields.is_empty() {
        let page_ids = up.resolve_pages(&pdf, fields)?;
        let page_count = page_ids.len();

        // Standard Type1 fonts: Helvetica for text/choice, ZapfDingbats for
        // checkboxes. Both are among the 14 standard PDF fonts every conforming
        // reader provides.
        let helv_id = alloc_id(&mut up.next_id)?;
        let zadb_id = alloc_id(&mut up.next_id)?;
        up.objects.push(type1_font_object(
            helv_id,
            typography::TEXT_FONT,
            Some("WinAnsiEncoding"),
        ));
        up.objects
            .push(type1_font_object(zadb_id, typography::CHECK_FONT, None));

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

            let stream_id = alloc_id(&mut up.next_id)?;
            up.objects.push(content_stream_object(
                stream_id,
                &build_content_stream(&drawable),
            ));

            let page_obj_id = page_ids[page_idx];
            let (s, e) = find_object_bytes(&pdf, page_obj_id)
                .ok_or_else(|| err(CODE_PARSE, format!("page object {page_obj_id} not found")))?;
            let pg_dict = extract_outer_dict(&pdf[s..e])
                .ok_or_else(|| err(CODE_PARSE, "page dict not parseable"))?;

            let new_pg = rewrite_page_for_flatten(pg_dict, helv_id, zadb_id, stream_id)?;
            up.objects.push(dict_object(page_obj_id, &new_pg));
        }
    }

    let flat = up.finish(pdf)?;
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
                let size = typography::check_size(h);
                // ZapfDingbats check glyphs are roughly square; centre in the box.
                let x_pos = x0 + (w - size * typography::CHECK_GLYPH_WIDTH_FACTOR) * 0.5;
                let y_pos = y0 + (h - size) * 0.5;
                write_zadb_char(&mut out, b'4', x_pos, y_pos, size);
            }
            FieldType::Text { .. } => {
                if let Some(value) = &spec.value {
                    let size = typography::value_size(h);
                    let x_pos = x0 + typography::TEXT_INSET;
                    // First baseline just inside the top edge.
                    let y_top = y1 - size - typography::TEXT_TOP_INSET;
                    let lines: Vec<&str> = value.lines().collect();
                    write_text_block(&mut out, &lines, x_pos, y_top, size, spec.rect);
                }
            }
            FieldType::Choice { .. } => {
                if let Some(value) = &spec.value {
                    let size = typography::value_size(h);
                    let x_pos = x0 + typography::TEXT_INSET;
                    let y_pos = y0 + (h - size) * 0.5;
                    write_text_block(&mut out, &[value.as_str()], x_pos, y_pos, size, spec.rect);
                }
            }
        }
    }
    out
}

/// Write a `q/Q`-wrapped `BT/ET` block for one or more lines of text using
/// `/Helv`. The block is clipped to `clip` (`[x0, y0, x1, y1]`, the field box)
/// so an over-long or multiline value can't paint outside its box and over
/// neighbouring content. Line bytes are transcoded to WinAnsi to match the
/// `/Helv` font's `/Encoding /WinAnsiEncoding`.
fn write_text_block(out: &mut Vec<u8>, lines: &[&str], x: f32, y: f32, size: f32, clip: [f32; 4]) {
    if lines.is_empty() {
        return;
    }
    let line_h = size * typography::LINE_SPACING;
    let [cx0, cy0, cx1, cy1] = clip;
    out.extend_from_slice(b"q\n");
    // Clip to the field box: `x y w h re W n`.
    push_f32(out, cx0);
    out.push(b' ');
    push_f32(out, cy0);
    out.push(b' ');
    push_f32(out, cx1 - cx0);
    out.push(b' ');
    push_f32(out, cy1 - cy0);
    out.extend_from_slice(b" re W n\n");
    out.extend_from_slice(b"BT\n/Helv ");
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
        pdf_escape(out, &winansi_encode(line));
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
/// An indirect `/Resources` (or `/Font`) reference is a clean error: the flatten
/// content stream selects `/Helv` and `/ZaDb` by *resource name*, and a `Tf`
/// name must resolve through the page's `/Font` subdictionary — even the
/// standard-14 fonts need a `/Font` entry mapping the name, so skipping
/// injection would leave the value text unresolvable (blank). The reader's input
/// contract produces inline resources, so this only rejects out-of-contract
/// input rather than silently dropping it.
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
                // Indirect /Resources ref: cannot inject the named font, and the
                // emitted `/Helv`/`/ZaDb` Tf operators would not resolve.
                return Err(err(
                    CODE_PARSE,
                    "page /Resources is an indirect reference; flatten requires inline resources",
                ));
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
                        // Indirect /Font ref: same unresolvable-name problem.
                        return Err(err(
                            CODE_PARSE,
                            "page /Resources /Font is an indirect reference; flatten requires \
                             an inline /Font dict",
                        ));
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

/// Build a base-14 Type1 font object. `encoding`, when given, is emitted as
/// `/Encoding /<name>` — text fonts use `WinAnsiEncoding` so the content stream's
/// WinAnsi bytes render correctly; symbol fonts (ZapfDingbats) pass `None` and
/// keep their built-in encoding.
fn type1_font_object(id: u32, base_font: &str, encoding: Option<&str>) -> UpdatedObject {
    let enc = match encoding {
        Some(name) => format!(" /Encoding /{name}"),
        None => String::new(),
    };
    UpdatedObject {
        id,
        bytes: format!(
            "{id} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /{base_font}{enc} >>\nendobj\n"
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

// ── PDF text helpers ──────────────────────────────────────────────────────────

/// Append `v` as a compact `%.2f` float, stripping trailing zeros and dot.
fn push_f32(out: &mut Vec<u8>, v: f32) {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(s.as_bytes());
}

// ── Tests ───────────────────────────────────────────────────────────────────
//
// This module inherits the whole file's `#[cfg(feature = "preview")]` gate, so
// it only compiles/runs under `--features preview`. It restores the byte-level
// coverage of the flatten output that the (now-removed) public `flatten: true`
// integration tests used to provide, exercised here at the `flatten()` unit
// level (plus the internal `build_content_stream` for the focused
// transcoding/clipping byte windows) — no public render-option knob involved.
#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Document as PdfDoc;
    use quillmark_pdf::CHECKBOX_ON_STATE;

    /// A stripped single-page US-Letter background PDF ([0 0 612 792], no
    /// AcroForm, no annots) — the perfect flatten base.
    const BASE: &[u8] =
        include_bytes!("../../../fixtures/resources/quills/sample_form/0.1.0/form.pdf");

    fn text_field(name: &str, value: &str) -> FieldSpec {
        FieldSpec {
            name: name.to_string(),
            page: 0,
            rect: [72.0, 700.0, 300.0, 720.0],
            field_type: FieldType::Text { multiline: false },
            value: Some(value.to_string()),
            tooltip: None,
        }
    }

    fn checkbox_field(name: &str, checked: bool) -> FieldSpec {
        FieldSpec {
            name: name.to_string(),
            page: 0,
            rect: [72.0, 660.0, 90.0, 678.0],
            field_type: FieldType::Checkbox,
            value: checked.then(|| CHECKBOX_ON_STATE.to_string()),
            tooltip: None,
        }
    }

    fn flatten_ok(fields: &[FieldSpec]) -> Vec<u8> {
        flatten(BASE.to_vec(), fields, &StampOptions::default())
            .expect("flatten succeeds")
            .pdf
    }

    fn contains_window(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// 1. The flat output reparses cleanly and its catalog carries NO
    ///    `/AcroForm` — values live in content streams, not widgets.
    #[test]
    fn flatten_produces_no_acroform() {
        let pdf = flatten_ok(&[text_field("FullName", "Ada Lovelace")]);

        let doc = PdfDoc::load_mem(&pdf).expect("lopdf reparse — structurally valid");
        let cat = doc.catalog().expect("catalog");
        assert!(
            cat.get(b"AcroForm").is_err(),
            "flat PDF must not contain /AcroForm"
        );
    }

    /// 2. The flat output declares both standard fonts, a WinAnsi-encoded text
    ///    font, the `BT`/`Tj` text operators, and the literal field value.
    #[test]
    fn flatten_has_fonts_and_text_operators() {
        let pdf = flatten_ok(&[text_field("FullName", "Ada Lovelace")]);
        let text = String::from_utf8_lossy(&pdf);

        assert!(text.contains("/Helvetica"), "must declare Helvetica");
        assert!(
            text.contains("/ZapfDingbats"),
            "must declare ZapfDingbats (checkbox glyph font)"
        );
        assert!(
            text.contains("/Encoding /WinAnsiEncoding"),
            "text font must declare WinAnsiEncoding"
        );
        assert!(
            text.contains("BT\n") || text.contains("BT "),
            "must contain BT (begin text) operator"
        );
        assert!(
            text.contains("Tj\n") || text.contains("Tj "),
            "must contain Tj (show text) operator"
        );
        assert!(
            text.contains("Ada Lovelace"),
            "must contain the field value literal"
        );
    }

    /// 3. Each text value clips to its field rect (`re W n`) so it can't
    ///    overflow over neighbouring content.
    #[test]
    fn flatten_clips_to_field_box() {
        let pdf = flatten_ok(&[text_field("FullName", "Ada Lovelace")]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains(" re W n"),
            "text must clip to the field box (re W n)"
        );
    }

    /// 4. Non-ASCII CP1252 chars are transcoded to their WinAnsi bytes in the
    ///    content stream (not drawn as raw UTF-8). Asserted directly on
    ///    `build_content_stream` — the closest seam to the original byte-window
    ///    test — and also that the value reaches the full `flatten()` output.
    #[test]
    fn build_content_stream_transcodes_non_ascii_to_winansi() {
        // é(U+00E9) — (U+2014) ñ(U+00F1) ’(U+2019)
        let value = "Caf\u{e9} \u{2014} Se\u{f1}or \u{2019}A\u{2019}";
        let spec = text_field("FullName", value);
        let stream = build_content_stream(&[&spec]);

        // WinAnsi bytes: é→0xE9, —→0x97, ñ→0xF1, ’→0x92. Drawn literal:
        // `Caf<E9> <97> Se<F1>or <92>A<92>`.
        let want: &[u8] = &[
            b'C', b'a', b'f', 0xE9, b' ', 0x97, b' ', b'S', b'e', 0xF1, b'o', b'r', b' ', 0x92,
            b'A', 0x92,
        ];
        assert!(
            contains_window(&stream, want),
            "content stream must carry the WinAnsi-encoded value bytes"
        );
        // The raw UTF-8 sequence for é (0xC3 0xA9) must NOT appear as a drawn
        // literal — that would be the pre-fix corruption.
        assert!(
            !contains_window(&stream, &[b'f', 0xC3, 0xA9, b' ']),
            "value must not be drawn as raw UTF-8"
        );

        // And the same bytes survive the full flatten() envelope.
        let pdf = flatten_ok(&[spec]);
        assert!(
            contains_window(&pdf, want),
            "flat PDF must carry the WinAnsi-encoded value bytes"
        );
    }

    /// 5. A checked checkbox wires the ZapfDingbats font and draws the check
    ///    glyph via the `write_zadb_char` path (`/ZaDb` + the `'4'` glyph).
    #[test]
    fn flatten_checked_checkbox_emits_zapfdingbats_glyph() {
        let spec = checkbox_field("Agree", true);
        let stream = build_content_stream(&[&spec]);
        let text = String::from_utf8_lossy(&stream);

        assert!(
            text.contains("/ZaDb"),
            "checked checkbox must select the ZapfDingbats font (/ZaDb)"
        );
        // The check glyph is ZapfDingbats byte 0x34 ('4'), drawn as `(4) Tj`.
        assert!(
            contains_window(&stream, b"(4) Tj"),
            "checked checkbox must draw the check glyph"
        );

        // The font object is wired in the full flatten() output.
        let pdf = flatten_ok(&[spec]);
        assert!(
            String::from_utf8_lossy(&pdf).contains("/ZapfDingbats"),
            "flat PDF must declare the ZapfDingbats font"
        );

        // Value-gating lives in flatten() (via `has_drawable_value`), not in
        // `build_content_stream`: an unchecked checkbox is filtered out before
        // it reaches the stream, so the check glyph is never drawn.
        assert!(has_drawable_value(&checkbox_field("Agree", true)));
        assert!(!has_drawable_value(&checkbox_field("Agree", false)));
        let unchecked = flatten_ok(&[checkbox_field("Agree", false)]);
        assert!(
            !contains_window(&unchecked, b"(4) Tj"),
            "unchecked checkbox must not draw the check glyph"
        );
    }
}
