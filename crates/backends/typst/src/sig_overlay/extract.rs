//! Walk a compiled Typst document, find every `signature-field` call, and
//! return `Vec<SigPlacement>` in Typst (top-left origin) coordinates.
//!
//! Authors invoke `#signature-field("approver", width: 200pt, height: 50pt)`
//! from `quillmark-helper`. The helper emits a metadata node labelled
//! `<qm-sig>` whose `value` is a dict `(kind: "qm-sig", name, width, height)`,
//! followed by an invisible same-sized box.
//!
//! The metadata's introspector position equals the box's top-left because
//! metadata has zero size and the box lays out immediately after it. We use
//! the position from `introspector.position()` and the dimensions from the
//! metadata value — no frame walk is needed. See `probe_annots::probe_p5`.

use std::collections::HashMap;

use typst::foundations::{Label, Selector, Value};
use typst::introspection::Location;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;
use typst::Document;

use quillmark_core::{Diagnostic, RenderError, Severity};

use super::SigPlacement;

/// Static label every `signature-field` invocation tags itself with. The
/// double-underscore convention makes accidental collision with an author's
/// own label virtually impossible — any same-named label in their plate is
/// almost certainly a deliberate hand-off into our extraction pipeline.
const SIG_LABEL: &str = "__qm_sig__";
/// `kind` value embedded in the metadata dict — second line of defence
/// against unrelated metadata getting through `<__qm_sig__>` collisions.
const SIG_KIND: &str = "__qm_sig__";

/// Walk the document and return a `SigPlacement` per `signature-field` call.
///
/// Returns an empty `Vec` if the document contains no calls.
///
/// Errors:
/// - Duplicate field name → `RenderError::CompilationFailed` carrying a
///   `Diagnostic` with code `typst::duplicate_signature_field`.
/// - Malformed helper output (wrong field types, missing keys) →
///   `RenderError::CompilationFailed` with `typst::sig_overlay_internal`.
pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    let intro = doc.introspector();
    let label = Label::new(PicoStr::intern(SIG_LABEL)).ok_or_else(|| internal(
        "invariant: SIG_LABEL must be a non-empty interned string",
    ))?;
    let elems = intro.query(&Selector::Label(label));
    if elems.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_name: HashMap<String, Location> = HashMap::new();
    let mut placements: Vec<SigPlacement> = Vec::with_capacity(elems.len());

    for c in elems.iter() {
        let value = c
            .get_by_name("value")
            .map_err(|e| internal(&format!("metadata.value missing: {e:?}")))?;
        let dict = match value {
            Value::Dict(d) => d,
            other => {
                return Err(internal(&format!(
                    "expected metadata value to be a dict, got {}",
                    other.ty()
                )));
            }
        };

        let kind = read_str(&dict, "kind")?;
        if kind != SIG_KIND {
            // Some other metadata tripped over our internal label — leave
            // it alone. The reserved label name makes this unreachable in
            // practice; the guard is defensive.
            continue;
        }
        let name = read_str(&dict, "name")?;
        let width = read_f64(&dict, "width")?;
        let height = read_f64(&dict, "height")?;

        let loc = c
            .location()
            .ok_or_else(|| internal("signature-field metadata is not located"))?;

        if let Some(prior) = by_name.get(&name) {
            return Err(duplicate_field_error(&name, *prior, loc));
        }
        by_name.insert(name.clone(), loc);

        let pos = intro.position(loc);
        let page_index = pos.page.get().saturating_sub(1);

        placements.push(SigPlacement {
            name,
            page: page_index,
            rect_typst_pt: [
                pos.point.x.to_pt() as f32,
                pos.point.y.to_pt() as f32,
                (pos.point.x.to_pt() + width) as f32,
                (pos.point.y.to_pt() + height) as f32,
            ],
        });
    }

    placements.sort_by(|a, b| (a.page, &a.name).cmp(&(b.page, &b.name)));
    Ok(placements)
}

fn read_str(d: &typst::foundations::Dict, key: &str) -> Result<String, RenderError> {
    match d.get(key) {
        Ok(Value::Str(s)) => Ok(s.to_string()),
        Ok(other) => Err(internal(&format!(
            "expected metadata.{key} to be str, got {}",
            other.ty()
        ))),
        Err(_) => Err(internal(&format!("metadata.{key} missing"))),
    }
}

fn read_f64(d: &typst::foundations::Dict, key: &str) -> Result<f64, RenderError> {
    match d.get(key) {
        Ok(Value::Float(f)) => Ok(*f),
        Ok(Value::Int(i)) => Ok(*i as f64),
        Ok(other) => Err(internal(&format!(
            "expected metadata.{key} to be float, got {}",
            other.ty()
        ))),
        Err(_) => Err(internal(&format!("metadata.{key} missing"))),
    }
}

fn internal(msg: &str) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(
            Severity::Error,
            format!("signature-field extract: {msg}"),
        )
        .with_code("typst::sig_overlay_internal".to_string())],
    }
}

fn duplicate_field_error(name: &str, first: Location, second: Location) -> RenderError {
    // Quote the name first in the message so downstream tooling can extract
    // it with a stable regex / first-quoted-token convention.
    let message = format!(
        "{name:?} is defined twice: each signature-field name must be unique"
    );
    let hint = format!(
        "Rename one of the calls. Conflicting Typst location ids: {first:?}, {second:?}"
    );
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, message)
            .with_code("typst::duplicate_signature_field".to_string())
            .with_hint(hint)],
    }
}
