//! Walk a compiled Typst document and return one `SigPlacement` per
//! `signature-field` call. The helper emits a `<__qm_sig__>`-labelled
//! `metadata` whose value carries `(kind, name, width, height)`, followed by
//! an invisible same-sized box. Metadata has zero size, so its
//! `introspector.position()` equals the box's top-left — no frame walk.

use std::collections::HashMap;

use typst::foundations::{Label, Selector, Value};
use typst::introspection::Location;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;
use typst::Document;

use quillmark_core::{Diagnostic, RenderError, Severity};

use super::{err, SigPlacement};

const SIG_LABEL: &str = "__qm_sig__";
const CODE_INTERNAL: &str = "typst::sig_overlay_internal";

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<SigPlacement>, RenderError> {
    let intro = doc.introspector();
    let label = Label::new(PicoStr::intern(SIG_LABEL))
        .ok_or_else(|| err(CODE_INTERNAL, "SIG_LABEL must be a non-empty interned string"))?;
    let elems = intro.query(&Selector::Label(label));
    if elems.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_name: HashMap<String, Location> = HashMap::new();
    let mut placements: Vec<SigPlacement> = Vec::with_capacity(elems.len());

    for c in elems.iter() {
        let dict = match c.get_by_name("value") {
            Ok(Value::Dict(d)) => d,
            Ok(other) => {
                return Err(err(
                    CODE_INTERNAL,
                    format!("expected metadata value to be a dict, got {}", other.ty()),
                ))
            }
            Err(e) => return Err(err(CODE_INTERNAL, format!("metadata.value missing: {e:?}"))),
        };
        if read_str(&dict, "kind")? != SIG_LABEL {
            // User attached <__qm_sig__> to unrelated metadata; ignore it.
            continue;
        }
        let name = read_str(&dict, "name")?;
        let width = read_f64(&dict, "width")?;
        let height = read_f64(&dict, "height")?;
        let loc = c
            .location()
            .ok_or_else(|| err(CODE_INTERNAL, "signature-field metadata is not located"))?;

        if let Some(&prior) = by_name.get(&name) {
            return Err(duplicate_field_error(&name, prior, loc));
        }
        by_name.insert(name.clone(), loc);

        let pos = intro.position(loc);
        placements.push(SigPlacement {
            name,
            page: pos.page.get().saturating_sub(1),
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
        Ok(other) => Err(err(
            CODE_INTERNAL,
            format!("expected metadata.{key} to be str, got {}", other.ty()),
        )),
        Err(_) => Err(err(CODE_INTERNAL, format!("metadata.{key} missing"))),
    }
}

fn read_f64(d: &typst::foundations::Dict, key: &str) -> Result<f64, RenderError> {
    match d.get(key) {
        Ok(Value::Float(f)) => Ok(*f),
        Ok(Value::Int(i)) => Ok(*i as f64),
        Ok(other) => Err(err(
            CODE_INTERNAL,
            format!("expected metadata.{key} to be float, got {}", other.ty()),
        )),
        Err(_) => Err(err(CODE_INTERNAL, format!("metadata.{key} missing"))),
    }
}

/// Quote the name first so downstream parsers can extract it with a stable
/// first-quoted-token convention.
fn duplicate_field_error(name: &str, first: Location, second: Location) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(
            Severity::Error,
            format!("{name:?} is defined twice: each signature-field name must be unique"),
        )
        .with_code("typst::duplicate_signature_field".to_string())
        .with_hint(format!(
            "Rename one of the calls. Conflicting Typst location ids: {first:?}, {second:?}"
        ))],
    }
}
