//! Walk a compiled Typst document and return one `FieldPlacement` per
//! `form-field` call. The helper emits a `<__qm_field__>`-labelled `metadata`
//! whose value carries `(kind, name, field-type, value, options, multiline,
//! width, height)`, followed by an invisible same-sized box. Metadata has zero
//! size, so its `introspector.position()` equals the box's top-left — no frame
//! walk.

use std::collections::HashMap;

use typst::foundations::{Label, Selector, Value};
use typst::introspection::{Introspector, Location};
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use quillmark_core::{Diagnostic, RenderError, Severity};

use super::{err, FieldKind, FieldPlacement};

const FIELD_LABEL: &str = "__qm_field__";
const CODE_INTERNAL: &str = "typst::overlay_internal";

pub(crate) fn extract(doc: &PagedDocument) -> Result<Vec<FieldPlacement>, RenderError> {
    let intro = doc.introspector();
    let label = Label::new(PicoStr::intern(FIELD_LABEL)).ok_or_else(|| {
        err(
            CODE_INTERNAL,
            "FIELD_LABEL must be a non-empty interned string",
        )
    })?;
    let elems = intro.query(&Selector::Label(label));
    if elems.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_name: HashMap<String, Location> = HashMap::new();
    let mut placements: Vec<FieldPlacement> = Vec::with_capacity(elems.len());

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
        if read_str(&dict, "kind")? != FIELD_LABEL {
            // User attached <__qm_field__> to unrelated metadata; ignore it.
            continue;
        }
        let name = read_str(&dict, "name")?;
        let schema_field = read_opt_str(&dict, "field")?;
        let field_type = read_str(&dict, "field-type")?;
        let width = read_f64(&dict, "width")?;
        let height = read_f64(&dict, "height")?;
        let kind = read_field_kind(&dict, &field_type)?;
        let loc = c
            .location()
            .ok_or_else(|| err(CODE_INTERNAL, "form-field metadata is not located"))?;

        if let Some(&prior) = by_name.get(&name) {
            return Err(duplicate_field_error(&name, prior, loc));
        }
        by_name.insert(name.clone(), loc);

        let pos = intro
            .position(loc)
            .ok_or_else(|| err(CODE_INTERNAL, "form-field metadata has no position"))?;
        placements.push(FieldPlacement {
            name,
            schema_field,
            page: pos.page.get().saturating_sub(1),
            rect_typst_pt: [
                pos.point.x.to_pt() as f32,
                pos.point.y.to_pt() as f32,
                (pos.point.x.to_pt() + width) as f32,
                (pos.point.y.to_pt() + height) as f32,
            ],
            kind,
        });
    }

    placements.sort_by(|a, b| (a.page, &a.name).cmp(&(b.page, &b.name)));
    Ok(placements)
}

/// Resolve the metadata dict into a [`FieldKind`], reading only the keys that
/// apply to `field_type`. The `value` key is polymorphic — a Typst `Str`,
/// `Bool`, `Int`/`Float` (stringified), or `None` — read via the per-kind
/// helpers below so each kind interprets it correctly.
fn read_field_kind(
    d: &typst::foundations::Dict,
    field_type: &str,
) -> Result<FieldKind, RenderError> {
    match field_type {
        "text" => Ok(FieldKind::Text {
            multiline: read_bool(d, "multiline")?.unwrap_or(false),
            value: read_value_str(d, "value")?,
        }),
        "checkbox" => Ok(FieldKind::Checkbox {
            // A checkbox binds on a truthy Typst bool; a missing/`none`/`false`
            // value leaves it unchecked.
            checked: read_value_bool(d, "value")?.unwrap_or(false),
        }),
        "choice" => Ok(FieldKind::Choice {
            options: read_str_array(d, "options")?,
            value: read_value_str(d, "value")?,
        }),
        "signature" => Ok(FieldKind::Signature),
        other => Err(err(
            CODE_INTERNAL,
            format!("unknown form-field type {other:?}"),
        )),
    }
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

/// Read an optional boolean key (`None` for a missing key, an error for a
/// present-but-wrong-type key). Used for the `multiline` flag.
fn read_bool(d: &typst::foundations::Dict, key: &str) -> Result<Option<bool>, RenderError> {
    match d.get(key) {
        Ok(Value::Bool(b)) => Ok(Some(*b)),
        Ok(other) => Err(err(
            CODE_INTERNAL,
            format!("expected metadata.{key} to be bool, got {}", other.ty()),
        )),
        Err(_) => Ok(None),
    }
}

/// Read an optional string key (`None` for a missing or `none` key, an error
/// for a present-but-wrong-type key). Used for the `field:` schema-path binding.
fn read_opt_str(d: &typst::foundations::Dict, key: &str) -> Result<Option<String>, RenderError> {
    match d.get(key) {
        Ok(Value::Str(s)) => Ok(Some(s.to_string())),
        Ok(Value::None) => Ok(None),
        Ok(other) => Err(err(
            CODE_INTERNAL,
            format!(
                "expected metadata.{key} to be str or none, got {}",
                other.ty()
            ),
        )),
        Err(_) => Ok(None),
    }
}

/// Read an array-of-str key, defaulting to empty for a missing key. Used for
/// choice `options`.
fn read_str_array(d: &typst::foundations::Dict, key: &str) -> Result<Vec<String>, RenderError> {
    match d.get(key) {
        Ok(Value::Array(arr)) => arr
            .iter()
            .map(|v| match v {
                Value::Str(s) => Ok(s.to_string()),
                other => Err(err(
                    CODE_INTERNAL,
                    format!(
                        "expected metadata.{key} elements to be str, got {}",
                        other.ty()
                    ),
                )),
            })
            .collect(),
        Ok(Value::None) => Ok(Vec::new()),
        Ok(other) => Err(err(
            CODE_INTERNAL,
            format!("expected metadata.{key} to be an array, got {}", other.ty()),
        )),
        Err(_) => Ok(Vec::new()),
    }
}

/// Read the polymorphic `value` key as display text (for text/choice fields).
/// A `Str` passes through; an `Int`/`Float` stringifies to its decimal form; a
/// `Bool` stringifies; `None` (or a missing key) yields `None`. An empty string
/// also yields `None` so the widget carries no `/V` (mirrors pdfform's
/// `coerce_text`).
fn read_value_str(d: &typst::foundations::Dict, key: &str) -> Result<Option<String>, RenderError> {
    let s = match d.get(key) {
        Ok(Value::Str(s)) => s.to_string(),
        Ok(Value::Int(i)) => i.to_string(),
        Ok(Value::Float(f)) => f.to_string(),
        Ok(Value::Bool(b)) => b.to_string(),
        Ok(Value::None) | Err(_) => return Ok(None),
        Ok(other) => {
            return Err(err(
                CODE_INTERNAL,
                format!(
                    "expected metadata.{key} to be str/int/float/bool/none, got {}",
                    other.ty()
                ),
            ))
        }
    };
    Ok((!s.is_empty()).then_some(s))
}

/// Read the polymorphic `value` key as a boolean (for checkbox fields). A
/// `Bool` passes through; `None` (or a missing key) yields `None` (unchecked).
fn read_value_bool(d: &typst::foundations::Dict, key: &str) -> Result<Option<bool>, RenderError> {
    match d.get(key) {
        Ok(Value::Bool(b)) => Ok(Some(*b)),
        Ok(Value::None) | Err(_) => Ok(None),
        Ok(other) => Err(err(
            CODE_INTERNAL,
            format!(
                "expected checkbox metadata.{key} to be bool or none, got {}",
                other.ty()
            ),
        )),
    }
}

/// Quote the name first so downstream parsers can extract it with a stable
/// first-quoted-token convention.
fn duplicate_field_error(name: &str, first: Location, second: Location) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(
            Severity::Error,
            format!("{name:?} is defined twice: each form-field name must be unique"),
        )
        .with_code("typst::duplicate_form_field".to_string())
        .with_hint(format!(
            "Rename one of the calls. Conflicting Typst location ids: {first:?}, {second:?}"
        ))],
    }
}
