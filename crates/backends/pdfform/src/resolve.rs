//! The resolver: the bind step that turns a value-free [`FormField`] plus the
//! document's `compile_data` JSON into a stamp-spine [`FieldSpec`].
//!
//! Binding is against `compile_data` — the same validated, zero-filled object
//! the Typst plate reads as `data.*` — so zero-fill, schema validation,
//! defaults, and scalar coercion are inherited, not re-implemented. V1
//! addressing is a shallow path: a root field name, optionally followed by an
//! array index or nested key (`field`, `field.0`, `field.sub`). Coercion is
//! type-directed; unbound or absent/null both render a blank field.

use quillmark_pdf::{FieldSpec, FieldType, CHECKBOX_ON_STATE};
use serde_json::Value;

use crate::form::{FieldKind, FormField, Rect};

/// Build a [`FieldSpec`] for `field`, flipping its top-left rect to bottom-left
/// against the page's `media_box` and resolving its bound value from `data`.
///
/// `media_box` is the normalized `[x0, y0, x1, y1]` of the target page.
pub fn field_spec(field: &FormField, media_box: [f32; 4], data: &Value) -> FieldSpec {
    FieldSpec {
        name: field.name.clone(),
        page: field.page,
        rect: flip_rect(field.rect, media_box),
        field_type: field_type(&field.kind),
        value: resolve_value(&field.kind, field.schema_field.as_deref(), data),
        tooltip: field.tooltip.clone(),
    }
}

/// Page-relative top-left `{x,y,w,h}` → spine bottom-left `[x0, y0, x1, y1]` in
/// PDF user space. Honours a non-zero page origin: the left edge is the
/// MediaBox `x0` and `y` is measured down from the top edge (MediaBox `y1`), so
/// a translated MediaBox (e.g. `[10 20 622 812]`) places widgets correctly
/// rather than shifting them by the origin. This is the single biggest
/// hand-authoring footgun, defused structurally in one place.
fn flip_rect(r: Rect, media_box: [f32; 4]) -> [f32; 4] {
    let left = media_box[0];
    let top = media_box[3];
    [left + r.x, top - (r.y + r.h), left + r.x + r.w, top - r.y]
}

fn field_type(kind: &FieldKind) -> FieldType {
    match kind {
        FieldKind::Text { multiline } => FieldType::Text {
            multiline: *multiline,
        },
        FieldKind::Checkbox => FieldType::Checkbox,
        FieldKind::Choice { options } => FieldType::Choice {
            options: options.clone(),
        },
        FieldKind::Signature => FieldType::Signature,
    }
}

/// Resolve a field's bound value. `None` (blank) for: an unbound field
/// (`schema_field: None`), an absent/null target, a signature, an empty text
/// value, an unchecked checkbox, or a choice value matching no option.
fn resolve_value(kind: &FieldKind, schema_field: Option<&str>, data: &Value) -> Option<String> {
    if matches!(kind, FieldKind::Signature) {
        return None;
    }
    let raw = lookup(data, schema_field?)?;
    match kind {
        FieldKind::Text { .. } => coerce_text(raw),
        FieldKind::Checkbox => is_truthy(raw).then(|| CHECKBOX_ON_STATE.to_string()),
        FieldKind::Choice { options } => coerce_choice(raw, options),
        FieldKind::Signature => None,
    }
}

/// Dereference a shallow `field[.<index-or-key>]*` path against `data`. Returns
/// `None` for any missing segment.
fn lookup<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut cur = data.get(parts.next()?)?;
    for seg in parts {
        cur = match seg.parse::<usize>() {
            Ok(idx) => cur.get(idx)?,
            Err(_) => cur.get(seg)?,
        };
    }
    Some(cur)
}

/// Coerce a JSON value to display text. Empty results (empty string, all-null
/// array) become `None` so the widget carries no `/V`.
fn coerce_text(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        // An array (e.g. a `markdown[]` or `string[]` field) joins its string
        // elements with newlines — the multiline text fill.
        Value::Array(arr) => arr
            .iter()
            .filter_map(|e| match e {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null | Value::Object(_) => return None,
    };
    (!s.is_empty()).then_some(s)
}

/// Truthiness for a checkbox binding. A boolean schema field coerces to a JSON
/// bool, the common path; strings and numbers are handled defensively.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1" | "y" | "checked"
        ),
        _ => false,
    }
}

/// A choice value binds only if it matches one of the declared options exactly.
fn coerce_choice(v: &Value, options: &[String]) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    options.iter().any(|o| o == &s).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> Value {
        json!({
            "full_name": "Ada Lovelace",
            "comments": ["line one", "line two"],
            "agree": true,
            "decline": false,
            "favorite_color": "green",
            "bad_color": "purple",
            "empty": "",
            "score": 42
        })
    }

    fn text(field: &str) -> Option<String> {
        resolve_value(&FieldKind::Text { multiline: false }, Some(field), &data())
    }

    #[test]
    fn text_binds_scalar_and_joins_arrays() {
        assert_eq!(text("full_name"), Some("Ada Lovelace".into()));
        assert_eq!(
            resolve_value(
                &FieldKind::Text { multiline: true },
                Some("comments"),
                &data()
            ),
            Some("line one\nline two".into())
        );
        assert_eq!(text("score"), Some("42".into()));
    }

    #[test]
    fn array_index_addressing() {
        assert_eq!(text("comments.0"), Some("line one".into()));
        assert_eq!(text("comments.1"), Some("line two".into()));
        assert_eq!(text("comments.9"), None);
    }

    #[test]
    fn empty_and_absent_are_blank() {
        assert_eq!(text("empty"), None);
        assert_eq!(text("does_not_exist"), None);
    }

    #[test]
    fn unbound_is_blank() {
        assert_eq!(
            resolve_value(&FieldKind::Text { multiline: false }, None, &data()),
            None
        );
    }

    #[test]
    fn checkbox_truthiness() {
        let on = |f| resolve_value(&FieldKind::Checkbox, Some(f), &data());
        assert_eq!(on("agree"), Some(CHECKBOX_ON_STATE.to_string()));
        assert_eq!(on("decline"), None);
        assert_eq!(on("missing"), None);
    }

    #[test]
    fn choice_must_match_option() {
        let opts = vec!["red".to_string(), "green".to_string(), "blue".to_string()];
        let kind = FieldKind::Choice { options: opts };
        assert_eq!(
            resolve_value(&kind, Some("favorite_color"), &data()),
            Some("green".into())
        );
        // A value matching no option is dropped to blank.
        assert_eq!(resolve_value(&kind, Some("bad_color"), &data()), None);
    }

    #[test]
    fn signature_never_binds() {
        assert_eq!(
            resolve_value(&FieldKind::Signature, Some("full_name"), &data()),
            None
        );
    }

    #[test]
    fn rect_flip_is_bottom_left() {
        // A 14×14 box at top-left (180, 90) on an 800-tall, zero-origin page.
        let r = Rect {
            x: 180.0,
            y: 90.0,
            w: 14.0,
            h: 14.0,
        };
        assert_eq!(
            flip_rect(r, [0.0, 0.0, 600.0, 800.0]),
            [180.0, 800.0 - 104.0, 194.0, 800.0 - 90.0]
        );
    }

    #[test]
    fn rect_flip_honours_nonzero_origin() {
        // Same box on a page whose MediaBox is translated to [10 20 622 812].
        // Widgets must land offset by the origin, not at a (0,0) origin.
        let r = Rect {
            x: 180.0,
            y: 100.0,
            w: 14.0,
            h: 14.0,
        };
        let mb = [10.0, 20.0, 622.0, 812.0]; // top edge y1 = 812
        assert_eq!(
            flip_rect(r, mb),
            [10.0 + 180.0, 812.0 - 114.0, 10.0 + 194.0, 812.0 - 100.0]
        );
    }
}
