//! The resolver: the bind step that turns a value-free [`FormField`] plus the
//! document's `compile_data` JSON into a stamp-spine [`FieldSpec`].
//!
//! Binding is against `compile_data` — the same validated, zero-filled object
//! the Typst plate reads as `data.*` — so zero-fill, schema validation,
//! defaults, and scalar coercion are inherited, not re-implemented. Addressing
//! is a shallow path: a root field name, optionally followed by an array index
//! or nested key (`field`, `field.0`, `field.sub`). Coercion is type-directed;
//! unbound or absent/null both render a blank field.
//!
//! ## Card-instance addressing
//!
//! A `schema_field` rooted at the reserved `$cards` key binds to one card
//! instance in the document's `$cards` array (the same array the Typst plate
//! iterates). Two forms, so a fixed-capacity form can lay out repeated card
//! slots:
//!
//! - **By absolute index:** `$cards.<i>.<field>` — the `i`-th card overall
//!   (e.g. `$cards.0.from`).
//! - **By kind + index:** `$cards.<kind>.<i>.<field>` — the `i`-th card whose
//!   `$kind` is `<kind>` (e.g. `$cards.indorsement.1.from` is the second
//!   indorsement). This survives reordering and intervening cards of other
//!   kinds, which absolute indexing does not.
//!
//! Either form descends the remaining path into the chosen card object exactly
//! as a top-level binding would.

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
        schema_field: field.schema_field.clone(),
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
    let raw = lookup(data, schema_field?)?;
    match kind {
        FieldKind::Text { .. } => coerce_text(raw),
        FieldKind::Checkbox => is_truthy(raw).then(|| CHECKBOX_ON_STATE.to_string()),
        FieldKind::Choice { options } => coerce_choice(raw, options),
        FieldKind::Signature => None,
    }
}

/// Dereference a shallow `field[.<index-or-key>]*` path against `data`. A path
/// rooted at the reserved `$cards` key resolves a card instance (by absolute
/// index, or by `$kind` + index) before descending the rest. Returns `None` for
/// any missing segment.
fn lookup<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let root = parts.next()?;
    if root == "$cards" {
        return lookup_card(data, parts);
    }
    descend(data.get(root)?, parts)
}

/// Resolve a `$cards`-rooted path: select one card from the `$cards` array
/// either by absolute index (`$cards.<i>...`) or by kind + index
/// (`$cards.<kind>.<i>...`), then descend the remaining segments into it.
///
/// The first segment is tried as an absolute index *before* being treated as a
/// kind, so **card kinds are expected to be non-numeric**: a `$kind` that is a
/// numeric string can only be reached by its absolute index, never by kind.
fn lookup_card<'a, 'p, I>(data: &'a Value, mut parts: I) -> Option<&'a Value>
where
    I: Iterator<Item = &'p str>,
{
    let cards = data.get("$cards")?.as_array()?;
    let first = parts.next()?;
    let card = if let Ok(i) = first.parse::<usize>() {
        // `$cards.<i>...` — absolute index.
        cards.get(i)?
    } else {
        // `$cards.<kind>.<i>...` — the i-th card of that `$kind`.
        let i: usize = parts.next()?.parse().ok()?;
        cards
            .iter()
            .filter(|c| c.get("$kind").and_then(Value::as_str) == Some(first))
            .nth(i)?
    };
    descend(card, parts)
}

/// Walk the remaining `[.<index-or-key>]*` segments from `start`.
fn descend<'a, 'p, I>(start: &'a Value, parts: I) -> Option<&'a Value>
where
    I: Iterator<Item = &'p str>,
{
    let mut cur = start;
    for seg in parts {
        cur = match seg.parse::<usize>() {
            Ok(idx) => cur.get(idx)?,
            Err(_) => cur.get(seg)?,
        };
    }
    Some(cur)
}

/// Stringify a JSON number the same way the Typst producer does, so the two
/// backends agree on the text they bind for the same value. `serde_json`'s own
/// `Number::to_string` preserves the JSON literal form (`42.0` → `"42.0"`,
/// `1e10` → `"10000000000.0"`), but the Typst side decodes the same JSON to a
/// Typst `Int`/`Float` and prints via Rust's integer/`f64` `Display`, so an
/// integral float renders without the trailing `.0`. Mirror that: float-backed
/// numbers go through `f64` `Display`; integer-backed ones are already aligned.
fn number_to_string(n: &serde_json::Number) -> String {
    if n.is_f64() {
        match n.as_f64() {
            Some(f) => f.to_string(),
            None => n.to_string(),
        }
    } else {
        n.to_string()
    }
}

/// Coerce a JSON value to display text. Empty results (empty string, all-null
/// array) become `None` so the widget carries no `/V`.
fn coerce_text(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => number_to_string(n),
        Value::Bool(b) => b.to_string(),
        // An array (e.g. a `markdown[]` or `string[]` field) joins its string
        // elements with newlines — the multiline text fill.
        Value::Array(arr) => arr
            .iter()
            .filter_map(|e| match e {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(number_to_string(n)),
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
        Value::Number(n) => number_to_string(n),
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
    fn number_stringification_matches_typst_producer() {
        // Integral float literals drop the trailing `.0` (matching the Typst
        // side's f64 Display), so the two backends bind identical text and a
        // choice option like "42" matches a 42.0 value on both.
        assert_eq!(coerce_text(&json!(42.0)), Some("42".into()));
        assert_eq!(coerce_text(&json!(1e10)), Some("10000000000".into()));
        // Integers and genuinely-fractional floats are unchanged.
        assert_eq!(coerce_text(&json!(42)), Some("42".into()));
        assert_eq!(coerce_text(&json!(42.5)), Some("42.5".into()));
        // Choice matching uses the same rule.
        let opts = vec!["42".to_string()];
        assert_eq!(coerce_choice(&json!(42.0), &opts), Some("42".into()));
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

    /// A mixed-kind `$cards` array: two indorsements with a note between them,
    /// so by-kind indexing must skip the note that absolute indexing would not.
    fn card_data() -> Value {
        json!({
            "$cards": [
                { "$kind": "indorsement", "from": "Alice", "agree": true },
                { "$kind": "note",        "from": "ignored" },
                { "$kind": "indorsement", "from": "Bob",   "agree": false }
            ]
        })
    }

    fn card_text(path: &str) -> Option<String> {
        resolve_value(
            &FieldKind::Text { multiline: false },
            Some(path),
            &card_data(),
        )
    }

    #[test]
    fn card_absolute_index() {
        assert_eq!(card_text("$cards.0.from"), Some("Alice".into()));
        assert_eq!(card_text("$cards.1.from"), Some("ignored".into()));
        assert_eq!(card_text("$cards.2.from"), Some("Bob".into()));
        // Past the end → blank.
        assert_eq!(card_text("$cards.3.from"), None);
    }

    #[test]
    fn card_by_kind_index() {
        // The i-th card OF THAT KIND, skipping intervening cards of other kinds.
        assert_eq!(card_text("$cards.indorsement.0.from"), Some("Alice".into()));
        assert_eq!(card_text("$cards.indorsement.1.from"), Some("Bob".into()));
        assert_eq!(card_text("$cards.note.0.from"), Some("ignored".into()));
        // Only two indorsements exist.
        assert_eq!(card_text("$cards.indorsement.2.from"), None);
        // No card of this kind.
        assert_eq!(card_text("$cards.memo.0.from"), None);
    }

    #[test]
    fn card_coercion_runs_per_field_kind() {
        // A checkbox bound to a card field coerces truthiness like any other.
        let agree = |path| resolve_value(&FieldKind::Checkbox, Some(path), &card_data());
        assert_eq!(
            agree("$cards.indorsement.0.agree"),
            Some(CHECKBOX_ON_STATE.to_string())
        );
        assert_eq!(agree("$cards.indorsement.1.agree"), None); // Bob: false
    }

    #[test]
    fn card_malformed_paths_are_blank() {
        // Missing index after a kind, a bare `$cards`, and a missing field.
        assert_eq!(card_text("$cards.indorsement"), None);
        assert_eq!(card_text("$cards"), None);
        assert_eq!(card_text("$cards.0.missing"), None);
    }

    #[test]
    fn is_truthy_string_and_number_variants() {
        // Beyond the common JSON-bool path: the defensive string forms (any
        // case, surrounding whitespace) and non-zero numbers read as truthy.
        for s in ["true", "Yes", " ON ", "1", "y", "Checked"] {
            assert!(is_truthy(&json!(s)), "{s:?} should be truthy");
        }
        for s in ["false", "no", "0", "", "maybe", "off"] {
            assert!(!is_truthy(&json!(s)), "{s:?} should be falsy");
        }
        assert!(is_truthy(&json!(42)));
        assert!(is_truthy(&json!(-1)));
        assert!(!is_truthy(&json!(0)));
        // Non-scalars are never truthy.
        assert!(!is_truthy(&json!(null)));
        assert!(!is_truthy(&json!([true])));
        assert!(!is_truthy(&json!({"a": 1})));
    }

    #[test]
    fn coerce_text_array_filters_non_string_elements() {
        // Mixed array: nulls and objects are dropped; scalars join with newlines.
        assert_eq!(
            coerce_text(&json!([null, "a", { "x": 1 }, 2, true])),
            Some("a\n2\ntrue".into())
        );
        // An all-null (or otherwise empty-after-filter) array → None, so the
        // widget carries no `/V`.
        assert_eq!(coerce_text(&json!([null, { "x": 1 }])), None);
        assert_eq!(coerce_text(&json!([])), None);
    }

    #[test]
    fn numeric_card_kind_cannot_be_addressed_by_kind() {
        // KNOWN LIMITATION: `lookup_card` tries `parse::<usize>()` on the first
        // segment first, so a `$kind` that is a numeric string is unreachable by
        // kind — `$cards.<n>...` is always read as an absolute index. Card kinds
        // are therefore expected to be non-numeric.
        let data = json!({
            "$cards": [
                { "$kind": "note", "from": "first" },
                { "$kind": "2",    "from": "numeric-kind" }
            ]
        });
        let by = |path| resolve_value(&FieldKind::Text { multiline: false }, Some(path), &data);
        // `$cards.2.from` is read as absolute index 2 (out of range) — NOT as
        // "kind \"2\", index <next>".
        assert_eq!(by("$cards.2.from"), None);
        // The numeric-kind card is only reachable by its absolute index.
        assert_eq!(by("$cards.1.from"), Some("numeric-kind".into()));
    }

    /// Card slots on a STATIC multi-page form, end-to-end through `field_spec`:
    /// a form with one card slot per page binds each card INSTANCE to its own
    /// page via card-instance addressing. Two cards of one kind, two slots on two
    /// different pages — instance 0's value must land on page 0 and instance 1's
    /// on page 1, each as a full `FieldSpec`. (The form's page set is fixed;
    /// page composition / continuation is out of scope.)
    #[test]
    fn card_instances_bind_to_their_static_form_pages() {
        // ≥2 cards of one kind in `$cards` (the same array the Typst plate reads).
        let data = json!({
            "$cards": [
                { "$kind": "indorsement", "from": "Alice" },
                { "$kind": "indorsement", "from": "Bob" }
            ]
        });

        // Two card slots, one per page, each bound to a distinct instance index.
        let mb = [0.0, 0.0, 612.0, 792.0];
        let slot = |name: &str, page: usize, schema_field: &str| FormField {
            name: name.into(),
            schema_field: Some(schema_field.into()),
            page,
            rect: Rect {
                x: 100.0,
                y: 100.0,
                w: 200.0,
                h: 20.0,
            },
            tooltip: None,
            kind: FieldKind::Text { multiline: false },
        };
        let slot0 = slot("Indorsement0From", 0, "$cards.indorsement.0.from");
        let slot1 = slot("Indorsement1From", 1, "$cards.indorsement.1.from");

        let spec0 = field_spec(&slot0, mb, &data);
        let spec1 = field_spec(&slot1, mb, &data);

        // Instance 0's value on page 0; instance 1's value on page 1.
        assert_eq!(spec0.page, 0, "first slot is on page 0");
        assert_eq!(spec0.value.as_deref(), Some("Alice"), "instance 0 value");
        assert_eq!(spec1.page, 1, "second slot is on page 1");
        assert_eq!(spec1.value.as_deref(), Some("Bob"), "instance 1 value");

        // The names carry through unchanged (the spine writes them to /T).
        assert_eq!(spec0.name, "Indorsement0From");
        assert_eq!(spec1.name, "Indorsement1From");
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
