use indexmap::IndexMap;

use crate::document::Document;
use crate::error::{Diagnostic, Severity};
use crate::quill::formats::is_valid_datetime;
use crate::quill::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

/// Literal sentinel string the blueprint emitter writes into the value
/// cell of every Unendorsed field. Validation detects unreplaced sentinels
/// and reports `validation::must_fill_sentinel` under the uniform
/// validation message contract (see `ERROR.md`).
pub const MUST_FILL_SENTINEL: &str = "<must-fill>";

/// Validation error with a structured field path.
///
/// Field-level type and presence errors carry the field path, the
/// schema-declared type, and any verbatim YAML source token / default —
/// enough for the `Display` impl to render the uniform diagnostic message
/// described in `ERROR.md` ("Validation message contract").
///
/// Absence and the surviving blueprint sentinel are *separate* concerns with
/// opposite fatality, so they get separate variants:
/// [`FieldAbsent`](Self::FieldAbsent) is a non-fatal completeness signal (the
/// render floor zero-fills the field), whereas
/// [`MustFillSentinel`](Self::MustFillSentinel) is the blueprint↔author
/// contract — a surviving `<must-fill>` sentinel is a fatal authoring error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A schema field with no `default:` was absent from the document.
    ///
    /// This is a non-fatal *completeness* signal, **not** a fill requirement:
    /// the render floor zero-fills the field (see `prose/canon/SCHEMAS.md`,
    /// "Zero-filled render"), so render never gates on it. Consumers read the
    /// code for doneness — e.g. the form view's per-field `Missing` state.
    FieldAbsent {
        path: String,
        /// Schema-declared type (`string`, `integer`, …).
        expected: String,
    },

    /// The blueprint's `<must-fill>` sentinel survived into the document and
    /// reached validation.
    ///
    /// The sentinel is the system's own "replace me" token, stamped into every
    /// Unendorsed cell by the blueprint to signal LLMs and authors. A surviving
    /// one is provably an authoring accident, so it is fatal on every path.
    MustFillSentinel {
        path: String,
        /// Schema-declared type (`string`, `integer`, …).
        expected: String,
    },

    TypeMismatch {
        path: String,
        /// Schema-declared type (`string`, `integer`, …).
        expected: String,
        /// YAML-parsed type of the source token (`integer`, `number`,
        /// `boolean`, `null`, `string`, `array`, `object`).
        actual: String,
        /// Verbatim YAML scalar that triggered the error, rendered in
        /// its canonical YAML form (`42`, `null`, `"hello"`, `""`).
        source_token: String,
        /// Pre-rendered default token from the schema, when present.
        /// Same canonical YAML form as `source_token`.
        default: Option<String>,
    },

    EnumViolation {
        path: String,
        value: String,
        allowed: Vec<String>,
    },

    FormatViolation {
        path: String,
        format: String,
    },

    UnknownCard {
        path: String,
        card: String,
    },

    BodyDisabled {
        path: String,
        card: String,
    },
}

impl std::error::Error for ValidationError {}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::FieldAbsent { path, expected } => write!(
                f,
                "Field `{path}` is missing, schema declares `{expected}` with no default. \
                 {hint}",
                hint = field_absent_hint(expected),
            ),
            // The blueprint sentinel survived into the document. Single exit:
            // there's no quote-or-omit alternative — the sentinel always means
            // "replace me".
            ValidationError::MustFillSentinel { path, expected } => write!(
                f,
                "Field `{path}` still carries the `{MUST_FILL_SENTINEL}` blueprint sentinel, \
                 schema declares `{expected}`. {hint}",
                hint = must_fill_sentinel_hint(expected),
            ),
            ValidationError::TypeMismatch {
                path,
                expected,
                actual,
                source_token,
                default,
            } => {
                // Line 1: what we got vs what the schema says.
                write!(f, "Field `{path}` got {actual} `{source_token}`, schema declares `{expected}`")?;
                if let Some(d) = default {
                    write!(f, " with default `{d}`")?;
                }
                write!(f, ". {hint}", hint = type_mismatch_hint(path, expected, actual, source_token, default.as_deref()))
            }
            ValidationError::EnumViolation { path, value, allowed } => {
                write!(
                    f,
                    "field `{path}` value `{value}` not in allowed set {allowed:?}"
                )
            }
            ValidationError::FormatViolation { path, format } => {
                write!(
                    f,
                    "field `{path}` does not match expected format `{format}`"
                )
            }
            ValidationError::UnknownCard { path, card } => {
                write!(f, "unknown card kind `{card}` at `{path}`")
            }
            ValidationError::BodyDisabled { path, card } => {
                write!(
                    f,
                    "card `{card}` at `{path}` has body content but the card kind declares `body.enabled: false` — {hint}",
                    hint = body_disabled_hint(),
                )
            }
        }
    }
}

/// Whether `actual` (a YAML-parsed type name) is a primitive scalar the
/// author could lift to a string by quoting the source token. `null` is
/// excluded — quoting `null` produces the literal string `"null"`, which
/// is rarely what an LLM author intended.
fn quotable_actual(actual: &str) -> bool {
    matches!(actual, "integer" | "number" | "boolean")
}

/// Actionable exit clause for an absent field. Used by both `Display` and
/// `hint()` so the recommended action lives in exactly one place.
fn field_absent_hint(expected: &str) -> String {
    format!("Provide a value of type `{expected}`.")
}

/// Actionable exit clause for a surviving `<must-fill>` sentinel.
fn must_fill_sentinel_hint(expected: &str) -> String {
    format!("Replace `{MUST_FILL_SENTINEL}` with a value of type `{expected}`.")
}

/// Actionable exit clause for a TypeMismatch. Mirrors the (expected, actual,
/// has_default) branching in `Display` so the structured hint and the prose
/// message can never disagree.
fn type_mismatch_hint(
    path: &str,
    expected: &str,
    actual: &str,
    source_token: &str,
    default: Option<&str>,
) -> String {
    if default.is_some() && actual == "null" {
        // `null` here means the YAML value parsed as null. That can be any of:
        //   `field: null`   (explicit literal)
        //   `field: ~`      (YAML shorthand for null)
        //   `field:`        (bare key — a missing value also parses as null)
        // In every case the LLM almost always meant "skip this field". The
        // shortest fix is to remove the line entirely so the default applies.
        format!(
            "To use the default, delete this entire line (do NOT write \
             `{path}:`, `{path}: null`, or `{path}: ~` — all three parse as \
             null). To set an explicit value, replace the right-hand side \
             with a {expected}."
        )
    } else if expected == "string" && quotable_actual(actual) {
        format!(
            "Either quote the value (`{path}: \"{source_token}\"`) or change the schema's `type:` to `{actual}`."
        )
    } else if default.is_some() {
        format!(
            "Either omit the line (the default will fill in) or provide a value of type `{expected}`."
        )
    } else {
        format!(
            "Either provide a value of type `{expected}` or change the schema's `type:` to `{actual}`."
        )
    }
}

/// Actionable exit clause for a `BodyDisabled` error. Same text in both the
/// prose message and the structured hint.
fn body_disabled_hint() -> &'static str {
    "remove the body content or set `body.enabled: true` on the card kind"
}

impl ValidationError {
    /// Document-model path anchor for this error.
    ///
    /// See [`crate::error`] module docs for the path grammar and conventions.
    pub fn path(&self) -> &str {
        match self {
            ValidationError::FieldAbsent { path, .. }
            | ValidationError::MustFillSentinel { path, .. }
            | ValidationError::TypeMismatch { path, .. }
            | ValidationError::EnumViolation { path, .. }
            | ValidationError::FormatViolation { path, .. }
            | ValidationError::UnknownCard { path, .. }
            | ValidationError::BodyDisabled { path, .. } => path,
        }
    }

    /// Stable diagnostic code for this error variant. Pattern-match on this
    /// instead of the message text.
    pub fn code(&self) -> &'static str {
        match self {
            ValidationError::FieldAbsent { .. } => "validation::field_absent",
            ValidationError::MustFillSentinel { .. } => "validation::must_fill_sentinel",
            ValidationError::TypeMismatch { .. } => "validation::type_mismatch",
            ValidationError::EnumViolation { .. } => "validation::enum_violation",
            ValidationError::FormatViolation { .. } => "validation::format_violation",
            ValidationError::UnknownCard { .. } => "validation::unknown_card",
            ValidationError::BodyDisabled { .. } => "validation::body_disabled",
        }
    }

    /// Actionable hint for this error, when one is well-defined for the
    /// variant. The hint restates the recommended exit so consumers (LLM
    /// agents, IDEs, MCP clients) can surface it next to the message
    /// without re-parsing prose. The hint text is the same string the
    /// `Display` impl bakes into the message.
    pub fn hint(&self) -> Option<String> {
        match self {
            ValidationError::FieldAbsent { expected, .. } => Some(field_absent_hint(expected)),
            ValidationError::MustFillSentinel { expected, .. } => {
                Some(must_fill_sentinel_hint(expected))
            }
            ValidationError::TypeMismatch {
                path,
                expected,
                actual,
                source_token,
                default,
            } => Some(type_mismatch_hint(
                path,
                expected,
                actual,
                source_token,
                default.as_deref(),
            )),
            ValidationError::BodyDisabled { .. } => Some(body_disabled_hint().to_string()),
            ValidationError::EnumViolation { .. }
            | ValidationError::FormatViolation { .. }
            | ValidationError::UnknownCard { .. } => None,
        }
    }

    /// Convert this error into a structured [`Diagnostic`] carrying the
    /// stable code, the document-model `path`, the canonical message, and
    /// the actionable hint (when the variant defines one).
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut diag = Diagnostic::new(Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_path(self.path().to_string());
        if let Some(hint) = self.hint() {
            diag = diag.with_hint(hint);
        }
        diag
    }
}

/// Render a JSON scalar as the verbatim YAML token it would parse from.
/// Primitives appear bare (`42`, `true`, `null`); strings appear quoted
/// (`"hello"`, `""`); compound values render as a short placeholder.
fn verbatim_yaml_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("\"{s}\""),
        serde_json::Value::Array(_) => "[…]".to_string(),
        serde_json::Value::Object(_) => "{…}".to_string(),
    }
}

/// YAML-parsed type name for a JSON value. Distinguishes `integer` from
/// `number` (the proposal's example messages need that split).
fn yaml_scalar_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Validate a typed [`Document`] (with `IndexMap` payload + typed `Card` list).
///
/// This is the typed entry point used by `QuillConfig::validate_document`.
pub fn validate_typed_document(
    config: &QuillConfig,
    doc: &Document,
) -> Result<(), Vec<ValidationError>> {
    let main_fields = doc.main().payload().to_index_map();
    let mut errors = validate_fields_for_card_indexmap(&config.main, &main_fields, "");

    // Enforce body.enabled on the main card. Whitespace-only bodies are
    // treated as empty — only meaningful prose triggers the diagnostic.
    if !config.main.body_enabled() && !doc.main().body().trim().is_empty() {
        errors.push(ValidationError::BodyDisabled {
            path: "main.body".to_string(),
            card: "main".to_string(),
        });
    }

    for (index, card) in doc.cards().iter().enumerate() {
        let card_name = card.kind().unwrap_or("").to_string();
        let item_path = format!("cards[{index}]");
        // NOTE: `cards[N]` is the document-instance-side path (the cards
        // array on a Document). Card-kind definitions live under
        // `card_kinds:` in Quill.yaml, but instances on a document are
        // still a `cards` list.

        let Some(card_schema) = config.card_kind(card_name.as_str()) else {
            errors.push(ValidationError::UnknownCard {
                path: item_path,
                card: card_name,
            });
            continue;
        };

        let card_path = format!("cards.{card_name}[{index}]");
        let card_fields = card.payload().to_index_map();
        errors.extend(validate_fields_for_card_indexmap(
            card_schema,
            &card_fields,
            &card_path,
        ));

        if !card_schema.body_enabled() && !card.body().trim().is_empty() {
            errors.push(ValidationError::BodyDisabled {
                path: format!("{card_path}.body"),
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
            None if schema.default.is_none() => {
                // Unendorsed (no `default:`) field absent from the document — a
                // non-fatal completeness signal; the render floor zero-fills it.
                errors.push(ValidationError::FieldAbsent {
                    path,
                    expected: expected_type_name(&schema.r#type).to_string(),
                })
            }
            None => {}
        }
    }

    errors
}

/// Distinguishes the two value sources the conformance core
/// [`validate_value`] serves. The type/enum/format/recursion checks are
/// identical; only the document-authoring concerns differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueContext {
    /// A value parsed from an authored document. Detects the `<must-fill>`
    /// sentinel, requires non-defaulted object properties to be present, and
    /// reports the field's `default:` token alongside a type mismatch.
    Document,
    /// An `example:` or `default:` literal declared in Quill.yaml. Partial
    /// objects are allowed (absent properties are not errors) and the
    /// document-only sentinel / default semantics do not apply.
    SchemaLiteral,
}

/// Shared conformance core: validate a single `value` against `field` at
/// `path`, checking type compatibility, enum membership, datetime format, and
/// recursing into array elements / object properties. `ctx` selects the few
/// document-only behaviors (see [`ValueContext`]).
fn validate_value(
    field: &FieldSchema,
    value: &QuillValue,
    path: &str,
    ctx: ValueContext,
) -> Vec<ValidationError> {
    // Sentinel detection is a document-only concern and runs before per-type
    // validation. Scalar fields carry the sentinel in the value cell; markdown
    // carries it as the trimmed content of the block scalar. On match, emit
    // `MustFillSentinel` and skip per-type checks.
    if ctx == ValueContext::Document {
        if let Some(text) = value.as_str() {
            let candidate = if matches!(field.r#type, FieldType::Markdown) {
                text.trim()
            } else {
                text
            };
            if candidate == MUST_FILL_SENTINEL {
                return vec![ValidationError::MustFillSentinel {
                    path: path.to_string(),
                    expected: expected_type_name(&field.r#type).to_string(),
                }];
            }
        }
    }

    let mut errors = Vec::new();

    let type_valid = match field.r#type {
        FieldType::String | FieldType::Markdown => value.as_str().is_some(),
        FieldType::Integer => {
            let json = value.as_json();
            json.is_i64() || json.is_u64()
        }
        FieldType::Number => value.as_json().is_number(),
        FieldType::Boolean => value.as_bool().is_some(),
        FieldType::DateTime => {
            if value.as_json().is_null() {
                true
            } else {
                match value.as_str() {
                    Some("") => true,
                    Some(text) => {
                        if is_valid_datetime(text) {
                            true
                        } else {
                            errors.push(ValidationError::FormatViolation {
                                path: path.to_string(),
                                format: "datetime".to_string(),
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
                // Validate each element against the array's `items` schema.
                // Scalar elements (`string[]`, `integer[]`, `markdown[]`, …)
                // are type-checked element-wise; object elements recurse into
                // their properties via the Object branch (in a document, a row
                // collapsed to the `<must-fill>` sentinel surfaces a single
                // Sentinel error at the row path through the sentinel detector
                // above).
                if let Some(item_schema) = &field.items {
                    for (idx, item) in items.iter().enumerate() {
                        let row_path = format!("{}[{}]", path, idx);
                        errors.extend(validate_value(
                            item_schema,
                            &QuillValue::from_json(item.clone()),
                            &row_path,
                            ctx,
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
                            Some(property_value) => errors.extend(validate_value(
                                property_schema,
                                &QuillValue::from_json(property_value.clone()),
                                &property_path,
                                ctx,
                            )),
                            // A missing non-defaulted (Unendorsed) property is a
                            // completeness signal in a document; in a schema
                            // literal, partial objects are intentional and
                            // absence is not an error.
                            None if ctx == ValueContext::Document
                                && property_schema.default.is_none() =>
                            {
                                errors.push(ValidationError::FieldAbsent {
                                    path: property_path,
                                    expected: expected_type_name(&property_schema.r#type)
                                        .to_string(),
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

    // A DateTime with a string value already emitted a FormatViolation;
    // skip the redundant TypeMismatch in that case.
    let format_error_already_reported =
        matches!(field.r#type, FieldType::DateTime) && value.as_str().is_some();

    if !type_valid && !format_error_already_reported {
        errors.push(ValidationError::TypeMismatch {
            path: path.to_string(),
            expected: expected_type_name(&field.r#type).to_string(),
            actual: yaml_scalar_type(value.as_json()).to_string(),
            source_token: verbatim_yaml_scalar(value.as_json()),
            // The `default:` token is a document-authoring aid ("omit the line
            // and the default fills in") — meaningless when validating the
            // schema's own literals.
            default: match ctx {
                ValueContext::Document => field
                    .default
                    .as_ref()
                    .map(|d| verbatim_yaml_scalar(d.as_json())),
                ValueContext::SchemaLiteral => None,
            },
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

/// Validate a single document value against a field schema at the given path.
/// Used internally; exposed for testing.
pub(crate) fn validate_field(
    field: &FieldSchema,
    value: &QuillValue,
    path: &str,
) -> Vec<ValidationError> {
    validate_value(field, value, path, ValueContext::Document)
}

/// Validate a schema literal value — an `example:` or `default:` declared in
/// Quill.yaml — against a field schema.
///
/// Shares the type/enum/format/recursion core with [`validate_field`] (see
/// [`validate_value`]) but omits the document-authoring concerns: it does not
/// detect the `<must-fill>` sentinel, does not emit `FieldAbsent` for absent
/// object properties (partial examples/defaults are intentional and valid), and
/// never attaches a `default:` token to a type mismatch.
pub(crate) fn validate_schema_literal(
    schema: &FieldSchema,
    value: &QuillValue,
    path: &str,
) -> Vec<ValidationError> {
    validate_value(schema, value, path, ValueContext::SchemaLiteral)
}

fn expected_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::Markdown | FieldType::DateTime => "string",
        FieldType::Integer => "integer",
        FieldType::Number => "number",
        FieldType::Boolean => "boolean",
        FieldType::Array => "array",
        FieldType::Object => "object",
    }
}

fn child_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}.{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Card, Document};
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
        use crate::document::Payload;
        let mut payload = IndexMap::new();
        for (k, v) in fm {
            payload.insert(k.to_string(), QuillValue::from_json(v.clone()));
        }
        let mut p = Payload::from_index_map(payload);
        p.set_quill("test_quill".parse().unwrap());
        p.set_kind("main");
        let main = Card::from_parts(p, String::new());
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
        let config = config_with("    title:\n      type: string", "");
        let doc = doc_from_fm(&[("title", json!("Memo"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_simple_string_type_mismatch() {
        let config = config_with("    title:\n      type: string\n      default: \"\"", "");
        let doc = doc_from_fm(&[("title", json!(9))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual, source_token, .. }
            if path == "title" && expected == "string" && actual == "integer" && source_token == "9"
        )));
    }

    #[test]
    fn validates_integer_field_with_integer_value() {
        let config = config_with("    count:\n      type: integer\n      default: 0", "");
        let doc = doc_from_fm(&[("count", json!(9))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_integer_field_with_decimal_value() {
        let config = config_with("    count:\n      type: integer\n      default: 0", "");
        let doc = doc_from_fm(&[("count", json!(9.5))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual, source_token, .. }
            if path == "count" && expected == "integer" && actual == "number" && source_token == "9.5"
        )));
    }

    #[test]
    fn reports_absent_must_fill_field() {
        // A field with no `default:` is Unendorsed. Missing from the document
        // → `FieldAbsent`.
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::FieldAbsent { path, expected } if path == "memo_for" && expected == "string")
        }));
    }

    #[test]
    fn missing_field_with_default_is_ok() {
        // Endorsed field absent from document → no error; default applies.
        let config = config_with(
            "    memo_for:\n      type: string\n      default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn detects_must_fill_sentinel() {
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[("memo_for", json!(MUST_FILL_SENTINEL))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::MustFillSentinel { path, expected } if path == "memo_for" && expected == "string")
        }));
    }

    #[test]
    fn detects_must_fill_sentinel_in_markdown_block() {
        // Markdown block scalars carry the sentinel inside the block; the
        // detector trims whitespace before comparing.
        let config = config_with("    body:\n      type: markdown", "");
        let doc = doc_from_fm(&[("body", json!(format!("  {}\n", MUST_FILL_SENTINEL)))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::MustFillSentinel { path, .. } if path == "body")
        }));
    }

    #[test]
    fn reports_must_fill_property_absent_in_array_object() {
        // Property `name` is Unendorsed (no default); `org` is Endorsed.
        // Missing `name` in a row → `FieldAbsent`.
        let config = config_with(
            "    recipients:\n      type: array\n      default: []\n      items:\n        type: object\n        properties:\n          name:\n            type: string\n          org:\n            type: string\n            default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "org": "HQ" }]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::FieldAbsent { path, .. } if path == "recipients[0].name")
        }));
    }

    #[test]
    fn detects_must_fill_sentinel_as_typed_table_row() {
        // Hand-edit edge case: the user collapses a synthetic row down to the
        // literal sentinel string instead of expanding it into a mapping.
        // One Sentinel error at the row path beats N noisy Absent errors per
        // Unendorsed property.
        let config = config_with(
            "    recipients:\n      type: array\n      items:\n        type: object\n        properties:\n          name:\n            type: string\n          org:\n            type: string",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([MUST_FILL_SENTINEL]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(
            has_error(&errors, |e| matches!(
                e,
                ValidationError::MustFillSentinel { path, .. }
                if path == "recipients[0]"
            )),
            "expected sentinel error at row path; got {errors:?}"
        );
        // And no per-property Absent errors for that row — the row-level
        // sentinel diagnostic supersedes them.
        assert!(
            !has_error(&errors, |e| matches!(
                e,
                ValidationError::FieldAbsent { path, .. }
                if path.starts_with("recipients[0].")
            )),
            "row-level sentinel should suppress per-property Absent errors; got {errors:?}"
        );
    }

    // NOTE: top-level typed-dictionary fields (`type: object` with `properties`)
    // are supported. Coverage lives in `validates_array_of_objects` (typed
    // tables) and the blueprint tests. Freeform objects without properties are
    // rejected at config parse time.

    #[test]
    fn accumulates_multiple_absent_must_fill_errors() {
        let config = config_with(
            "    memo_for:\n      type: string\n    memo_from:\n      type: string",
            "",
        );
        let doc = doc_from_fm(&[]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let missing_paths: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                ValidationError::FieldAbsent { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert!(missing_paths.contains(&"memo_for"));
        assert!(missing_paths.contains(&"memo_from"));
    }

    #[test]
    fn validates_card_with_valid_discriminator() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
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
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
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
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
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
    fn validates_multiple_card_kinds_mixed() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string\n  routing:\n    fields:\n      office:\n        type: string",
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
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(&[], vec![typed_card("indorsement", &[])]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::FieldAbsent { path, .. } if path == "cards.indorsement[0].signature_block")
        }));
    }

    #[test]
    fn body_disabled_card_enforces_trim_boundary() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  skills:\n    body:\n      enabled: false\n    fields:\n      items:\n        type: array\n        items:\n          type: string\n        default: []",
        );
        // Prose triggers the error; whitespace-only does not.
        let mut prose_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        prose_card.replace_body("Should not be here.");
        let doc = doc_with_typed_cards(&[], vec![prose_card]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "skills" && path == "cards.skills[0].body"
        )));

        let mut ws_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        ws_card.replace_body("\n   \n");
        let ok_doc = doc_with_typed_cards(&[], vec![ws_card]);
        assert!(validate_typed_document(&config, &ok_doc).is_ok());
    }

    #[test]
    fn to_diagnostic_carries_path_and_code() {
        let err = ValidationError::FieldAbsent {
            path: "cards.indorsement[0].signature_block".to_string(),
            expected: "string".to_string(),
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("validation::field_absent"));
        assert_eq!(
            diag.path.as_deref(),
            Some("cards.indorsement[0].signature_block")
        );
        assert_eq!(diag.severity, Severity::Error);
    }

    #[test]
    fn field_absent_diagnostic_carries_actionable_hint() {
        let err = ValidationError::FieldAbsent {
            path: "title".to_string(),
            expected: "string".to_string(),
        };
        let diag = err.to_diagnostic();
        let hint = diag
            .hint
            .as_deref()
            .expect("field_absent diagnostic should carry a hint");
        assert!(hint.contains("string"), "hint missing expected type: {hint}");
        assert!(
            !hint.contains(MUST_FILL_SENTINEL),
            "absent-branch hint must not mention the sentinel: {hint}"
        );
    }

    #[test]
    fn must_fill_sentinel_diagnostic_carries_actionable_hint() {
        let err = ValidationError::MustFillSentinel {
            path: "title".to_string(),
            expected: "string".to_string(),
        };
        let diag = err.to_diagnostic();
        assert_eq!(
            diag.code.as_deref(),
            Some("validation::must_fill_sentinel")
        );
        let hint = diag
            .hint
            .as_deref()
            .expect("must_fill_sentinel diagnostic should carry a hint");
        assert!(hint.contains(MUST_FILL_SENTINEL));
        assert!(hint.contains("string"));
    }

    #[test]
    fn type_mismatch_diagnostic_carries_hint_matching_message() {
        // The structured hint must equal the exit clause baked into the
        // prose message, so consumers never need to re-parse.
        let config = config_with(
            "    build_number:\n      type: string\n      default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("build_number", json!(42))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let err = errors
            .iter()
            .find(|e| matches!(e, ValidationError::TypeMismatch { .. }))
            .expect("expected TypeMismatch");
        let diag = err.to_diagnostic();
        let hint = diag.hint.expect("TypeMismatch diagnostic should carry a hint");
        assert!(
            err.to_string().ends_with(&hint),
            "message tail must equal hint; msg={msg}, hint={hint}",
            msg = err,
        );
        assert!(hint.contains("quote the value"));
    }

    #[test]
    fn body_disabled_diagnostic_carries_hint() {
        let err = ValidationError::BodyDisabled {
            path: "cards.skills[0].body".to_string(),
            card: "skills".to_string(),
        };
        let diag = err.to_diagnostic();
        let hint = diag.hint.expect("BodyDisabled diagnostic should carry a hint");
        assert!(hint.contains("remove the body content"));
    }

    #[test]
    fn type_mismatch_message_has_canonical_shape_quote_exit() {
        // Integer source under a `string` schema → quote-the-value exit.
        let config = config_with("    build_number:\n      type: string\n      default: \"\"", "");
        let doc = doc_from_fm(&[("build_number", json!(42))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let msg = errors
            .iter()
            .find_map(|e| match e {
                ValidationError::TypeMismatch { .. } => Some(e.to_string()),
                _ => None,
            })
            .expect("expected TypeMismatch");
        assert!(
            msg.contains("Field `build_number` got integer `42`, schema declares `string`"),
            "wrong head: {msg}"
        );
        assert!(
            msg.contains("quote the value (`build_number: \"42\"`)"),
            "missing quote exit: {msg}"
        );
        assert!(
            msg.contains("change the schema's `type:` to `integer`"),
            "missing schema-change exit: {msg}"
        );
    }

    #[test]
    fn type_mismatch_message_has_canonical_shape_default_exit() {
        // Null under a `string` schema with a default → omit-the-line exit.
        let config = config_with(
            "    subtitle:\n      type: string\n      default: \"My Subtitle\"",
            "",
        );
        let doc = doc_from_fm(&[("subtitle", json!(null))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let msg = errors
            .iter()
            .find_map(|e| match e {
                ValidationError::TypeMismatch { .. } => Some(e.to_string()),
                _ => None,
            })
            .expect("expected TypeMismatch");
        assert!(
            msg.contains(
                "Field `subtitle` got null `null`, schema declares `string` with default `\"My Subtitle\"`"
            ),
            "wrong head: {msg}"
        );
        assert!(
            msg.contains("delete this entire line"),
            "missing omit-line exit: {msg}"
        );
        assert!(
            msg.contains("`subtitle: ~`"),
            "expected message to name the `~` shorthand: {msg}"
        );
    }

    #[test]
    fn main_body_disabled_with_body_content_is_an_error() {
        let config = QuillConfig::from_yaml(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
  body:
    enabled: false
  fields:
    title:
      type: string
      default: ""
"#,
        )
        .unwrap();
        use crate::document::Payload;
        let mut p = Payload::from_index_map(IndexMap::new());
        p.set_quill("test_quill".parse().unwrap());
        p.set_kind("main");
        let main = Card::from_parts(p, "Body content that should not be here.".to_string());
        let doc = Document::from_main_and_cards(main, vec![], vec![]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "main" && path == "main.body"
        )));
    }
}
