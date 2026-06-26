//! The stamp operation: write a fresh `/AcroForm` (and an `/Info` `/Producer`
//! stamp) onto a base PDF via one incremental-update append.
//!
//! **Technique A** is locked: we style the *real* AcroForm fields and set
//! `/NeedAppearances`; we never bake `/AP` appearance streams. Appearance
//! synthesis is the viewer's job (Acrobat, Chrome/pdfium, Preview). Flat
//! rasterizers therefore render the fields blank — values reach non-interactive
//! output only via the [`RenderedRegion`] sidecar.
//!
//! **Opinionated rendering**: the background owns all visual chrome, so the
//! widget is a transparent input over it. One house style — Helvetica at `0 Tf`
//! (auto-size), black — registered once in the form `/DR` and named in every
//! `/DA`. The form is built fresh from the spec; we never reconcile a foreign
//! AcroForm.

use pdf_writer::types::{AnnotationFlags, FieldFlags, FieldType as PwFieldType, SigFlags};
use pdf_writer::writers::Form;
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, Str, TextStr};

use quillmark_core::{RegionKind, RenderedRegion};

use crate::error::PdfError;
use crate::reader::{
    append_incremental_update, assert_traditional_xref, err, extract_outer_dict, find_dict_value,
    find_object_bytes, find_startxref, find_trailer_dict, parse_indirect_ref, resolve_page_ids,
    UpdatedObject,
};
use crate::{FieldSpec, FieldType};

const CODE_PARSE: &str = "pdf::stamp_parse";

/// The fixed checkbox on-state export name the engine rebuilds the form with.
/// A resolver producing a checkbox [`FieldSpec`] sets `value` to this when the
/// box is checked, and `None` when it is not.
pub const CHECKBOX_ON_STATE: &str = "Yes";

/// House-style default appearance: Helvetica, `0 Tf` (auto-size), black fill.
/// `Helv` is registered in the form `/DR` `/Font`.
const DEFAULT_APPEARANCE: &[u8] = b"/Helv 0 Tf 0 g";

/// Options for [`stamp`](crate::stamp).
#[derive(Debug, Clone, Default)]
pub struct StampOptions {
    /// `/Info` `/Producer` override, passed down from the product layer. `None`
    /// leaves the base PDF's `/Producer` untouched. The spine never defaults
    /// this from its own crate version.
    pub producer: Option<String>,
}

/// Result of [`stamp`](crate::stamp): the stamped PDF and the region sidecar.
#[derive(Debug, Clone)]
pub struct StampResult {
    pub pdf: Vec<u8>,
    pub regions: Vec<RenderedRegion>,
}

/// Stamp `fields` onto `base` as a fresh AcroForm via one incremental update,
/// optionally stamping `/Info` `/Producer`. Returns the stamped bytes plus a
/// [`RenderedRegion`] per field.
///
/// `base` must satisfy the reader's input contract (traditional-xref,
/// unencrypted, inline-annots, flat-tree). Each field's `rect` is final
/// **bottom-left** PDF-point geometry — the spine never reasons about page
/// height or reflow; the caller converts.
pub fn stamp(
    base: Vec<u8>,
    fields: &[FieldSpec],
    opts: &StampOptions,
) -> Result<StampResult, PdfError> {
    let regions = regions_of(fields);

    // Nothing to write: no producer stamp and no fields. Return the base as-is
    // rather than append an empty revision.
    if opts.producer.is_none() && fields.is_empty() {
        return Ok(StampResult { pdf: base, regions });
    }

    let pdf = base;
    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;

    let trailer = find_trailer_dict(&pdf, xref_offset)?;
    if find_dict_value(trailer, "Encrypt").is_some() {
        return Err(err(
            "pdf::encrypted",
            "PDF is encrypted; the stamp spine does not handle encrypted PDFs",
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

    // Object ids are handed out from a single counter so created objects never
    // collide with the base's, nor with each other.
    let mut next_id = size;
    let mut objects: Vec<UpdatedObject> = Vec::new();
    let mut extra_info_ref = None;

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
                let info_id = next_id;
                next_id += 1;
                let mut inner = b"/Producer ".to_vec();
                inner.extend_from_slice(&literal);
                objects.push(dict_object(info_id, &inner));
                extra_info_ref = Some(info_id);
            }
        }
    }

    // ─── AcroForm + widgets (when there are fields) ──────────────────────────
    if !fields.is_empty() {
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

        let font_id = next_id;
        next_id += 1;
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

        // Widgets, grouped by page for the page-side `/Annots`.
        let mut widgets_by_page: Vec<Vec<u32>> = vec![Vec::new(); page_count];
        for (spec, &wid) in fields.iter().zip(&widget_ids) {
            widgets_by_page[spec.page].push(wid);
            let page_ref = Ref::new(page_ids[spec.page] as i32);
            objects.push(UpdatedObject {
                id: wid,
                bytes: write_widget_object(spec, Ref::new(wid as i32), page_ref),
            });
        }

        // The Helvetica /DR font, registered once and named `Helv`.
        let mut fchunk = Chunk::new();
        fchunk
            .type1_font(Ref::new(font_id as i32))
            .base_font(Name(b"Helvetica"));
        objects.push(UpdatedObject {
            id: font_id,
            bytes: fchunk.as_bytes().to_vec(),
        });

        // The AcroForm dictionary.
        let has_signature = fields
            .iter()
            .any(|f| matches!(f.field_type, FieldType::Signature));
        let mut achunk = Chunk::new();
        {
            let mut form: Form<'_> = achunk
                .indirect(Ref::new(acroform_id as i32))
                .start::<Form>();
            form.fields(widget_ids.iter().map(|&id| Ref::new(id as i32)));
            if has_signature {
                form.sig_flags(SigFlags::SIGNATURES_EXIST);
            }
            form.pair(Name(b"NeedAppearances"), true);
            form.default_appearance(Str(DEFAULT_APPEARANCE));
            // /DR << /Font << /Helv <font> >> >>
            {
                let mut dr = form.insert(Name(b"DR")).dict();
                let mut font_dict = dr.insert(Name(b"Font")).dict();
                font_dict.pair(Name(b"Helv"), Ref::new(font_id as i32));
            }
            form.finish();
        }
        objects.push(UpdatedObject {
            id: acroform_id,
            bytes: achunk.as_bytes().to_vec(),
        });

        // A widget is fillable only if reachable both ways: the catalog's
        // `/AcroForm /Fields` (added here) and the page's `/Annots` (below).
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
            let pg_dict = extract_outer_dict(&pdf[s..e])
                .ok_or_else(|| err(CODE_PARSE, format!("page node {page_obj_id} not parseable")))?;
            objects.push(dict_object(
                page_obj_id,
                &rewrite_page_with_annots(pg_dict, widget_refs)?,
            ));
        }
    }

    let new_size = next_id;
    let stamped = append_incremental_update(
        pdf,
        xref_offset,
        catalog_id,
        new_size,
        extra_info_ref,
        &objects,
    )?;
    Ok(StampResult {
        pdf: stamped,
        regions,
    })
}

/// Build a [`RenderedRegion`] per field — the geometry+value sidecar that is the
/// only path to values in non-interactive output. Shared by `stamp` and any
/// no-stamp render path so the region geometry always matches the widget.
pub fn regions_of(fields: &[FieldSpec]) -> Vec<RenderedRegion> {
    fields
        .iter()
        .map(|f| RenderedRegion {
            name: f.name.clone(),
            page: f.page,
            rect: f.rect,
            kind: RegionKind::Field {
                field_type: f.field_type.type_id().to_string(),
                value: f.value.clone(),
            },
        })
        .collect()
}

/// Serialize one field as a merged field+widget indirect object via pdf-writer.
fn write_widget_object(spec: &FieldSpec, wid: Ref, page_ref: Ref) -> Vec<u8> {
    let mut chunk = Chunk::new();
    {
        let mut field = chunk.form_field(wid);
        field.partial_name(TextStr(&spec.name));
        if let Some(tt) = spec.tooltip.as_deref() {
            field.alternate_name(TextStr(tt));
        }

        // Checkbox on-state, captured to also set the annotation `/AS` below.
        let mut checkbox_on: Option<bool> = None;

        match &spec.field_type {
            FieldType::Text { multiline } => {
                field.field_type(PwFieldType::Text);
                field.vartext_default_appearance(Str(DEFAULT_APPEARANCE));
                if *multiline {
                    field.field_flags(FieldFlags::MULTILINE);
                }
                if let Some(v) = spec.value.as_deref() {
                    field.text_value(TextStr(v));
                }
            }
            FieldType::Checkbox => {
                field.field_type(PwFieldType::Button);
                let on = spec.value.is_some();
                checkbox_on = Some(on);
                let on_name = spec.value.as_deref().unwrap_or(CHECKBOX_ON_STATE);
                // /V is the on-state name when checked, else /Off.
                field.pair(
                    Name(b"V"),
                    if on {
                        Name(on_name.as_bytes())
                    } else {
                        Name(b"Off")
                    },
                );
                // /MK << /CA (4) >> — the ZapfDingbats check glyph the viewer
                // synthesizes under NeedAppearances.
                {
                    let mut mk = field.insert(Name(b"MK")).dict();
                    mk.pair(Name(b"CA"), Str(b"4"));
                }
            }
            FieldType::Choice { options } => {
                field.field_type(PwFieldType::Choice);
                // Choice is always a dropdown (combo box).
                field.field_flags(FieldFlags::COMBO);
                field.vartext_default_appearance(Str(DEFAULT_APPEARANCE));
                {
                    let mut opts = field.choice_options();
                    for o in options {
                        opts.option(TextStr(o));
                    }
                }
                if let Some(v) = spec.value.as_deref() {
                    field.choice_value(Some(TextStr(v)));
                }
            }
            FieldType::Signature => {
                field.field_type(PwFieldType::Signature);
            }
        }

        // `into_annotation` writes `/Type /Annot` + `/Subtype /Widget` exactly
        // once; do not call `.subtype()` again or it duplicates the key.
        let mut ann = field.into_annotation();
        ann.rect(Rect::new(
            spec.rect[0],
            spec.rect[1],
            spec.rect[2],
            spec.rect[3],
        ))
        .page(page_ref)
        .flags(AnnotationFlags::PRINT);
        if let Some(on) = checkbox_on {
            ann.appearance_state(if on {
                Name(CHECKBOX_ON_STATE.as_bytes())
            } else {
                Name(b"Off")
            });
        }
        ann.finish();
    }
    chunk.as_bytes().to_vec()
}

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
            // `value` starts exactly after the matched `/Producer` key, so the
            // key span is `[value_start - key.len, value_end)` — derived, not
            // re-scanned, so a `/Producer` token inside another value can't be
            // matched by accident.
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

/// Three cases for the existing `/Annots`: absent (write a fresh array);
/// inline array (splice widget refs before `]`); indirect reference (hard
/// error — the input contract requires inline annots).
fn rewrite_page_with_annots(pg_dict: &[u8], widget_refs: &[u32]) -> Result<Vec<u8>, PdfError> {
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
                    "pdf::indirect_annots",
                    "/Annots is an indirect reference; only inline arrays are supported",
                ))
            } else {
                Err(err(CODE_PARSE, "/Annots is neither array nor indirect ref"))
            }
        }
    }
}
