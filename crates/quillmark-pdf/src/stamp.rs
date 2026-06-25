//! Stamp styled AcroForm widgets onto a base PDF via one incremental update.
//!
//! [`stamp`] is the shared spine of issue #744: both backends produce a base
//! PDF plus a `&[FieldSpec]`, and this writes the form fresh — it never reads or
//! reconciles a foreign `/AcroForm`. The base PDF must be unencrypted with a
//! traditional xref table (decrypt/normalize happens upstream in the
//! qualification layer).
//!
//! The revision carries, in a single appended xref/trailer:
//! - the `/Info` `/Producer` stamp (always); and
//! - one Widget annotation per field, a fresh indirect `/AcroForm`, an updated
//!   catalog and updated page `/Annots` (only when there are fields).
//!
//! Technique A is locked: no baked `/AP` appearance streams. We style the real
//! fields (`/DA`, `/MK`, `/Ff`, `/MaxLen`) and set `/NeedAppearances true` so
//! the viewer renders them. `/DA` fonts (`Helv`, `ZaDb`) are registered in the
//! AcroForm `/DR`.

use quillmark_core::RenderError;

use crate::scan::{
    append_incremental_update, assert_traditional_xref, err, extract_outer_dict, find_dict_value,
    find_object_bytes, find_startxref, find_trailer_dict, parse_indirect_ref, resolve_page_ids,
    rewrite_page_with_annots, UpdatedObject,
};
use crate::spec::{FieldSpec, FieldType, RenderedRegion};

const CODE_PARSE: &str = "pdf::parse";

/// Options for a stamp pass.
#[derive(Debug, Clone, Default)]
pub struct StampOptions {
    /// `/Info` `/Producer` override. `None` uses [`default_producer`].
    pub producer: Option<String>,
}

/// The stamped artifact plus the phase-1 regions sidecar (field geometry the
/// GUI uses for its interactivity overlay).
#[derive(Debug, Clone)]
pub struct StampResult {
    pub pdf: Vec<u8>,
    pub regions: Vec<RenderedRegion>,
}

/// Default `/Producer`: `Quillmark <crate-version>`.
pub fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}

/// Count the pages of a base PDF (traditional xref, unencrypted), the same way
/// [`stamp`] resolves them. Backends use this to report `page_count` without a
/// second PDF parser.
pub fn page_count(pdf: &[u8]) -> Result<usize, RenderError> {
    let xref_offset = find_startxref(pdf)?;
    assert_traditional_xref(pdf, xref_offset)?;
    let trailer = find_trailer_dict(pdf, xref_offset)?;
    let (catalog_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(CODE_PARSE, "/Root missing or malformed in trailer"))?;
    Ok(resolve_page_ids(pdf, catalog_id)?.len())
}

/// Write `fields` as a fresh AcroForm onto `pdf` and report their geometry.
///
/// With no fields this still appends the `/Producer` stamp (and adds no
/// `/AcroForm`), so a stamp pass is unconditional in the render pipeline.
pub fn stamp(
    pdf: Vec<u8>,
    fields: &[FieldSpec],
    opts: &StampOptions,
) -> Result<StampResult, RenderError> {
    let producer = opts.producer.clone().unwrap_or_else(default_producer);

    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;

    let trailer = find_trailer_dict(&pdf, xref_offset)?;
    if find_dict_value(trailer, "Encrypt").is_some() {
        return Err(err(
            "pdf::encrypted",
            "PDF is encrypted; decrypt upstream (qpdf --decrypt) before stamping",
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

    // Object ids come from one counter so a (defensive) fresh `/Info` object
    // can't collide with widget/AcroForm ids.
    let mut next_id = size;
    let mut objects: Vec<UpdatedObject> = Vec::new();
    let mut extra_info_ref = None;

    // ─── /Info /Producer (always) ────────────────────────────────────────────
    let literal = pdf_text_string(&producer);
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

    // ─── widgets + AcroForm (only when there are fields) ──────────────────────
    if !fields.is_empty() {
        let page_ids = resolve_page_ids(&pdf, catalog_id)?;
        let page_count = page_ids.len();
        for f in fields {
            if f.page >= page_count {
                return Err(err(
                    CODE_PARSE,
                    format!(
                        "field {:?} references page {} but the PDF has {} page(s)",
                        f.name, f.page, page_count
                    ),
                ));
            }
        }

        let needs_da = fields.iter().any(|f| f.field_type.needs_appearance());
        let has_sig = fields
            .iter()
            .any(|f| matches!(f.field_type, FieldType::Signature));

        let widget_ids: Vec<u32> = fields
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
        for (f, &wid) in fields.iter().zip(&widget_ids) {
            widgets_by_page[f.page].push(wid);
            objects.push(dict_object(wid, &build_widget_inner(f, page_ids[f.page])));
        }

        objects.push(dict_object(
            acroform_id,
            &build_acroform_inner(&widget_ids, has_sig, needs_da),
        ));

        // A widget is fillable only if reachable both ways: the catalog's
        // `/AcroForm /Fields` (document-wide registry) and the page's `/Annots`
        // (what actually paints it). Either alone leaves it unlisted/invisible.
        let (cs, ce) = find_object_bytes(&pdf, catalog_id)
            .ok_or_else(|| err(CODE_PARSE, "catalog not found"))?;
        let cat_dict = extract_outer_dict(&pdf[cs..ce])
            .ok_or_else(|| err(CODE_PARSE, "catalog dict not parseable"))?;
        let mut cat_inner = cat_dict.to_vec();
        cat_inner.extend_from_slice(format!(" /AcroForm {acroform_id} 0 R").as_bytes());
        objects.push(dict_object(catalog_id, &cat_inner));

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
    let out = append_incremental_update(
        pdf,
        xref_offset,
        catalog_id,
        new_size,
        extra_info_ref,
        &objects,
    )?;

    let regions = fields.iter().map(FieldSpec::to_region).collect();
    Ok(StampResult { pdf: out, regions })
}

// ─── widget / AcroForm serialization ─────────────────────────────────────────

/// AcroForm-flag bit for a combo box (`/Ff` bit 18, 1-based → value `1 << 17`).
const FF_COMBO: u32 = 1 << 17;

/// Inner dict bytes for one widget — everything between `<<` and `>>`.
fn build_widget_inner(field: &FieldSpec, page_id: u32) -> Vec<u8> {
    let [x0, y0, x1, y1] = field.rect;
    let mut out = Vec::new();
    out.extend_from_slice(b"/Type /Annot /Subtype /Widget");
    out.extend_from_slice(
        format!(
            " /Rect [{} {} {} {}]",
            fmt_num(x0),
            fmt_num(y0),
            fmt_num(x1),
            fmt_num(y1)
        )
        .as_bytes(),
    );
    out.extend_from_slice(format!(" /P {page_id} 0 R /F 4 /T ").as_bytes());
    out.extend_from_slice(&pdf_text_string(&field.name));
    if let Some(tu) = &field.tooltip {
        out.extend_from_slice(b" /TU ");
        out.extend_from_slice(&pdf_text_string(tu));
    }

    let mut ff = field.flags;
    match &field.field_type {
        FieldType::Signature => {
            out.extend_from_slice(b" /FT /Sig");
        }
        FieldType::Text => {
            out.extend_from_slice(b" /FT /Tx");
            if let Some(ml) = field.max_len {
                out.extend_from_slice(format!(" /MaxLen {ml}").as_bytes());
            }
            push_da(&mut out, field, "/Helv 0 Tf 0 g");
            if let Some(v) = &field.value {
                out.extend_from_slice(b" /V ");
                out.extend_from_slice(&pdf_text_string(v));
            }
            if let Some(dv) = &field.default_value {
                out.extend_from_slice(b" /DV ");
                out.extend_from_slice(&pdf_text_string(dv));
            }
            push_mk(&mut out, field);
        }
        FieldType::Choice { options, combo } => {
            out.extend_from_slice(b" /FT /Ch");
            if *combo {
                ff |= FF_COMBO;
            }
            out.extend_from_slice(b" /Opt [");
            for (i, opt) in options.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                match &opt.display {
                    None => out.extend_from_slice(&pdf_text_string(&opt.export)),
                    Some(d) => {
                        out.push(b'[');
                        out.extend_from_slice(&pdf_text_string(&opt.export));
                        out.push(b' ');
                        out.extend_from_slice(&pdf_text_string(d));
                        out.push(b']');
                    }
                }
            }
            out.push(b']');
            push_da(&mut out, field, "/Helv 0 Tf 0 g");
            if let Some(v) = &field.value {
                out.extend_from_slice(b" /V ");
                out.extend_from_slice(&pdf_text_string(v));
            }
            push_mk(&mut out, field);
        }
        FieldType::Checkbox { on_state, checked } => {
            out.extend_from_slice(b" /FT /Btn");
            let state = if *checked { on_state.as_str() } else { "Off" };
            out.extend_from_slice(b" /V ");
            out.extend_from_slice(&pdf_name(state));
            out.extend_from_slice(b" /AS ");
            out.extend_from_slice(&pdf_name(state));
            push_da(&mut out, field, "/ZaDb 0 Tf 0 g");
            // `/MK /CA (4)` is the ZapfDingbats check caption. Painting the
            // glyph generally needs a baked `/AP` (Technique B, out of scope);
            // the structural field + caption is the spike's commitment.
            out.extend_from_slice(b" /MK << /CA (4)");
            push_mk_colors(&mut out, field);
            out.extend_from_slice(b" >>");
        }
    }
    if ff != 0 {
        out.extend_from_slice(format!(" /Ff {ff}").as_bytes());
    }
    out
}

/// Inner dict bytes for the document `/AcroForm`.
fn build_acroform_inner(widget_ids: &[u32], has_sig: bool, needs_da: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"/Fields [");
    for (i, id) in widget_ids.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        out.extend_from_slice(format!("{id} 0 R").as_bytes());
    }
    out.push(b']');
    if has_sig {
        out.extend_from_slice(b" /SigFlags 1");
    }
    out.extend_from_slice(b" /NeedAppearances true");
    if needs_da {
        out.extend_from_slice(b" /DA ");
        out.extend_from_slice(&pdf_text_string("/Helv 0 Tf 0 g"));
        // Standard-14 fonts need no embedded font file; `/DR` makes the `/DA`
        // font names resolvable, which the spec requires.
        out.extend_from_slice(
            b" /DR << /Font << \
              /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> \
              /ZaDb << /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >> \
              >> >>",
        );
    }
    out
}

fn push_da(out: &mut Vec<u8>, field: &FieldSpec, default: &str) {
    let da = field.appearance.da.as_deref().unwrap_or(default);
    out.extend_from_slice(b" /DA ");
    out.extend_from_slice(&pdf_text_string(da));
}

/// `/MK << … >>` with border (`/BC`) and background (`/BG`) colors, emitted
/// only when at least one is set.
fn push_mk(out: &mut Vec<u8>, field: &FieldSpec) {
    if field.appearance.border_color.is_none() && field.appearance.background_color.is_none() {
        return;
    }
    out.extend_from_slice(b" /MK <<");
    push_mk_colors(out, field);
    out.extend_from_slice(b" >>");
}

fn push_mk_colors(out: &mut Vec<u8>, field: &FieldSpec) {
    if let Some([r, g, b]) = field.appearance.border_color {
        out.extend_from_slice(
            format!(" /BC [{} {} {}]", fmt_num(r), fmt_num(g), fmt_num(b)).as_bytes(),
        );
    }
    if let Some([r, g, b]) = field.appearance.background_color {
        out.extend_from_slice(
            format!(" /BG [{} {} {}]", fmt_num(r), fmt_num(g), fmt_num(b)).as_bytes(),
        );
    }
}

// ─── low-level PDF encoding ──────────────────────────────────────────────────

/// Serialize one indirect object from its inner dict bytes:
/// `<id> 0 obj\n<< <inner> >>\nendobj\n`.
fn dict_object(id: u32, inner: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< ").into_bytes();
    bytes.extend_from_slice(inner);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}

/// Replace `/Producer`'s value if present, else append the entry.
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
            // `value` starts exactly after the matched `/Producer` key
            // (find_dict_value's contract), so the key span is derivable rather
            // than re-scanned — avoids matching the token inside another key.
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
/// and `\` escaped; anything else uses a UTF-16BE hex string with a BOM.
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

/// Encode `s` as a PDF name (`/…`), `#`-escaping bytes outside the printable
/// ASCII range and the name delimiters.
fn pdf_name(s: &str) -> Vec<u8> {
    let mut out = vec![b'/'];
    for &b in s.as_bytes() {
        let needs_escape = !(0x21..=0x7e).contains(&b)
            || matches!(
                b,
                b'/' | b'#' | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'%'
            );
        if needs_escape {
            out.extend_from_slice(format!("#{:02X}", b).as_bytes());
        } else {
            out.push(b);
        }
    }
    out
}

/// Format a coordinate/number compactly: fixed precision, trailing zeros and a
/// dangling point trimmed.
fn fmt_num(v: f32) -> String {
    let s = format!("{:.4}", v);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    match trimmed {
        "" | "-" | "-0" => "0".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_escapes_space_and_delimiters() {
        assert_eq!(pdf_name("Yes"), b"/Yes");
        assert_eq!(pdf_name("a b"), b"/a#20b");
        assert_eq!(pdf_name("x/y"), b"/x#2Fy");
    }

    #[test]
    fn num_trims_cleanly() {
        assert_eq!(fmt_num(200.0), "200");
        assert_eq!(fmt_num(72.5), "72.5");
        assert_eq!(fmt_num(-0.0), "0");
    }

    #[test]
    fn text_string_escapes_literal() {
        assert_eq!(pdf_text_string("a(b)\\c"), b"(a\\(b\\)\\\\c)");
    }
}
