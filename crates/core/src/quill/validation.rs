use indexmap::IndexMap;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::document::Document;
use crate::quill::formats::DATE_FORMAT;
use crate::quill::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

/// Validation error with a structured field path.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("missing required field `{path}`")]
    MissingRequired { path: String },

    #[error("field `{path}` has type `{actual}`, expected `{expected}`")]
    TypeMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("field `{path}` value `{value}` not in allowed set {allowed:?}")]
    EnumViolation {
        path: String,
        value: String,
        allowed: Vec<String>,
    },

    #[error("field `{path}` does not match expected format `{format}`")]
    FormatViolation { path: String, format: String },

    #[error("unknown card type `{card}` at `{path}`")]
    UnknownCard { path: String, card: String },

    #[error("card at `{path}` missing `CARD` discriminator")]
    MissingCardDiscriminator { path: String },

    #[error(
        "card `{card}` at `{path}` has body content but `body.enabled` is false for this card type"
    )]
    BodyDisabled { path: String, card: String },
}

/// Validate a typed [`Document`] (with `IndexMap` frontmatter + typed `Card` list).
///
/// This is the typed entry point used by `QuillConfig::validate_document`.
pub fn validate_typed_document(
    config: &QuillConfig,
    doc: &Document,
) -> Result<(), Vec<ValidationError>> {
    let main_fields = doc.main().frontmatter().to_index_map();
    let mut errors = validate_fields_for_card_indexmap(&config.main, &main_fields, "");

    // Enforce body.enabled on the main card.
    if !config.main.body_enabled() && !doc.main().body().is_empty() {
        errors.push(ValidationError::BodyDisabled {
            path: "main".to_string(),
            card: "main".to_string(),
        });
    }

    for (index, card) in doc.cards().iter().enumerate() {
        let card_name = card.tag();
        let item_path = format!("cards[{index}]");
        // NOTE: `cards[N]` is the document-instance-side path (the cards
        // array on a Document). Card-type definitions live under
        // `card_types:` in Quill.yaml, but instances on a document are
        // still a `cards` list.

        let Some(card_schema) = config.card_type(card_name.as_str()) else {
            errors.push(ValidationError::UnknownCard {
                path: item_path,
                card: card_name,
            });
            continue;
        };

        let card_path = format!("cards.{card_name}[{index}]");
        let card_fields = card.frontmatter().to_index_map();
        errors.extend(validate_fields_for_card_indexmap(
            card_schema,
            &card_fields,
            &card_path,
        ));

        // Enforce body.enabled: when false, body content is not permitted.
        if !card_schema.body_enabled() && !card.body().is_empty() {
            errors.push(ValidationError::BodyDisabled {
                path: card_path,
                card: card_name,
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_fields_for_card_indexmap(
    card: &CardSchema,
    fields: &IndexMap<String, QuillValue>,
    base_path: &str,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut field_names: Vec<&String> = card.fields.keys().collect();
    field_names.sort();

    for field_name in field_names {
        let schema = &card.fields[field_name];
        let path = child_path(base_path, field_name);
        match fields.get(field_name) {
            Some(value) => errors.extend(validate_field(schema, value, &path)),
            None if schema.required => errors.push(ValidationError::MissingRequired { path }),
            None => {}
        }
    }

    errors
}

/// Validate a single value against a field schema at the given path.
/// Used internally; exposed for testing.
pub(crate) fn validate_field(
    field: &FieldSchema,
    value: &QuillValue,
    path: &str,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    let type_valid = match field.r#type {
        FieldType::String | FieldType::Markdown => value.as_str().is_some(),
        FieldType::Integer => {
            let json = value.as_json();
            json.is_i64() || json.is_u64()
        }
        FieldType::Number => value.as_json().is_number(),
        FieldType::Boolean => value.as_bool().is_some(),
        FieldType::Date => {
            if value.as_json().is_null() {
                true
            } else {
                match value.as_str() {
                    Some(text) if text.is_empty() => true,
                    Some(text) => {
                        if is_valid_date(text) {
                            true
                        } else {
                            errors.push(ValidationError::FormatViolation {
                                path: path.to_string(),
                                format: "date".to_string(),
                            });
                            false
                        }
                    }
                    None => false,
                }
            }
        }
        FieldType::DateTime => {
            if value.as_json().is_null() {
                true
            } else {
                match value.as_str() {
                    Some(text) if text.is_empty() => true,
                    Some(text) => {
                        if is_valid_datetime(text) {
                            true
                        } else {
                            errors.push(ValidationError::FormatViolation {
                                path: path.to_string(),
                                format: "date-time".to_string(),
                            });
                            false
                        }
                    }
                    None => false,
                }
            }
        }
        FieldType::Array => match value.as_array() {
            Some(items) => {
                if let Some(item_schema) = &field.items {
                    for (idx, item) in items.iter().enumerate() {
                        errors.extend(validate_field(
                            item_schema,
                            &QuillValue::from_json(item.clone()),
                            &index_path(path, idx),
                        ));
                    }
                }
                true
            }
            None => false,
        },
        FieldType::Object => match value.as_object() {
            Some(object) => {
                if let Some(properties) = &field.properties {
                    let mut property_names: Vec<&String> = properties.keys().collect();
                    property_names.sort();
                    for property_name in property_names {
                        let property_schema = &properties[property_name];
                        let property_path = child_path(path, property_name);
                        match object.get(property_name) {
                            Some(property_value) => errors.extend(validate_field(
                                property_schema,
                                &QuillValue::from_json(property_value.clone()),
                                &property_path,
                            )),
                            None if property_schema.required => {
                                errors.push(ValidationError::MissingRequired {
                                    path: property_path,
                                })
                            }
                            None => {}
                        }
                    }
                }
                true
            }
            None => false,
        },
    };

    // A Date/DateTime with a string value already emitted a FormatViolation;
    // skip the redundant TypeMismatch in that case.
    let format_error_already_reported =
        matches!(field.r#type, FieldType::Date | FieldType::DateTime) && value.as_str().is_some();

    if !type_valid && !format_error_already_reported {
        errors.push(ValidationError::TypeMismatch {
            path: path.to_string(),
            expected: expected_type_name(&field.r#type).to_string(),
            actual: json_type_name(value.as_json()).to_string(),
        });
    }

    if type_valid {
        if let (Some(allowed), Some(actual)) = (&field.enum_values, value.as_str()) {
            if !allowed.contains(&actual.to_string()) {
                errors.push(ValidationError::EnumViolation {
                    path: path.to_string(),
                    value: actual.to_string(),
                    allowed: allowed.clone(),
                });
            }
        }
    }

    errors
}

fn is_valid_date(value: &str) -> bool {
    Date::parse(value, &DATE_FORMAT).is_ok()
}

fn is_valid_datetime(value: &str) -> bool {
    OffsetDateTime::parse(value, &Rfc3339).is_ok()
}

fn expected_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::Markdown | FieldType::Date | FieldType::DateTime => "string",
        FieldType::Integer => "integer",
        FieldType::Number => "number",
        FieldType::Boolean => "boolean",
        FieldType::Array => "array",
        FieldType::Object => "object",
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn child_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}.{child}")
    }
}

fn index_path(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::document::{Card, Document};
    use crate::version::QuillReference;
    use serde_json::json;

    fn config_with(main_fields: &str, cards: &str) -> QuillConfig {
        let yaml = format!(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
  fields:
{main_fields}
{cards}
"#
        );
        let (config, warnings) = QuillConfig::from_yaml_with_warnings(&yaml).unwrap();
        assert!(
            warnings.is_empty(),
            "config_with produced warnings (test schema is unsupported): {:?}",
            warnings
        );
        config
    }

    fn doc_from_fm(entries: &[(&str, serde_json::Value)]) -> Document {
        doc_with_typed_cards(entries, vec![])
    }

    fn doc_with_typed_cards(fm: &[(&str, serde_json::Value)], cards: Vec<Card>) -> Document {
        use crate::document::{Frontmatter, Sentinel};
        let mut frontmatter = IndexMap::new();
        for (k, v) in fm {
            frontmatter.insert(k.to_string(), QuillValue::from_json(v.clone()));
        }
        let main = Card::new_with_sentinel(
            Sentinel::Main(QuillReference::from_str("test_quill").unwrap()),
            Frontmatter::from_index_map(frontmatter),
            String::new(),
        );
        Document::from_main_and_cards(main, cards, vec![])
    }

    fn typed_card(tag: &str, fields: &[(&str, serde_json::Value)]) -> Card {
        let mut card = Card::new(tag).unwrap();
        for (k, v) in fields {
            card.set_field(k, QuillValue::from_json(v.clone())).unwrap();
        }
        card
    }

    fn has_error<F>(errors: &[ValidationError], predicate: F) -> bool
    where
        F: Fn(&ValidationError) -> bool,
    {
        errors.iter().any(predicate)
    }

    #[test]
    fn validates_simple_string_field() {
        let config = config_with("    title:\n      type: string\n      required: true", "");
        let doc = doc_from_fm(&[("title", json!("Memo"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_simple_string_type_mismatch() {
        let config = config_with("    title:\n      type: string", "");
        let doc = doc_from_fm(&[("title", json!(9))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual }
            if path == "title" && expected == "string" && actual == "number"
        )));
    }

    #[test]
    fn validates_integer_field_with_integer_value() {
        let config = config_with("    count:\n      type: integer", "");
        let doc = doc_from_fm(&[("count", json!(9))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_integer_field_with_decimal_value() {
        let config = config_with("    count:\n      type: integer", "");
        let doc = doc_from_fm(&[("count", json!(9.5))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual }
            if path == "count" && expected == "integer" && actual == "number"
        )));
    }

    #[test]
    fn reports_missing_required_field() {
        let config = config_with(
            "    memo_for:\n      type: string\n      required: true",
            "",
        );
        let doc = doc_from_fm(&[]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::MissingRequired { path } if path == "memo_for")
        }));
    }

    #[test]
    fn reports_required_field_wrong_type() {
        let config = config_with(
            "    memo_for:\n      type: string\n      required: true",
            "",
        );
        let doc = doc_from_fm(&[("memo_for", json!(true))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, .. } if path == "memo_for"
        )));
    }

    #[test]
    fn validates_enum_value() {
        let config = config_with(
            "    status:\n      type: string\n      enum:\n        - draft\n        - final",
            "",
        );
        let doc = doc_from_fm(&[("status", json!("draft"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_enum_value() {
        let config = config_with(
            "    status:\n      type: string\n      enum:\n        - draft\n        - final",
            "",
        );
        let doc = doc_from_fm(&[("status", json!("invalid"))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::EnumViolation { path, value, .. }
            if path == "status" && value == "invalid"
        )));
    }

    #[test]
    fn validates_date_format() {
        let config = config_with("    signed_on:\n      type: date", "");
        let doc = doc_from_fm(&[("signed_on", json!("2026-04-13"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_date_format() {
        let config = config_with("    signed_on:\n      type: date", "");
        let doc = doc_from_fm(&[("signed_on", json!("13-04-2026"))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::FormatViolation { path, format } if path == "signed_on" && format == "date")
        }));
    }

    #[test]
    fn validates_datetime_format() {
        let config = config_with("    created_at:\n      type: datetime", "");
        let doc = doc_from_fm(&[("created_at", json!("2026-04-13T19:24:55Z"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_datetime_format() {
        let config = config_with("    created_at:\n      type: datetime", "");
        let doc = doc_from_fm(&[("created_at", json!("2026-04-13 19:24:55"))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::FormatViolation { path, format }
            if path == "created_at" && format == "date-time"
        )));
    }

    #[test]
    fn markdown_accepts_any_string() {
        let config = config_with("    body:\n      type: markdown", "");
        let doc = doc_from_fm(&[("body", json!("# Heading\n\nBody text"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn validates_array_of_strings() {
        let config = config_with(
            "    tags:\n      type: array\n      items:\n        type: string",
            "",
        );
        let doc = doc_from_fm(&[("tags", json!(["a", "b"]))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_invalid_array_element_type() {
        let config = config_with(
            "    tags:\n      type: array\n      items:\n        type: string",
            "",
        );
        let doc = doc_from_fm(&[("tags", json!(["a", 2]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, .. } if path == "tags[1]"
        )));
    }

    #[test]
    fn validates_array_of_objects() {
        let config = config_with(
            "    recipients:\n      type: array\n      items:\n        type: object\n        properties:\n          name:\n            type: string\n            required: true\n          org:\n            type: string",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "name": "Sam", "org": "HQ" }]))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn reports_missing_required_field_in_array_object() {
        let config = config_with(
            "    recipients:\n      type: array\n      items:\n        type: object\n        properties:\n          name:\n            type: string\n            required: true\n          org:\n            type: string",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "org": "HQ" }]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::MissingRequired { path } if path == "recipients[0].name")
        }));
    }

    // NOTE: top-level `type: object` fields are explicitly unsupported by
    // the config parser (see `config::parse_fields_with_order`). Object
    // schemas only appear inside `array.items`; coverage for that shape lives
    // in `validates_array_of_objects` and
    // `reports_missing_required_field_in_array_object`.

    #[test]
    fn accumulates_multiple_missing_required_errors() {
        let config = config_with(
            "    memo_for:\n      type: string\n      required: true\n    memo_from:\n      type: string\n      required: true",
            "",
        );
        let doc = doc_from_fm(&[]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let missing_paths: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                ValidationError::MissingRequired { path } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert!(missing_paths.contains(&"memo_for"));
        assert!(missing_paths.contains(&"memo_from"));
    }

    #[test]
    fn validates_card_with_valid_discriminator() {
        let config = config_with(
            "    title:\n      type: string",
            "card_types:\n  indorsement:\n    fields:\n      signature_block:\n        type: string\n        required: true",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![typed_card(
                "indorsement",
                &[("signature_block", json!("Signed"))],
            )],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_unknown_card_discriminator() {
        let config = config_with(
            "    title:\n      type: string",
            "card_types:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(&[], vec![typed_card("unknown", &[])]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::UnknownCard { path, card } if path == "cards[0]" && card == "unknown")
        }));
    }

    #[test]
    fn validates_multiple_card_instances_same_type() {
        let config = config_with(
            "    title:\n      type: string",
            "card_types:\n  indorsement:\n    fields:\n      signature_block:\n        type: string\n        required: true",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![
                typed_card("indorsement", &[("signature_block", json!("A"))]),
                typed_card("indorsement", &[("signature_block", json!("B"))]),
            ],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn validates_multiple_card_types_mixed() {
        let config = config_with(
            "    title:\n      type: string",
            "card_types:\n  indorsement:\n    fields:\n      signature_block:\n        type: string\n        required: true\n  routing:\n    fields:\n      office:\n        type: string\n        required: true",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![
                typed_card("indorsement", &[("signature_block", json!("A"))]),
                typed_card("routing", &[("office", json!("HQ"))]),
            ],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn reports_card_field_paths_with_card_name_and_index() {
        let config = config_with(
            "    title:\n      type: string",
            "card_types:\n  indorsement:\n    fields:\n      signature_block:\n        type: string\n        required: true",
        );
        let doc = doc_with_typed_cards(&[], vec![typed_card("indorsement", &[])]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::MissingRequired { path } if path == "cards.indorsement[0].signature_block")
        }));
    }

    // ── body.enabled enforcement ──────────────────────────────────────────────

    fn config_with_body(main_fields: &str, cards: &str, extra_main: &str) -> QuillConfig {
        let yaml = format!(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
{extra_main}
  fields:
{main_fields}
{cards}
"#
        );
        QuillConfig::from_yaml(&yaml).unwrap()
    }

    #[test]
    fn body_disabled_card_with_body_content_is_an_error() {
        let config = config_with_body(
            "    title:\n      type: string",
            "card_types:\n  skills:\n    body:\n      enabled: false\n    fields:\n      items:\n        type: array\n        required: true",
            "",
        );
        let mut card = typed_card("skills", &[("items", json!(["Rust", "Go"]))]);
        card.replace_body("Should not be here.");
        let doc = doc_with_typed_cards(&[], vec![card]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { card, .. } if card == "skills"
        )));
    }

    #[test]
    fn body_disabled_card_without_body_content_is_valid() {
        let config = config_with_body(
            "    title:\n      type: string",
            "card_types:\n  skills:\n    body:\n      enabled: false\n    fields:\n      items:\n        type: array\n        required: true",
            "",
        );
        let card = typed_card("skills", &[("items", json!(["Rust"]))]);
        let doc = doc_with_typed_cards(&[], vec![card]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn body_enabled_true_explicitly_allows_body_content() {
        let config = config_with_body(
            "    title:\n      type: string",
            "card_types:\n  note:\n    body:\n      enabled: true\n    fields:\n      author:\n        type: string",
            "",
        );
        let mut card = typed_card("note", &[("author", json!("Alice"))]);
        card.replace_body("Some body text.");
        let doc = doc_with_typed_cards(&[], vec![card]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn main_body_disabled_with_body_content_is_an_error() {
        let config = config_with_body(
            "    title:\n      type: string",
            "",
            "  body:\n    enabled: false",
        );
        use crate::document::{Frontmatter, Sentinel};
        let main = Card::new_with_sentinel(
            Sentinel::Main(
                crate::version::QuillReference::from_str("test_quill").unwrap(),
            ),
            Frontmatter::from_index_map(IndexMap::new()),
            "Body content that should not be here.".to_string(),
        );
        let doc = Document::from_main_and_cards(main, vec![], vec![]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { card, .. } if card == "main"
        )));
    }
}
