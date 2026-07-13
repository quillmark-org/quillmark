//! The value step: turn a value-free [`BoundWidget`] plus the document's
//! `compile_data` JSON into a stamp-spine [`FieldSpec`]. Intrinsics (kind,
//! options, multiline, tooltip) and final geometry were already resolved from
//! the static inputs at load ([`crate::bind`]); this module resolves only the
//! per-document *value* and copies the rest through.
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
//! iterates), by kind + index: `$cards.<kind>.<i>.<field>` — the `i`-th card
//! whose `$kind` is `<kind>` (e.g. `$cards.indorsement.1.from` is the second
//! indorsement). This survives reordering and intervening cards of other kinds.
//! Absolute-index addressing (`$cards.<i>`) was retired in `form@0.2.0`: a
//! widget kind must be statically derivable at load, and only the kind names the
//! field. The path descends the remaining segments into the chosen card exactly
//! as a top-level binding would.

use quillmark_pdf::{FieldSpec, FieldType, CHECKBOX_ON_STATE};
use serde_json::Value;

use crate::bind::BoundWidget;

/// Build a [`FieldSpec`] for `widget`: copy its already-final identity, geometry,
/// and kind through, and resolve its bound value from `data`.
pub fn field_spec(widget: &BoundWidget, data: &Value) -> FieldSpec {
    FieldSpec {
        name: widget.name.clone(),
        schema_field: widget.schema_field.clone(),
        page: widget.page,
        rect: widget.rect,
        field_type: widget.field_type.clone(),
        value: resolve_value(&widget.field_type, widget.schema_field.as_deref(), data),
        tooltip: widget.tooltip.clone(),
    }
}

/// Resolve a widget's bound value. `None` (blank) for: an unbound widget
/// (`schema_field: None`), an absent/null target, a signature, an empty text
/// value, an unchecked checkbox, or a choice value matching no option.
fn resolve_value(field_type: &FieldType, schema_field: Option<&str>, data: &Value) -> Option<String> {
    let raw = lookup(data, schema_field?)?;
    match field_type {
        FieldType::Text { .. } => coerce_text(raw),
        FieldType::Checkbox => is_truthy(raw).then(|| CHECKBOX_ON_STATE.to_string()),
        FieldType::Choice { options } => coerce_choice(raw, options),
        FieldType::Signature => None,
    }
}

/// Dereference a shallow `field[.<index-or-key>]*` path against `data`. A path
/// rooted at the reserved `$cards` key resolves a card instance (by `$kind` +
/// index) before descending the rest. Returns `None` for any missing segment.
fn lookup<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let root = parts.next()?;
    if root == "$cards" {
        return lookup_card(data, parts);
    }
    descend(data.get(root)?, parts)
}

/// Resolve a `$cards.<kind>.<i>...` path: select the `i`-th card whose `$kind`
/// is `<kind>`, then descend the remaining segments into it. Absolute indexing
/// was retired in `form@0.2.0` (the bind step rejects it), so the first segment
/// is always a kind and the second its instance index.
fn lookup_card<'a, 'p, I>(data: &'a Value, mut parts: I) -> Option<&'a Value>
where
    I: Iterator<Item = &'p str>,
{
    let cards = data.get("$cards")?.as_array()?;
    let kind = parts.next()?;
    let i: usize = parts.next()?.parse().ok()?;
    let card = cards
        .iter()
        .filter(|c| c.get("$kind").and_then(Value::as_str) == Some(kind))
        .nth(i)?;
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
    // `as_f64()` is always `Some` for a float-backed `Number`; the guard picks
    // the `f64` `Display` path for those and leaves integers on the JSON literal.
    match n.as_f64() {
        Some(f) if n.is_f64() => f.to_string(),
        _ => n.to_string(),
    }
}

/// Coerce a JSON value to display text. Empty results (empty string, all-null
/// array) become `None` so the widget carries no `/V`.
fn coerce_text(v: &Value) -> Option<String> {
    match v {
        // An array (e.g. an `array<richtext>`, richtext-corpus elements, or a
        // `string[]` field) joins its element texts with newlines — the
        // multiline text fill.
        Value::Array(arr) => {
            let s = arr
                .iter()
                .filter_map(element_text)
                .collect::<Vec<_>>()
                .join("\n");
            (!s.is_empty()).then_some(s)
        }
        // Every other shape is one scalar or richtext object: the same rule
        // an array element follows, with an empty result blanked out too.
        _ => element_text(v).filter(|s| !s.is_empty()),
    }
}

/// A scalar's (or richtext object's) display text: a string/number/bool
/// directly, or a richtext corpus via its plaintext — the corpus text minus
/// island slots (tables/images have no plaintext form; a non-corpus object
/// binds nothing). Shared by top-level scalar coercion and per-element array
/// joining, which is why an empty string survives here and is blanked by the
/// caller instead.
fn element_text(e: &Value) -> Option<String> {
    match e {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(number_to_string(n)),
        Value::Bool(b) => Some(b.to_string()),
        Value::Object(_) => richtext_plaintext(e),
        _ => None,
    }
}

/// A richtext corpus's plaintext, via [`quillmark_richtext::export::to_plaintext`]
/// (island slots stripped). `None` for a non-corpus object or an empty result.
///
/// Tables and images carry no plaintext, so a corpus whose content is only a
/// table binds nothing here — the field renders blank, no diagnostic. This is
/// the decided pdfform limitation (issue #880); see `to_plaintext`.
fn richtext_plaintext(v: &Value) -> Option<String> {
    let rt = quillmark_richtext::serial::from_canonical_value(v).ok()?;
    let text = quillmark_richtext::export::to_plaintext(&rt);
    (!text.is_empty()).then_some(text)
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
        resolve_value(&FieldType::Text { multiline: false }, Some(field), &data())
    }

    #[test]
    fn text_binds_scalar_and_joins_arrays() {
        assert_eq!(text("full_name"), Some("Ada Lovelace".into()));
        assert_eq!(
            resolve_value(
                &FieldType::Text { multiline: true },
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
    fn richtext_corpus_lowers_to_plaintext() {
        // A richtext field crosses the seam as canonical corpus JSON; the widget
        // value is its plaintext — markup dropped (marks live off the text),
        // island slots stripped.
        let rt =
            quillmark_richtext::import::from_markdown("A **bold** claim.\n\nSecond line.").unwrap();
        let corpus = quillmark_richtext::serial::to_canonical_value(&rt);
        assert_eq!(
            coerce_text(&corpus).as_deref(),
            Some("A bold claim.\nSecond line.")
        );
        // A blank corpus binds nothing.
        let blank =
            quillmark_richtext::serial::to_canonical_value(&quillmark_richtext::RichText::empty());
        assert_eq!(coerce_text(&blank), None);
        // A non-corpus object binds nothing.
        assert_eq!(coerce_text(&json!({ "x": 1 })), None);
    }

    #[test]
    fn richtext_array_joins_element_plaintext() {
        // An `array<richtext>` joins each element's plaintext with newlines.
        let el = |md: &str| {
            quillmark_richtext::serial::to_canonical_value(
                &quillmark_richtext::import::from_markdown(md).unwrap(),
            )
        };
        let arr = Value::Array(vec![el("First **ref**."), el("Second _ref_.")]);
        assert_eq!(
            coerce_text(&arr).as_deref(),
            Some("First ref.\nSecond ref.")
        );
    }

    #[test]
    fn unbound_is_blank() {
        assert_eq!(
            resolve_value(&FieldType::Text { multiline: false }, None, &data()),
            None
        );
    }

    #[test]
    fn checkbox_truthiness() {
        let on = |f| resolve_value(&FieldType::Checkbox, Some(f), &data());
        assert_eq!(on("agree"), Some(CHECKBOX_ON_STATE.to_string()));
        assert_eq!(on("decline"), None);
        assert_eq!(on("missing"), None);
    }

    #[test]
    fn choice_must_match_option() {
        let opts = vec!["red".to_string(), "green".to_string(), "blue".to_string()];
        let kind = FieldType::Choice { options: opts };
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
            resolve_value(&FieldType::Signature, Some("full_name"), &data()),
            None
        );
    }

    /// A mixed-kind `$cards` array: two indorsements with a note between them,
    /// so by-kind indexing must skip the note.
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
            &FieldType::Text { multiline: false },
            Some(path),
            &card_data(),
        )
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
    fn card_coercion_runs_per_widget_type() {
        // A checkbox bound to a card field coerces truthiness like any other.
        let agree = |path| resolve_value(&FieldType::Checkbox, Some(path), &card_data());
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
        assert_eq!(card_text("$cards.indorsement.0.missing"), None);
        // Absolute-index addressing is gone: `$cards.0.from` reads `0` as a kind,
        // which matches no card's `$kind`, so it resolves blank.
        assert_eq!(card_text("$cards.0.from"), None);
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
        // Geometry is already final (placed at bind time); this test pins the
        // value/page binding, so the rect is an opaque placeholder.
        let slot = |name: &str, page: usize, schema_field: &str| BoundWidget {
            name: name.into(),
            schema_field: Some(schema_field.into()),
            page,
            rect: [100.0, 100.0, 300.0, 120.0],
            field_type: FieldType::Text { multiline: false },
            tooltip: None,
        };
        let slot0 = slot("Indorsement0From", 0, "$cards.indorsement.0.from");
        let slot1 = slot("Indorsement1From", 1, "$cards.indorsement.1.from");

        let spec0 = field_spec(&slot0, &data);
        let spec1 = field_spec(&slot1, &data);

        // Instance 0's value on page 0; instance 1's value on page 1.
        assert_eq!(spec0.page, 0, "first slot is on page 0");
        assert_eq!(spec0.value.as_deref(), Some("Alice"), "instance 0 value");
        assert_eq!(spec1.page, 1, "second slot is on page 1");
        assert_eq!(spec1.value.as_deref(), Some("Bob"), "instance 1 value");

        // The names carry through unchanged (the spine writes them to /T).
        assert_eq!(spec0.name, "Indorsement0From");
        assert_eq!(spec1.name, "Indorsement1From");
    }
}
