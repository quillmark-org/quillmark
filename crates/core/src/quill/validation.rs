use indexmap::IndexMap;

use crate::document::Document;
use crate::error::{Diagnostic, Severity};
use crate::quill::formats::is_valid_datetime;
use crate::quill::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

/// Validation error with a structured field path.
///
/// Field-level type and presence errors carry the field path, the
/// schema-declared type, and any verbatim YAML source token / default —
/// enough for the `Display` impl to render the uniform diagnostic message
/// described in `ERROR.md` ("Validation message contract").
///
/// Two concerns are deliberately *not* well-formedness errors and so have no
/// variant here: the `!must_fill` marker (surfaced as a non-fatal warning by
/// `Quill::validate`) and field absence (an absent or present-null field
/// zero-fills at render). Both are handled outside the value-layer checks below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
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

    /// A `richtext(inline)` field whose corpus is not single-`Para` (a block, a
    /// list/quote container, or an island). Same fatality class as
    /// `TypeMismatch` — the value is well-typed richtext but the wrong *shape*
    /// for an inline field.
    NotInline {
        path: String,
    },

    /// A `plaintext` field whose corpus carries marks, islands, or block
    /// formatting. Same fatality class as `TypeMismatch` — the value is a
    /// well-formed corpus but the wrong *shape* for a plaintext field, which
    /// takes prose the author navigates but no formatting.
    NotPlain {
        path: String,
    },
}

impl std::error::Error for ValidationError {}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::TypeMismatch {
                path,
                expected,
                actual,
                source_token,
                default,
            } => {
                // Line 1: what we got vs what the schema says.
                write!(
                    f,
                    "Field `{path}` got {actual} `{source_token}`, schema declares `{expected}`"
                )?;
                if let Some(d) = default {
                    write!(f, " with default `{d}`")?;
                }
                write!(
                    f,
                    ". {hint}",
                    hint = type_mismatch_hint(expected, actual, default.as_deref())
                )
            }
            ValidationError::EnumViolation {
                path,
                value,
                allowed,
            } => {
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
            ValidationError::NotInline { path } => {
                write!(
                    f,
                    "field `{path}` is `richtext(inline)` but its content is not a single \
                     paragraph — {hint}",
                    hint = not_inline_hint(),
                )
            }
            ValidationError::NotPlain { path } => {
                write!(
                    f,
                    "field `{path}` is `plaintext` but its content carries formatting — {hint}",
                    hint = not_plain_hint(),
                )
            }
        }
    }
}

/// Actionable exit clause for a TypeMismatch. Mirrors the (expected, actual,
/// has_default) branching in `Display` so the structured hint and the prose
/// message can never disagree.
fn type_mismatch_hint(expected: &str, actual: &str, default: Option<&str>) -> String {
    if default.is_some() {
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

/// Actionable exit clause for a `NotInline` error.
fn not_inline_hint() -> &'static str {
    "keep the value to a single paragraph (no blank lines, headings, lists, \
     quotes, or tables), or change the schema's `type:` to `richtext`"
}

/// Actionable exit clause for a `NotPlain` error.
fn not_plain_hint() -> &'static str {
    "remove the formatting (marks, tables, images, headings, lists, quotes), or \
     change the schema's `type:` to `richtext`"
}

impl ValidationError {
    /// Document-model path anchor for this error.
    ///
    /// See [`crate::error`] module docs for the path grammar and conventions.
    pub fn path(&self) -> &str {
        match self {
            ValidationError::TypeMismatch { path, .. }
            | ValidationError::EnumViolation { path, .. }
            | ValidationError::FormatViolation { path, .. }
            | ValidationError::UnknownCard { path, .. }
            | ValidationError::BodyDisabled { path, .. }
            | ValidationError::NotInline { path, .. }
            | ValidationError::NotPlain { path, .. } => path,
        }
    }

    /// Stable diagnostic code for this error variant. Pattern-match on this
    /// instead of the message text.
    pub fn code(&self) -> &'static str {
        match self {
            ValidationError::TypeMismatch { .. } => "validation::type_mismatch",
            ValidationError::EnumViolation { .. } => "validation::enum_violation",
            ValidationError::FormatViolation { .. } => "validation::format_violation",
            ValidationError::UnknownCard { .. } => "validation::unknown_card",
            ValidationError::BodyDisabled { .. } => "validation::body_disabled",
            ValidationError::NotInline { .. } => "richtext::not_inline",
            ValidationError::NotPlain { .. } => "plaintext::not_plain",
        }
    }

    /// Actionable hint for this error, when defined for the variant — the same
    /// string the `Display` impl bakes in, exposed so consumers can surface it
    /// without re-parsing prose.
    pub fn hint(&self) -> Option<String> {
        match self {
            ValidationError::TypeMismatch {
                expected,
                actual,
                default,
                ..
            } => Some(type_mismatch_hint(expected, actual, default.as_deref())),
            ValidationError::BodyDisabled { .. } => Some(body_disabled_hint().to_string()),
            ValidationError::NotInline { .. } => Some(not_inline_hint().to_string()),
            ValidationError::NotPlain { .. } => Some(not_plain_hint().to_string()),
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
/// `number` so diagnostic messages can report the two separately.
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
    if !config.main.body_enabled() && !doc.main().body().is_blank() {
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

        if !card_schema.body_enabled() && !card.body().is_blank() {
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
        // Absence is a completeness concern, not a well-formedness one: an
        // absent field — like a present-null one — is zero-filled at render and
        // raises nothing here.
        if let Some(value) = fields.get(field_name) {
            errors.extend(validate_field(schema, value, &path));
        }
    }

    errors
}

/// Distinguishes the two value sources the conformance core
/// [`validate_value`] serves. The type/enum/format/recursion checks are
/// identical; only the document-authoring concerns differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueContext {
    /// A value parsed from an authored document. Treats present-null as absent
    /// and reports the field's `default:` token alongside a type mismatch.
    Document,
    /// An `example:` or `default:` literal declared in Quill.yaml. Partial
    /// objects are allowed (absent properties are not errors) and the
    /// document-only null/default semantics do not apply.
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
    // Null ≡ absent: a present-null value in a document is treated as omitted
    // (no type error). The `!must_fill` marker is surfaced separately as a
    // warning by `Quill::validate`, not here.
    if ctx == ValueContext::Document && value.as_json().is_null() {
        return vec![];
    }

    let mut errors = Vec::new();

    let type_valid = match field.r#type {
        // In a document a bare bool/number is type-valid as a string (the
        // coercion layer adopts it) — in lockstep with `coerce_value_strict`
        // via `scalar_as_string`. Schema literals stay strict so the blueprint
        // keeps quoting ambiguous string literals.
        // Enum is string-valued data (domain membership is checked separately
        // below), so it is type-valid exactly where a string is.
        FieldType::String | FieldType::Enum => {
            value.as_str().is_some()
                || (ctx == ValueContext::Document
                    && super::config::scalar_as_string(value.as_json()).is_some())
        }
        // Post-coercion (Document) a richtext/plaintext value is a canonical
        // corpus object; an authored `default`/`example` (Schema) is a string
        // (markdown for richtext, literal for plaintext). Accept both shapes —
        // the corpus's own invariants were enforced at coercion, and a bare
        // scalar still stringifies. The plaintext-specific plain constraint is
        // checked in the shape pass below, parallel to the inline check.
        FieldType::RichText { .. } | FieldType::PlainText { .. } => {
            value.as_json().is_object()
                || value.as_str().is_some()
                || (ctx == ValueContext::Document
                    && super::config::scalar_as_string(value.as_json()).is_some())
        }
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
                // Scalar elements (`string[]`, `integer[]`, `richtext[]`, …)
                // are type-checked element-wise; object elements recurse into
                // their properties via the Object branch.
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
                        // Absent object property: completeness, not
                        // well-formedness. Like a top-level absent field, it
                        // zero-fills at render and raises nothing here.
                        if let Some(property_value) = object.get(property_name) {
                            errors.extend(validate_value(
                                property_schema,
                                &QuillValue::from_json(property_value.clone()),
                                &property_path,
                                ctx,
                            ));
                        }
                    }
                }
                true
            }
            None => false,
        },
    };

    // Corpus shape checks, run only on a type-valid value (a mistyped value
    // already raises TypeMismatch below, and a null/absent field zero-fills to
    // the empty corpus, which is both inline and plain). Mirror the
    // coercion-layer checks so a corpus that bypassed coercion (e.g. a direct
    // `validate_document`) is still caught. A decode failure is not this layer's
    // error to report — swallow it and flag only a well-formed but mis-shaped
    // corpus.
    if type_valid {
        match field.r#type {
            FieldType::RichText { inline: true } => {
                let parsed =
                    crate::document::decode_richtext_value(value.as_json()).and_then(Result::ok);
                if let Some(rt) = parsed {
                    if !rt.is_inline() {
                        errors.push(ValidationError::NotInline {
                            path: path.to_string(),
                        });
                    }
                }
            }
            FieldType::PlainText { inline } => {
                // Plaintext strings are literal, not markdown, so a schema
                // literal decodes through the literal codec; a Document value is
                // a canonical corpus object. The plain constraint is primary;
                // the single-line constraint applies only when `inline`. A decode
                // error is another layer's to report (swallowed via `.ok()`).
                if let Some(rt) = crate::document::decode_plaintext_value(value.as_json())
                    .and_then(Result::ok)
                {
                    if !rt.is_plain() {
                        errors.push(ValidationError::NotPlain {
                            path: path.to_string(),
                        });
                    } else if inline && !rt.is_inline() {
                        errors.push(ValidationError::NotInline {
                            path: path.to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

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
/// apply null≡absent leniency, and never attaches a `default:` token to a type
/// mismatch (partial examples/defaults are intentional and valid).
pub(crate) fn validate_schema_literal(
    schema: &FieldSchema,
    value: &QuillValue,
    path: &str,
) -> Vec<ValidationError> {
    validate_value(schema, value, path, ValueContext::SchemaLiteral)
}

fn expected_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::DateTime => "string",
        // Enum is a closed string domain; a mistyped value reports the base type.
        FieldType::Enum => "string",
        FieldType::RichText { .. } => "richtext",
        FieldType::PlainText { .. } => "plaintext",
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
        let main = Card::from_parts(p, quillmark_richtext::RichText::empty());
        Document::from_main_and_cards(main, cards)
    }

    fn typed_card(tag: &str, fields: &[(&str, serde_json::Value)]) -> Card {
        let mut card = Card::new(tag).unwrap();
        for (k, v) in fields {
            card.store_field(k, QuillValue::from_json(v.clone())).unwrap();
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
        // A bare scalar coerces into a string; an array does not, so it
        // raises a string TypeMismatch.
        let config = config_with("    title:\n      type: string\n      default: \"\"", "");
        let doc = doc_from_fm(&[("title", json!([1, 2, 3]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual, source_token, .. }
            if path == "title" && expected == "string" && actual == "array" && source_token == "[…]"
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
    fn absent_unendorsed_field_raises_nothing() {
        // A field with no `default:` absent from the document is a completeness
        // concern, not a well-formedness one: validation is clean and the field
        // zero-fills at render.
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn present_null_is_treated_as_absent() {
        // `memo_for:` (a bare/null value) carries no data, so it validates the
        // same as an omitted field — no type mismatch — for every type.
        let config = config_with(
            "    memo_for:\n      type: string\n    n:\n      type: integer",
            "",
        );
        let doc = doc_from_fm(&[("memo_for", json!(null)), ("n", json!(null))]);
        assert!(
            validate_typed_document(&config, &doc).is_ok(),
            "present-null must validate like absence"
        );
    }

    #[test]
    fn missing_field_with_default_is_ok() {
        // Endorsed field absent from document → no error; default applies.
        let config = config_with("    memo_for:\n      type: string\n      default: \"\"", "");
        let doc = doc_from_fm(&[]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn absent_object_property_raises_nothing() {
        // Property `name` is Unendorsed and absent from the row. Like a
        // top-level absent field, this is a completeness concern, not a
        // validation error.
        let config = config_with(
            "    recipients:\n      type: array\n      default: []\n      items:\n        type: object\n        properties:\n          name:\n            type: string\n          org:\n            type: string\n            default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "org": "HQ" }]))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    // NOTE: top-level typed-dictionary fields (`type: object` with `properties`)
    // are supported. Coverage lives in the `schema.rs` transform-schema tests
    // (typed tables/dicts) and the blueprint tests. Freeform objects without
    // properties are rejected at config parse time.

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
        // A type-mismatched card field anchors the `cards.<kind>[<i>].<field>`
        // path shape (absence does not raise, so we exercise the path via a
        // well-formedness error instead).
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![typed_card(
                "indorsement",
                &[("signature_block", json!([1, 2, 3]))],
            )],
        );
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::TypeMismatch { path, .. } if path == "cards.indorsement[0].signature_block")
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
        prose_card.revise_body("Should not be here.").unwrap();
        let doc = doc_with_typed_cards(&[], vec![prose_card]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "skills" && path == "cards.skills[0].body"
        )));

        let mut ws_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        ws_card.revise_body("\n   \n").unwrap();
        let ok_doc = doc_with_typed_cards(&[], vec![ws_card]);
        assert!(validate_typed_document(&config, &ok_doc).is_ok());
    }

    #[test]
    fn to_diagnostic_carries_path_code_and_hint() {
        let err = ValidationError::TypeMismatch {
            path: "cards.indorsement[0].signature_block".to_string(),
            expected: "string".to_string(),
            actual: "integer".to_string(),
            source_token: "42".to_string(),
            default: None,
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("validation::type_mismatch"));
        assert_eq!(
            diag.path.as_deref(),
            Some("cards.indorsement[0].signature_block")
        );
        assert_eq!(diag.severity, Severity::Error);
        let hint = diag
            .hint
            .as_deref()
            .expect("type_mismatch diagnostic should carry a hint");
        assert!(
            hint.contains("string"),
            "hint missing expected type: {hint}"
        );
    }

    #[test]
    fn type_mismatch_diagnostic_carries_hint_matching_message() {
        // The structured hint must equal the exit clause baked into the
        // prose message, so consumers never need to re-parse.
        // An array under a `string` schema is a genuine mismatch (not a bare
        // scalar the coercion layer can adopt), so it still raises TypeMismatch.
        let config = config_with(
            "    build_number:\n      type: string\n      default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("build_number", json!([1, 2, 3]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let err = errors
            .iter()
            .find(|e| matches!(e, ValidationError::TypeMismatch { .. }))
            .expect("expected TypeMismatch");
        let diag = err.to_diagnostic();
        let hint = diag
            .hint
            .expect("TypeMismatch diagnostic should carry a hint");
        assert!(
            err.to_string().ends_with(&hint),
            "message tail must equal hint; msg={msg}, hint={hint}",
            msg = err,
        );
        assert!(hint.contains("provide a value of type"));
    }

    #[test]
    fn body_disabled_diagnostic_carries_hint() {
        let err = ValidationError::BodyDisabled {
            path: "cards.skills[0].body".to_string(),
            card: "skills".to_string(),
        };
        let diag = err.to_diagnostic();
        let hint = diag
            .hint
            .expect("BodyDisabled diagnostic should carry a hint");
        assert!(hint.contains("remove the body content"));
    }

    #[test]
    fn bare_scalar_into_string_field_is_valid() {
        // Gracious scalar→string: a bare integer/boolean/number under a
        // `string` schema is unambiguously representable as its canonical text,
        // so it validates (the coercion layer adopts the token). No
        // TypeMismatch — see `quill::config::scalar_as_string`.
        for value in [json!(42), json!(true), json!(1.5)] {
            let config = config_with(
                "    build_number:\n      type: string\n      default: \"\"",
                "",
            );
            let doc = doc_from_fm(&[("build_number", value.clone())]);
            assert!(
                validate_typed_document(&config, &doc).is_ok(),
                "bare scalar {value} should validate as a string"
            );
        }
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
        let main = Card::from_parts(
            p,
            crate::document::import_body("Body content that should not be here.").unwrap(),
        );
        let doc = Document::from_main_and_cards(main, vec![]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "main" && path == "main.body"
        )));
    }

    #[test]
    fn rejects_richtext_inline_with_multi_block_corpus() {
        // A pre-built two-paragraph corpus reaches the validator directly (no
        // coercion), so the validation-layer NotInline backstop must fire.
        let config = config_with("    tag:\n      type: richtext\n      inline: true", "");
        let rt = quillmark_richtext::import::from_markdown("one\n\ntwo").unwrap();
        let corpus = quillmark_richtext::serial::to_canonical_value(&rt);
        let doc = doc_from_fm(&[("tag", corpus)]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::NotInline { path } if path == "tag"
        )));
    }

    #[test]
    fn accepts_richtext_inline_single_para_corpus() {
        let config = config_with("    tag:\n      type: richtext\n      inline: true", "");
        let rt = quillmark_richtext::import::from_markdown("one line only").unwrap();
        let corpus = quillmark_richtext::serial::to_canonical_value(&rt);
        let doc = doc_from_fm(&[("tag", corpus)]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }
}
