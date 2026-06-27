//! # quillmark-qualify — the qualification layer
//!
//! The **inverse** of the `quillmark-pdf` stamping spine. It turns a real
//! AcroForm PDF (e.g. a government form) into the two assets a `pdfform` quill
//! ships:
//!
//! - **`form.pdf`** — the *stripped background*: the gov form with its
//!   `/AcroForm`, widget annotations, and page `/Annots` removed, re-serialized
//!   as traditional-xref so it satisfies the spine's input contract.
//! - **`form.json`** — the value-free field reconstruction spec, emitted via
//!   `quillmark-pdfform`'s wire types (the single source of truth for the
//!   schema).
//!
//! Pipeline: decrypt → extract field definitions → strip → re-serialize.
//! Everything runs in-process via `lopdf` (qpdf is not available). This crate
//! is build-time tooling and **never** depends on Typst.
//!
//! See [`qualify`] for the operation.

mod error;

pub use error::QualifyError;

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document, Object, ObjectId};

use quillmark_pdfform::{FieldKind, FormField, FormSpec, Rect};

/// AcroForm field flag bits (`/Ff`). The inverse of the spine's writer, which
/// sets only `MULTILINE` and `COMBO`; the rest discriminate field *shapes* we
/// must classify (and the pushbutton/radio shapes we skip).
const FF_MULTILINE: i64 = 0x1000; // bit 13
const FF_RADIO: i64 = 0x8000; // bit 16
const FF_PUSHBUTTON: i64 = 0x1_0000; // bit 17

/// The form.json `schema` value this layer emits. Hand-set, never
/// `CARGO_PKG_VERSION`, following the Document DTO convention.
const SCHEMA: &str = "quillmark/form@0.1.0";

/// The two assets a `pdfform` quill ships, produced from one AcroForm PDF.
#[derive(Debug, Clone)]
pub struct Qualified {
    /// The stripped background: traditional-xref, unencrypted, no `/AcroForm`,
    /// no widget annotations, no page `/Annots`, no `/Encrypt`.
    pub form_pdf: Vec<u8>,
    /// The pretty-printed `form.json` field spec.
    pub form_json: Vec<u8>,
}

/// Qualify an AcroForm PDF into a `pdfform` quill's `form.pdf` + `form.json`.
///
/// `password` decrypts an encrypted input (`Some("")` for the common
/// empty-user-password case; `None` is treated as `""`). Field definitions are
/// extracted from the catalog `/AcroForm /Fields`, geometry is inverted from
/// the widget `/Rect` back to top-left page-relative `{x,y,w,h}`, and the
/// background is stripped and re-serialized as traditional xref so it satisfies
/// the stamp spine's input contract.
///
/// Pushbuttons, radio groups, and unrecognized `/FT` values are **skipped**
/// (radio is deferred per the proposal); the rest map to the four supported
/// kinds.
pub fn qualify(pdf_bytes: &[u8], password: Option<&str>) -> Result<Qualified, QualifyError> {
    let mut doc = Document::load_mem(pdf_bytes)
        .map_err(|e| QualifyError::Malformed(format!("load failed: {e}")))?;

    if doc.is_encrypted() {
        doc.decrypt(password.unwrap_or(""))
            .map_err(|e| QualifyError::Decrypt(e.to_string()))?;
    }

    // ── 1. Extract field definitions (before we mutate anything) ─────────────
    let fields = extract_fields(&doc)?;
    let spec = FormSpec {
        schema: SCHEMA.to_string(),
        fields,
    };
    let form_json = serde_json::to_vec_pretty(&spec)
        .map_err(|e| QualifyError::Internal(format!("form.json serialize failed: {e}")))?;

    // ── 2. Strip → traditional-xref background ───────────────────────────────
    let form_pdf = strip_background(doc)?;

    Ok(Qualified {
        form_pdf,
        form_json,
    })
}

/// One terminal field node plus the inherited attributes resolved from its
/// ancestors while walking the tree.
struct TerminalField {
    /// The terminal field/widget dictionary id (resolves to the merged
    /// field+widget object for a non-nested form).
    id: ObjectId,
    /// Fully-qualified name (`/T` partial names joined with `.`).
    fq_name: String,
    /// Effective field type (`/FT`), inherited if the terminal omits it.
    ft: Option<Vec<u8>>,
    /// Effective field flags (`/Ff`), inherited if the terminal omits them.
    ff: i64,
}

/// Walk `/AcroForm /Fields`, recursing into `/Kids`, and return every terminal
/// field with its fully-qualified name and inherited `/FT` + `/Ff`. Document
/// (`/AcroForm /Fields`) order is preserved.
fn collect_terminals(doc: &Document) -> Result<Vec<TerminalField>, QualifyError> {
    let acroform = match doc.catalog().ok().and_then(|cat| {
        cat.get(b"AcroForm")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
    }) {
        Some(af) => af,
        // No AcroForm at all → no fields. Not an error; a strip-only PDF is
        // still a valid (field-less) qualification.
        None => return Ok(Vec::new()),
    };

    let root_fields = match acroform.get(b"Fields").ok().and_then(|o| o.as_array().ok()) {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };

    let mut out = Vec::new();
    for obj in root_fields {
        if let Ok(id) = obj.as_reference() {
            walk_node(doc, id, None, None, 0, &mut out)?;
        }
    }
    Ok(out)
}

/// Recurse a single field-tree node. `parent_name` is the FQ prefix; `ft`/`ff`
/// are the inherited type/flags from ancestors. A node is *terminal* when it has
/// no `/Kids` that are themselves field nodes (i.e. it is a widget / leaf field).
fn walk_node(
    doc: &Document,
    id: ObjectId,
    parent_name: Option<&str>,
    inherited_ft: Option<&[u8]>,
    inherited_ff: i64,
    out: &mut Vec<TerminalField>,
) -> Result<(), QualifyError> {
    let dict = match doc.get_object(id).ok().and_then(|o| o.as_dict().ok()) {
        Some(d) => d,
        None => return Ok(()),
    };

    // This node's own /FT and /Ff override the inherited ones for its subtree.
    let ft: Option<Vec<u8>> = dict
        .get(b"FT")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n.to_vec())
        .or_else(|| inherited_ft.map(|n| n.to_vec()));
    let ff = dict
        .get(b"Ff")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(inherited_ff);

    // Build the FQ name by joining this node's /T partial onto the parent's.
    let fq_name = match dict.get(b"T").ok().and_then(|o| o.as_str().ok()) {
        Some(t) => {
            let partial = String::from_utf8_lossy(t).into_owned();
            match parent_name {
                Some(p) => format!("{p}.{partial}"),
                None => partial,
            }
        }
        None => parent_name.unwrap_or("").to_string(),
    };

    // A node's /Kids are field nodes only if they carry their own /T (a partial
    // name). Widget-only kids (a field whose appearances are split across
    // multiple widgets) have no /T and are NOT recursed as field nodes.
    let field_kids: Vec<ObjectId> = dict
        .get(b"Kids")
        .ok()
        .and_then(|o| o.as_array().ok())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.as_reference().ok())
                .filter(|kid_id| {
                    doc.get_object(*kid_id)
                        .ok()
                        .and_then(|o| o.as_dict().ok())
                        .map(|d| d.has(b"T"))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();

    if field_kids.is_empty() {
        // Terminal: the node is the field (and, in a non-nested form, also the
        // widget carrying /Rect).
        out.push(TerminalField {
            id,
            fq_name,
            ft,
            ff,
        });
    } else {
        for kid in field_kids {
            walk_node(doc, kid, Some(&fq_name), ft.as_deref(), ff, out)?;
        }
    }
    Ok(())
}

/// Map every terminal field to a [`FormField`], skipping the shapes the V1
/// pdfform backend does not reproduce (pushbutton / radio / unknown `/FT`).
fn extract_fields(doc: &Document) -> Result<Vec<FormField>, QualifyError> {
    let terminals = collect_terminals(doc)?;
    let page_index = build_page_index(doc);

    let mut fields = Vec::new();
    for term in terminals {
        let Some(mut kind) = classify(&term)? else {
            // Skipped shape (pushbutton/radio/unknown) — drop with no error.
            continue;
        };

        let dict = doc
            .get_object(term.id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .ok_or_else(|| {
                QualifyError::Malformed(format!("field {:?} not a dict", term.fq_name))
            })?;

        // `classify` builds an empty `Choice`; the options live on the dict, so
        // fill them here (a `/Ch` field's `/Opt` may be inherited, but in the
        // common shape it sits on the terminal).
        finalize_choice_options(&mut kind, dict);

        // Geometry lives on the dict carrying `/Rect`: the field node itself for
        // the spine's MERGED field+widget objects, or the field's widget-
        // annotation kid for the SPLIT shape Acrobat/gov forms produce (a field
        // node with `/T`+`/FT` but no `/Rect`, whose widget kid has `/Rect`+`/P`
        // but no `/T`). `/P` (owning page) lives wherever `/Rect` does.
        let (geom_id, geom_dict) = geometry_dict(doc, term.id, dict).ok_or_else(|| {
            QualifyError::Malformed(format!(
                "field {:?} has no /Rect on itself or a widget kid",
                term.fq_name
            ))
        })?;

        // Read the widget /Rect and the owning page's MediaBox, then invert the
        // spine's bottom-left flip back to top-left page-relative.
        let rect_bl = read_rect(geom_dict).ok_or_else(|| {
            QualifyError::Malformed(format!("field {:?} has no /Rect", term.fq_name))
        })?;
        let page = owning_page(doc, geom_id, geom_dict, &page_index);
        let media_box = page_index
            .get(page)
            .map(|(_, mb)| *mb)
            .ok_or_else(|| QualifyError::Malformed("page has no resolvable MediaBox".into()))?;
        let rect = invert_flip(rect_bl, media_box);

        // `/TU` is a field attribute: prefer the field node, fall back to the
        // widget dict (which, in the merged case, is the same dict).
        let tooltip = dict
            .get(b"TU")
            .ok()
            .and_then(|o| o.as_str().ok())
            .or_else(|| geom_dict.get(b"TU").ok().and_then(|o| o.as_str().ok()))
            .map(decode_pdf_text);

        let schema_field = match kind {
            FieldKind::Signature => None,
            _ => Some(snake_case(&term.fq_name)),
        };

        fields.push(FormField {
            name: term.fq_name,
            schema_field,
            page,
            rect,
            tooltip,
            kind,
        });
    }

    Ok(fields)
}

/// Classify a terminal field's `/FT` + `/Ff` into a [`FieldKind`]. Returns
/// `Ok(None)` for a skipped shape (pushbutton / radio / unknown `/FT`).
fn classify(term: &TerminalField) -> Result<Option<FieldKind>, QualifyError> {
    let ft = term.ft.as_deref();
    Ok(match ft {
        Some(b"Tx") => Some(FieldKind::Text {
            multiline: term.ff & FF_MULTILINE != 0,
        }),
        Some(b"Btn") => {
            if term.ff & FF_PUSHBUTTON != 0 {
                // A pushbutton is not a fillable form field.
                None
            } else if term.ff & FF_RADIO != 0 {
                // Radio groups are deferred (proposal §8): reintroduce an
                // on-state model first. Skip with no error.
                None
            } else {
                Some(FieldKind::Checkbox)
            }
        }
        // Options are filled from `/Opt` by `finalize_choice_options` once the
        // terminal's dict is in hand (`classify` only sees flags/type).
        Some(b"Ch") => Some(FieldKind::Choice {
            options: Vec::new(),
        }),
        Some(b"Sig") => Some(FieldKind::Signature),
        // No /FT at all, or an /FT we don't model → skip.
        _ => None,
    })
}

/// Resolve the dict that carries a terminal field's geometry (`/Rect` + `/P`),
/// plus that dict's object id, returning `None` if neither the field node nor a
/// widget kid has a `/Rect`.
///
/// Two shapes:
/// - **Merged** (the spine's output): the field node itself carries `/Rect` —
///   return `(field_id, field_dict)`.
/// - **Split** (Acrobat / gov forms): the field node has `/T`+`/FT` but no
///   `/Rect`; geometry lives on a widget-annotation kid (`/Subtype /Widget`,
///   has `/Rect`, no `/T`) — return that kid's `(id, dict)`.
///
/// V1 limitation: a field with MULTIPLE widget kids (the same field placed on
/// several pages — rare for text/checkbox/choice; radio is already skipped via
/// `/Ff`) uses the FIRST widget kid's geometry. Multi-widget / continuation
/// placement is deferred per the proposal; this never errors.
fn geometry_dict<'a>(
    doc: &'a Document,
    field_id: ObjectId,
    field_dict: &'a Dictionary,
) -> Option<(ObjectId, &'a Dictionary)> {
    // Merged: the field node carries its own /Rect.
    if field_dict.has(b"Rect") {
        return Some((field_id, field_dict));
    }
    // Split: find the first widget-annotation kid with a /Rect.
    let kids = field_dict.get(b"Kids").ok()?.as_array().ok()?;
    for kid in kids {
        let Ok(kid_id) = kid.as_reference() else {
            continue;
        };
        let Some(kid_dict) = doc.get_object(kid_id).ok().and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        // The geometry-bearing kid is a widget annotation with a `/Rect`. We key
        // on `/Rect` (the actual signal); a missing `/Subtype /Widget` is
        // tolerated, since some producers omit it on a single-widget field.
        if kid_dict.has(b"Rect") {
            return Some((kid_id, kid_dict));
        }
    }
    None
}

/// Read a widget's `/Rect` as a normalized bottom-left `[x0, y0, x1, y1]`
/// (unordered corners reordered so `x0 <= x1`, `y0 <= y1`).
fn read_rect(dict: &Dictionary) -> Option<[f32; 4]> {
    let arr = dict.get(b"Rect").ok()?.as_array().ok()?;
    if arr.len() != 4 {
        return None;
    }
    let mut n = [0.0f32; 4];
    for (i, o) in arr.iter().enumerate() {
        n[i] = o.as_float().ok()?;
    }
    Some([
        n[0].min(n[2]),
        n[1].min(n[3]),
        n[0].max(n[2]),
        n[1].max(n[3]),
    ])
}

/// Invert the spine's top-left → bottom-left flip
/// (`resolve.rs::flip_rect`). Given a bottom-left widget rect and the owning
/// page's normalized MediaBox `[mb0, mb1, mb2, mb3]`, recover the top-left
/// page-relative `{x, y, w, h}`:
///
/// ```text
/// x = x0 - mb0
/// y = mb3 - y1
/// w = x1 - x0
/// h = y1 - y0
/// ```
fn invert_flip(rect_bl: [f32; 4], media_box: [f32; 4]) -> Rect {
    let [x0, y0, x1, y1] = rect_bl;
    let mb0 = media_box[0];
    let mb3 = media_box[3];
    Rect {
        x: x0 - mb0,
        y: mb3 - y1,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// A flat list of `(page_object_id, normalized_media_box)` in document order;
/// the index into the vec is the 0-based page number.
type PageIndex = Vec<(ObjectId, [f32; 4])>;

/// Build the page index in document order, resolving inherited `/MediaBox`.
fn build_page_index(doc: &Document) -> PageIndex {
    // `get_pages` returns page-number → object-id in order.
    let pages = doc.get_pages();
    let mut out = Vec::with_capacity(pages.len());
    for (_, page_id) in pages {
        let mb = resolve_media_box(doc, page_id).unwrap_or([0.0, 0.0, 612.0, 792.0]);
        out.push((page_id, mb));
    }
    out
}

/// Resolve a page's `/MediaBox`, walking up `/Parent` for the inherited case,
/// normalized to `[x0, y0, x1, y1]`.
fn resolve_media_box(doc: &Document, page_id: ObjectId) -> Option<[f32; 4]> {
    let mut cur = Some(page_id);
    let mut guard = 0;
    while let Some(id) = cur {
        guard += 1;
        if guard > 64 {
            break;
        }
        let dict = doc.get_object(id).ok()?.as_dict().ok()?;
        if let Ok(arr) = dict.get(b"MediaBox").and_then(|o| o.as_array()) {
            if arr.len() == 4 {
                let mut n = [0.0f32; 4];
                for (i, o) in arr.iter().enumerate() {
                    n[i] = o.as_float().ok()?;
                }
                return Some([
                    n[0].min(n[2]),
                    n[1].min(n[3]),
                    n[0].max(n[2]),
                    n[1].max(n[3]),
                ]);
            }
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

/// Determine the 0-based owning page of a widget: prefer its `/P` entry matched
/// against the page list, else the page whose `/Annots` contains the widget id.
/// Defaults to page 0.
fn owning_page(doc: &Document, widget_id: ObjectId, dict: &Dictionary, pages: &PageIndex) -> usize {
    if let Ok(p) = dict.get(b"P").and_then(|o| o.as_reference()) {
        if let Some(idx) = pages.iter().position(|(id, _)| *id == p) {
            return idx;
        }
    }
    for (idx, (page_id, _)) in pages.iter().enumerate() {
        let annots = doc
            .get_object(*page_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Annots").ok().cloned());
        if let Some(annots) = annots {
            // /Annots may itself be an indirect ref.
            let arr = doc
                .dereference(&annots)
                .ok()
                .and_then(|(_, o)| o.as_array().ok().cloned());
            if let Some(arr) = arr {
                if arr.iter().any(|o| o.as_reference().ok() == Some(widget_id)) {
                    return idx;
                }
            }
        }
    }
    0
}

/// Decode a PDF text string (`/TU`, `/T`): a UTF-16BE-with-BOM hex/literal
/// string decodes to its code points; otherwise it is treated as PDFDocEncoding
/// (ASCII-compatible for the common case) via a lossy UTF-8 read.
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Turn a (possibly fully-qualified, CamelCase) field name into a snake_case
/// binding suggestion: `FullName` → `full_name`, `FavoriteColor` →
/// `favorite_color`, `full_name` unchanged. Non-alphanumerics map to `_`
/// (collapsed, trimmed). A CASE-boundary inserts a `_`. This is a *suggested*
/// binding the quill author refines.
fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower_or_digit = false;
    for ch in s.chars() {
        if ch.is_ascii_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_lower_or_digit = true;
        } else {
            // Non-alphanumeric (including `.`): a separator.
            out.push('_');
            prev_lower_or_digit = false;
        }
    }
    // Collapse runs of `_` and trim leading/trailing.
    let mut collapsed = String::with_capacity(out.len());
    let mut last_us = false;
    for ch in out.chars() {
        if ch == '_' {
            if !last_us {
                collapsed.push('_');
            }
            last_us = true;
        } else {
            collapsed.push(ch);
            last_us = false;
        }
    }
    collapsed.trim_matches('_').to_string()
}

/// Strip `/AcroForm`, every widget/field object, and each page's `/Annots`, then
/// force traditional-xref output so the result satisfies the spine's input
/// contract.
fn strip_background(mut doc: Document) -> Result<Vec<u8>, QualifyError> {
    // 1. Collect every object id reachable from /AcroForm /Fields (the field
    //    tree + its widget kids) so we can delete them after dropping /AcroForm.
    let mut to_delete: Vec<ObjectId> = Vec::new();
    if let Ok(cat) = doc.catalog() {
        if let Some(af) = cat
            .get(b"AcroForm")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
        {
            // The /AcroForm itself, if it's an indirect object.
            if let Ok(af_ref) = cat.get(b"AcroForm").and_then(|o| o.as_reference()) {
                to_delete.push(af_ref);
            }
            if let Ok(af_dict) = af.1.as_dict() {
                if let Ok(arr) = af_dict.get(b"Fields").and_then(|o| o.as_array()) {
                    let roots: Vec<ObjectId> =
                        arr.iter().filter_map(|o| o.as_reference().ok()).collect();
                    for r in roots {
                        collect_subtree(&doc, r, &mut to_delete);
                    }
                }
            }
        }
    }

    // 2. Remove /AcroForm from the catalog.
    if let Ok(cat) = doc.catalog_mut() {
        cat.remove(b"AcroForm");
    }

    // 3. Remove each page's /Annots entirely. (We drop the whole array; in this
    //    contract a gov form's page annotations are exactly the widgets, so the
    //    stripped background is pure pages + content.)
    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for page_id in page_ids {
        if let Ok(page) = doc.get_object_mut(page_id).and_then(Object::as_dict_mut) {
            page.remove(b"Annots");
        }
    }

    // 4. Delete the field/widget objects.
    for id in to_delete {
        doc.objects.remove(&id);
    }

    // 5. Drop /Encrypt (gone after decrypt, but be defensive) and trailer keys
    //    that belong to an xref *stream* dict, not a traditional trailer. When
    //    the source used a cross-reference stream, lopdf folds that stream's
    //    dict into `doc.trailer`, leaving stale `/Type /XRef` / `/W` / `/Index`
    //    keys that have no place in a traditional trailer; remove them so the
    //    re-serialized trailer is clean.
    doc.trailer.remove(b"Encrypt");
    for key in [
        b"Type".as_slice(),
        b"W",
        b"Index",
        b"XRefStm",
        b"Length",
        b"Filter",
        b"DecodeParms",
    ] {
        doc.trailer.remove(key);
    }

    // 6. FORCE traditional xref output. lopdf defaults the save's xref type to
    //    whatever the source used; a source with an xref *stream* would emit one
    //    again and violate `assert_traditional_xref`. Force the table form.
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;

    let mut buf = Vec::new();
    doc.save_to(&mut buf)
        .map_err(|e| QualifyError::Internal(format!("save failed: {e}")))?;
    Ok(buf)
}

/// Collect `root` and all `/Kids` (transitively) into `acc` — the field-tree
/// objects to delete when stripping.
fn collect_subtree(doc: &Document, root: ObjectId, acc: &mut Vec<ObjectId>) {
    let mut stack = vec![root];
    let mut guard = 0;
    while let Some(id) = stack.pop() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        if acc.contains(&id) {
            continue;
        }
        acc.push(id);
        if let Some(dict) = doc.get_object(id).ok().and_then(|o| o.as_dict().ok()) {
            if let Ok(kids) = dict.get(b"Kids").and_then(|o| o.as_array()) {
                for k in kids {
                    if let Ok(kid) = k.as_reference() {
                        stack.push(kid);
                    }
                }
            }
        }
    }
}

/// Read `/Opt` for a choice field. Each entry is either a display string or a
/// `[export, display]` pair — we take the DISPLAY string. Absent `/Opt` yields
/// empty options.
fn read_choice_options(dict: &Dictionary) -> Vec<String> {
    let Ok(opt) = dict.get(b"Opt").and_then(|o| o.as_array()) else {
        return Vec::new();
    };
    opt.iter()
        .filter_map(|entry| match entry {
            Object::String(s, _) => Some(decode_pdf_text(s)),
            Object::Array(pair) => {
                // [export, display] — take display (index 1), fall back to
                // export (index 0).
                pair.get(1)
                    .or_else(|| pair.first())
                    .and_then(|o| o.as_str().ok())
                    .map(decode_pdf_text)
            }
            _ => None,
        })
        .collect()
}

// `classify` builds `Choice { options: empty }`; the real options need the
// terminal's dict, which `classify` doesn't hold. `extract_fields` fills them
// in via this helper so the public surface stays a single pass.
fn finalize_choice_options(kind: &mut FieldKind, dict: &Dictionary) {
    if let FieldKind::Choice { options } = kind {
        *options = read_choice_options(dict);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    use lopdf::{Object, StringFormat};

    fn name(b: &[u8]) -> Object {
        Object::Name(b.to_vec())
    }
    fn pdf_str(s: &str) -> Object {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    }

    #[test]
    fn snake_case_examples() {
        assert_eq!(snake_case("FullName"), "full_name");
        assert_eq!(snake_case("FavoriteColor"), "favorite_color");
        assert_eq!(snake_case("full_name"), "full_name");
        assert_eq!(snake_case("section.name"), "section_name");
        assert_eq!(snake_case("Field 1"), "field_1");
        assert_eq!(snake_case("HTTPServer"), "httpserver");
    }

    #[test]
    fn invert_flip_zero_origin() {
        // Mirror of resolve.rs `rect_flip_is_bottom_left`: a 14×14 box at
        // top-left (180, 90) on an 800-tall, zero-origin page becomes the
        // bottom-left rect [180, 696, 194, 710]; inverting it must recover the
        // original {x:180, y:90, w:14, h:14}.
        let bl = [180.0, 800.0 - 104.0, 194.0, 800.0 - 90.0];
        let r = invert_flip(bl, [0.0, 0.0, 600.0, 800.0]);
        assert_eq!(
            r,
            Rect {
                x: 180.0,
                y: 90.0,
                w: 14.0,
                h: 14.0
            }
        );
    }

    #[test]
    fn invert_flip_nonzero_origin() {
        // Mirror of resolve.rs `rect_flip_honours_nonzero_origin`.
        let mb = [10.0, 20.0, 622.0, 812.0];
        let bl = [10.0 + 180.0, 812.0 - 114.0, 10.0 + 194.0, 812.0 - 100.0];
        let r = invert_flip(bl, mb);
        assert_eq!(
            r,
            Rect {
                x: 180.0,
                y: 100.0,
                w: 14.0,
                h: 14.0
            }
        );
    }

    #[test]
    fn classify_field_type_table() {
        let tx = |ff: i64| TerminalField {
            id: (0, 0),
            fq_name: "x".into(),
            ft: Some(b"Tx".to_vec()),
            ff,
        };
        assert_eq!(
            classify(&tx(0)).unwrap(),
            Some(FieldKind::Text { multiline: false })
        );
        assert_eq!(
            classify(&tx(FF_MULTILINE)).unwrap(),
            Some(FieldKind::Text { multiline: true })
        );

        let btn = |ff: i64| TerminalField {
            id: (0, 0),
            fq_name: "x".into(),
            ft: Some(b"Btn".to_vec()),
            ff,
        };
        assert_eq!(classify(&btn(0)).unwrap(), Some(FieldKind::Checkbox));
        assert_eq!(classify(&btn(FF_PUSHBUTTON)).unwrap(), None); // pushbutton skipped
        assert_eq!(classify(&btn(FF_RADIO)).unwrap(), None); // radio skipped

        let ch = TerminalField {
            id: (0, 0),
            fq_name: "x".into(),
            ft: Some(b"Ch".to_vec()),
            ff: 0,
        };
        assert!(matches!(
            classify(&ch).unwrap(),
            Some(FieldKind::Choice { .. })
        ));

        let sig = TerminalField {
            id: (0, 0),
            fq_name: "x".into(),
            ft: Some(b"Sig".to_vec()),
            ff: 0,
        };
        assert_eq!(classify(&sig).unwrap(), Some(FieldKind::Signature));

        // Unknown / missing /FT → skipped.
        let unknown = TerminalField {
            id: (0, 0),
            fq_name: "x".into(),
            ft: None,
            ff: 0,
        };
        assert_eq!(classify(&unknown).unwrap(), None);
    }

    #[test]
    fn choice_options_bare_and_pairs() {
        // Bare display strings.
        let bare = dictionary! {
            "Opt" => vec![pdf_str("red"), pdf_str("green"), pdf_str("blue")],
        };
        assert_eq!(read_choice_options(&bare), vec!["red", "green", "blue"]);

        // [export, display] pairs — DISPLAY taken.
        let pairs = dictionary! {
            "Opt" => vec![
                Object::Array(vec![pdf_str("R"), pdf_str("Red")]),
                Object::Array(vec![pdf_str("G"), pdf_str("Green")]),
            ],
        };
        assert_eq!(read_choice_options(&pairs), vec!["Red", "Green"]);

        // Absent /Opt → empty.
        let none = dictionary! { "FT" => name(b"Ch") };
        assert_eq!(read_choice_options(&none), Vec::<String>::new());
    }
}
