//! Validation helpers for card-yaml `$`-prefixed system metadata.
//!
//! The closed set of `$` keys (`$quill`, `$kind`, `$id`, `$ext`, `$seed`) and
//! their typed values are stored as variants of [`super::PayloadItem`] inside a
//! card's unified [`super::Payload`] item list — they sit alongside user
//! fields and comments in source order, which is what makes inline-comment
//! preservation symmetric across the `$`/non-`$` boundary.
//!
//! This module holds the validation primitives shared between the parser,
//! the editor surface, and the storage DTO: stripping `$` keys out of a
//! parsed YAML mapping into typed [`super::PayloadItem`]s, and checking
//! `$kind` name conformance.

use std::str::FromStr;

use serde_json::Value as JsonValue;

use super::payload::{MetaKey, PayloadItem};
use crate::error::ParseError;
use crate::version::QuillReference;

/// The `$key` string a system-metadata [`PayloadItem`] variant corresponds
/// to, or `None` for non-system variants ([`PayloadItem::Field`] and
/// [`PayloadItem::Comment`]).
pub(super) fn meta_key(item: &PayloadItem) -> Option<&'static str> {
    match item {
        PayloadItem::Quill { .. } => Some("$quill"),
        PayloadItem::Kind { .. } => Some("$kind"),
        PayloadItem::Id { .. } => Some("$id"),
        PayloadItem::Meta { key, .. } => Some(key.as_str()),
        PayloadItem::Field { .. } | PayloadItem::Comment { .. } => None,
    }
}

/// Walk the parsed YAML payload, extracting `$`-prefixed reserved keys into
/// typed system-metadata [`PayloadItem`]s (`Quill` / `Kind` / `Id` / `Ext`)
/// in source order. The keys are removed from `payload` so the caller can
/// build the user-field portion from what remains.
///
/// The accepted keys are the closed set `{$quill, $kind, $id, $ext, $seed}`.
/// Any other `$`-prefixed key is a parse error. Duplicate keys cannot arise
/// here — the YAML parser rejects them as duplicate mapping keys before
/// this function runs.
///
/// `$quill` and `$kind` require string scalars (non-string YAML types are
/// rejected). `$id` accepts any scalar and stringifies it. `$ext` and `$seed`
/// each require a YAML mapping (object); `$ext` contents are carried opaquely,
/// while `$seed` is a map keyed by card-kind interpreted by the seeding layer.
pub(super) fn extract_meta_items(payload: &mut JsonValue) -> Result<Vec<PayloadItem>, ParseError> {
    let map = match payload {
        JsonValue::Object(m) => m,
        _ => return Ok(Vec::new()),
    };

    let dollar_keys: Vec<String> = map.keys().filter(|k| k.starts_with('$')).cloned().collect();

    let mut out = Vec::with_capacity(dollar_keys.len());
    for key in dollar_keys {
        let value = map
            .shift_remove(&key)
            .expect("key was just enumerated from the same map");
        let meta = match key.as_str() {
            "$quill" => {
                let s = require_string("$quill reference", value)?;
                let reference = QuillReference::from_str(&s).map_err(|reason| {
                    ParseError::InvalidQuillReference {
                        value: s.clone(),
                        reason,
                    }
                })?;
                PayloadItem::Quill { reference }
            }
            "$kind" => {
                let s = match value {
                    JsonValue::String(s) => s,
                    other => {
                        return Err(ParseError::InvalidStructure(format!(
                            "Invalid `$kind` value — a card kind must be a string \
                             matching `[a-z_][a-z0-9_]*` (got {})",
                            yaml_type_name(&other)
                        )));
                    }
                };
                if !is_valid_kind_name(&s) {
                    return Err(ParseError::InvalidStructure(format!(
                        "Invalid `$kind` value '{}' — a card kind must match \
                         `[a-z_][a-z0-9_]*`",
                        s
                    )));
                }
                PayloadItem::Kind { value: s }
            }
            "$id" => PayloadItem::Id {
                value: scalar_to_string(&key, value)?,
            },
            "$ext" | "$seed" => {
                let meta_key = MetaKey::from_key_str(&key).expect("matched $ext/$seed above");
                match value {
                    JsonValue::Object(map) => PayloadItem::Meta {
                        key: meta_key,
                        value: map,
                        nested_comments: Vec::new(),
                    },
                    other => {
                        return Err(ParseError::InvalidStructure(format!(
                            "Invalid `{}` value — expected a mapping, got {}",
                            meta_key.as_str(),
                            yaml_type_name(&other)
                        )));
                    }
                }
            }
            other => {
                return Err(ParseError::InvalidStructure(format!(
                    "Unknown `{}` system-metadata key — the card-yaml block \
                     accepts only `$quill`, `$kind`, `$id`, `$ext`, and `$seed`",
                    other
                )));
            }
        };
        out.push(meta);
    }

    Ok(out)
}

fn require_string(label: &str, value: JsonValue) -> Result<String, ParseError> {
    match value {
        JsonValue::String(s) => Ok(s),
        other => Err(ParseError::InvalidStructure(format!(
            "Invalid {} — expected a string scalar, got {}",
            label,
            yaml_type_name(&other)
        ))),
    }
}

fn scalar_to_string(key: &str, value: JsonValue) -> Result<String, ParseError> {
    match value {
        JsonValue::String(s) => Ok(s),
        JsonValue::Bool(b) => Ok(b.to_string()),
        JsonValue::Number(n) => Ok(n.to_string()),
        JsonValue::Null => Err(ParseError::InvalidStructure(format!(
            "`{}` cannot be null — provide a scalar value",
            key
        ))),
        other => Err(ParseError::InvalidStructure(format!(
            "`{}` must be a scalar value, got {}",
            key,
            yaml_type_name(&other)
        ))),
    }
}

fn yaml_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "sequence",
        JsonValue::Object(_) => "mapping",
    }
}

/// `true` when `name` matches `[a-z_][a-z0-9_]*`.
pub fn is_valid_kind_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }
    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }
    true
}

/// Validate a composable card kind: must match `[a-z_][a-z0-9_]*` and must
/// not be the reserved root kind `"main"`.
///
/// Single source of truth for the composable-kind rule, used by
/// [`crate::Card::new`], [`crate::Document::set_card_kind`], and the storage
/// DTO conversion so the rule cannot drift between editor and reader paths.
pub fn validate_composable_kind(kind: &str) -> Result<(), CardKindError> {
    if !is_valid_kind_name(kind) {
        return Err(CardKindError::InvalidName);
    }
    if kind == "main" {
        return Err(CardKindError::Reserved);
    }
    Ok(())
}

/// Reason [`validate_composable_kind`] rejected a kind string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKindError {
    /// Kind did not match `[a-z_][a-z0-9_]*`.
    InvalidName,
    /// Kind was `"main"`, reserved for the document root.
    Reserved,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_quill_kind_and_leaves_data_intact() {
        let mut payload = json!({
            "$quill": "foo@0.1",
            "$kind": "main",
            "title": "Doc",
        });
        let items = extract_meta_items(&mut payload).unwrap();
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], PayloadItem::Quill { .. }));
        assert!(matches!(items[1], PayloadItem::Kind { .. }));
        assert_eq!(payload, json!({"title": "Doc"}));
    }

    #[test]
    fn extracts_id_from_number() {
        let mut payload = json!({"$id": 42});
        let items = extract_meta_items(&mut payload).unwrap();
        assert!(matches!(items[0], PayloadItem::Id { ref value } if value == "42"));
    }

    #[test]
    fn rejects_unknown_dollar_key() {
        let mut payload = json!({"$unknown": "x"});
        let err = extract_meta_items(&mut payload).unwrap_err();
        assert!(err.to_string().contains("Unknown `$unknown`"));
    }

    #[test]
    fn rejects_non_string_quill() {
        let mut payload = json!({"$quill": 42});
        let err = extract_meta_items(&mut payload).unwrap_err();
        assert!(err.to_string().contains("$quill reference"));
    }

    #[test]
    fn rejects_invalid_kind_pattern() {
        let mut payload = json!({"$kind": "Bad-Kind"});
        let err = extract_meta_items(&mut payload).unwrap_err();
        assert!(err.to_string().contains("Invalid `$kind`"));
    }

    #[test]
    fn validate_composable_kind_rejects_main() {
        assert_eq!(
            validate_composable_kind("main"),
            Err(CardKindError::Reserved)
        );
    }

    #[test]
    fn validate_composable_kind_rejects_bad_name() {
        assert_eq!(
            validate_composable_kind("Bad-Name"),
            Err(CardKindError::InvalidName)
        );
    }

    #[test]
    fn validate_composable_kind_accepts_valid() {
        assert!(validate_composable_kind("indorsement").is_ok());
    }
}
