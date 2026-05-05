//! Quill configuration parsing and normalization.
use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;

use indexmap::IndexMap;

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::error::{Diagnostic, Severity};
use crate::value::QuillValue;

use super::formats::DATE_FORMAT;
use super::{CardSchema, FieldSchema, FieldType, UiCardSchema, UiFieldSchema};

/// Top-level configuration for a Quillmark project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuillConfig {
    /// Quill package name
    pub name: String,
    /// Human-readable description of the quill itself (parsed from
    /// `quill.description`). Distinct from `main.description`, which describes
    /// the main card's schema.
    pub description: String,
    /// The entry-point card schema (parsed from the Quill.yaml `main:` section).
    pub main: CardSchema,
    /// Named, composable card-type schemas (parsed from the Quill.yaml
    /// `card_types:` section). Does not include `main`.
    pub card_types: Vec<CardSchema>,
    /// Backend to use for rendering (e.g., "typst", "html")
    pub backend: String,
    /// Version of the Quillmark spec
    pub version: String,
    /// Author of the project
    pub author: String,
    /// Example data file for preview
    pub example_file: Option<String>,
    /// Loaded markdown example content from `Quill.example`/`Quill.example_file`
    pub example_markdown: Option<String>,
    /// Plate file (template)
    pub plate_file: Option<String>,
    /// Backend-specific configuration parsed from the top-level YAML section
    /// whose key matches `backend` (e.g. `[typst]`, `[html]`).
    #[serde(default)]
    pub backend_config: HashMap<String, QuillValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CardSchemaDef {
    pub description: Option<String>,
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
    pub ui: Option<UiCardSchema>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CoercionError {
    #[error("cannot coerce `{value}` to type `{target}` at `{path}`: {reason}")]
    Uncoercible {
        path: String,
        value: String,
        target: String,
        reason: String,
    },
}

impl QuillConfig {
    /// Returns a named card-type schema by name.
    pub fn card_type(&self, name: &str) -> Option<&CardSchema> {
        self.card_types.iter().find(|card| card.name == name)
    }

    /// Structural schema plus `ui` hints — for form builders.
    pub fn form_schema(&self) -> serde_json::Value {
        self.build_schema(true)
    }

    /// Structural schema with `ui` keys stripped — for LLM/MCP consumers.
    ///
    /// `main.fields` is prefixed with a required `QUILL` entry (`const = name@version`);
    /// each `card_types[<name>].fields` is prefixed with a required `CARD` entry
    /// (`const = <name>`). Identity (`name`, `version`, etc.) and the bundled
    /// example live elsewhere on the host's metadata surface.
    pub fn schema(&self) -> serde_json::Value {
        self.build_schema(false)
    }

    fn build_schema(&self, with_ui: bool) -> serde_json::Value {
        let canonical_ref = format!("{}@{}", self.name, self.version);

        let mut obj = serde_json::Map::new();

        let mut main_value = serde_json::to_value(&self.main).unwrap_or(serde_json::Value::Null);
        Self::prepend_sentinel_field(
            &mut main_value,
            "QUILL",
            &canonical_ref,
            "Canonical quill reference. Must be exactly this value as the QUILL: sentinel in the document frontmatter.",
        );
        if !with_ui {
            Self::strip_ui_recursive(&mut main_value);
        }
        obj.insert("main".to_string(), main_value);

        if !self.card_types.is_empty() {
            let card_types: BTreeMap<String, serde_json::Value> = self
                .card_types
                .iter()
                .map(|card| {
                    let mut card_value =
                        serde_json::to_value(card).unwrap_or(serde_json::Value::Null);
                    Self::prepend_sentinel_field(
                        &mut card_value,
                        "CARD",
                        &card.name,
                        "Card type name. Must be exactly this value as the CARD: sentinel in the card frontmatter.",
                    );
                    if !with_ui {
                        Self::strip_ui_recursive(&mut card_value);
                    }
                    (card.name.clone(), card_value)
                })
                .collect();
            obj.insert(
                "card_types".to_string(),
                serde_json::to_value(&card_types).unwrap_or(serde_json::Value::Null),
            );
        }

        serde_json::Value::Object(obj)
    }

    /// Insert a `QUILL`/`CARD` sentinel as the first entry of a card's `fields`.
    fn prepend_sentinel_field(
        card_value: &mut serde_json::Value,
        key: &str,
        const_value: &str,
        description: &str,
    ) {
        let sentinel = serde_json::json!({
            "type": "string",
            "const": const_value,
            "description": description,
            "required": true
        });
        if let Some(serde_json::Value::Object(fields)) = card_value.get_mut("fields") {
            let existing = std::mem::take(fields);
            fields.insert(key.to_string(), sentinel);
            fields.extend(existing);
        }
    }

    fn strip_ui_recursive(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("ui");
                for v in map.values_mut() {
                    Self::strip_ui_recursive(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr.iter_mut() {
                    Self::strip_ui_recursive(v);
                }
            }
            _ => {}
        }
    }

    /// Coerce typed frontmatter fields (IndexMap, no CARDS/BODY keys).
    pub fn coerce_frontmatter(
        &self,
        frontmatter: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in frontmatter {
            if let Some(field_schema) = self.main.fields.get(field_name) {
                let path = field_name.as_str();
                coerced.insert(
                    field_name.clone(),
                    Self::coerce_value_strict(field_value, field_schema, path)?,
                );
            } else {
                coerced.insert(field_name.clone(), field_value.clone());
            }
        }
        Ok(coerced)
    }

    /// Coerce typed fields for a single card (IndexMap, no CARD/BODY keys).
    ///
    /// Returns the input unchanged when the card tag is unknown.
    pub fn coerce_card(
        &self,
        card_tag: &str,
        fields: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let Some(card_schema) = self.card_type(card_tag) else {
            return Ok(fields.clone());
        };
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in fields {
            if let Some(field_schema) = card_schema.fields.get(field_name) {
                let path = format!("card_types.{card_tag}.{field_name}");
                coerced.insert(
                    field_name.clone(),
                    Self::coerce_value_strict(field_value, field_schema, &path)?,
                );
            } else {
                coerced.insert(field_name.clone(), field_value.clone());
            }
        }
        Ok(coerced)
    }

    /// Validate a typed [`crate::document::Document`] against this configuration.
    pub fn validate_document(
        &self,
        doc: &crate::document::Document,
    ) -> Result<(), Vec<super::validation::ValidationError>> {
        super::validation::validate_typed_document(self, doc)
    }

    fn coerce_value_strict(
        value: &QuillValue,
        field_schema: &super::FieldSchema,
        path: &str,
    ) -> Result<QuillValue, CoercionError> {
        use super::FieldType;

        let json_value = value.as_json();
        match field_schema.r#type {
            FieldType::Array => {
                let arr = if let Some(a) = json_value.as_array() {
                    a.clone()
                } else {
                    vec![json_value.clone()]
                };

                if let Some(items_schema) = &field_schema.items {
                    let mut out = Vec::with_capacity(arr.len());
                    for (idx, elem) in arr.iter().enumerate() {
                        let item_path = format!("{path}[{idx}]");
                        let coerced = Self::coerce_value_strict(
                            &QuillValue::from_json(elem.clone()),
                            items_schema,
                            &item_path,
                        )?;
                        out.push(coerced.into_json());
                    }
                    Ok(QuillValue::from_json(serde_json::Value::Array(out)))
                } else {
                    Ok(QuillValue::from_json(serde_json::Value::Array(arr)))
                }
            }
            FieldType::Boolean => {
                if let Some(b) = json_value.as_bool() {
                    return Ok(QuillValue::from_json(serde_json::Value::Bool(b)));
                }
                if let Some(s) = json_value.as_str() {
                    let lower = s.to_lowercase();
                    if lower == "true" {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(true)));
                    } else if lower == "false" {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(false)));
                    }
                }
                if let Some(n) = json_value.as_i64() {
                    return Ok(QuillValue::from_json(serde_json::Value::Bool(n != 0)));
                }
                if let Some(n) = json_value.as_f64() {
                    if n.is_nan() {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(false)));
                    }
                    return Ok(QuillValue::from_json(serde_json::Value::Bool(
                        n.abs() > f64::EPSILON,
                    )));
                }

                Err(CoercionError::Uncoercible {
                    path: path.to_string(),
                    value: json_value.to_string(),
                    target: "boolean".to_string(),
                    reason: "value is not coercible to boolean".to_string(),
                })
            }
            FieldType::Number => {
                if json_value.is_number() {
                    return Ok(value.clone());
                }
                if let Some(s) = json_value.as_str() {
                    if let Ok(i) = s.parse::<i64>() {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    if let Ok(f) = s.parse::<f64>() {
                        if let Some(num) = serde_json::Number::from_f64(f) {
                            return Ok(QuillValue::from_json(num.into()));
                        }
                    }
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: s.to_string(),
                        target: "number".to_string(),
                        reason: "string is not a valid number".to_string(),
                    });
                }
                if let Some(b) = json_value.as_bool() {
                    let n = if b { 1 } else { 0 };
                    return Ok(QuillValue::from_json(serde_json::Value::Number(
                        serde_json::Number::from(n),
                    )));
                }

                Err(CoercionError::Uncoercible {
                    path: path.to_string(),
                    value: json_value.to_string(),
                    target: "number".to_string(),
                    reason: "value is not coercible to number".to_string(),
                })
            }
            FieldType::Integer => {
                if let Some(i) = json_value.as_i64() {
                    return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                }
                if let Some(u) = json_value.as_u64() {
                    if let Ok(i) = i64::try_from(u) {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: json_value.to_string(),
                        target: "integer".to_string(),
                        reason: "integer value exceeds i64 range".to_string(),
                    });
                }
                if let Some(s) = json_value.as_str() {
                    if let Ok(i) = s.parse::<i64>() {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: s.to_string(),
                        target: "integer".to_string(),
                        reason: "string is not a valid integer".to_string(),
                    });
                }
                if let Some(b) = json_value.as_bool() {
                    let n = if b { 1 } else { 0 };
                    return Ok(QuillValue::from_json(serde_json::Value::Number(
                        serde_json::Number::from(n),
                    )));
                }

                Err(CoercionError::Uncoercible {
                    path: path.to_string(),
                    value: json_value.to_string(),
                    target: "integer".to_string(),
                    reason: "value is not coercible to integer".to_string(),
                })
            }
            FieldType::String | FieldType::Markdown => {
                if json_value.is_string() {
                    return Ok(value.clone());
                }
                if let Some(arr) = json_value.as_array() {
                    if arr.len() == 1 {
                        if let Some(s) = arr[0].as_str() {
                            return Ok(QuillValue::from_json(serde_json::Value::String(
                                s.to_string(),
                            )));
                        }
                    }
                }
                Ok(value.clone())
            }
            FieldType::Date | FieldType::DateTime => {
                if json_value.is_null() {
                    return Ok(QuillValue::from_json(serde_json::Value::Null));
                }
                let text = if let Some(s) = json_value.as_str() {
                    if s.is_empty() {
                        return Ok(QuillValue::from_json(serde_json::Value::Null));
                    }
                    s.to_string()
                } else if let Some(arr) = json_value.as_array() {
                    if arr.len() == 1 {
                        if let Some(s) = arr[0].as_str() {
                            s.to_string()
                        } else {
                            return Err(CoercionError::Uncoercible {
                                path: path.to_string(),
                                value: json_value.to_string(),
                                target: field_schema.r#type.as_str().to_string(),
                                reason: "value must be a string".to_string(),
                            });
                        }
                    } else {
                        return Err(CoercionError::Uncoercible {
                            path: path.to_string(),
                            value: json_value.to_string(),
                            target: field_schema.r#type.as_str().to_string(),
                            reason: "value must be a single string".to_string(),
                        });
                    }
                } else {
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: json_value.to_string(),
                        target: field_schema.r#type.as_str().to_string(),
                        reason: "value must be a string".to_string(),
                    });
                };

                let valid = if field_schema.r#type == FieldType::Date {
                    Date::parse(&text, &DATE_FORMAT).is_ok()
                } else {
                    OffsetDateTime::parse(&text, &Rfc3339).is_ok()
                };

                if valid {
                    Ok(QuillValue::from_json(serde_json::Value::String(text)))
                } else {
                    Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: text,
                        target: field_schema.r#type.as_str().to_string(),
                        reason: "invalid date/datetime format".to_string(),
                    })
                }
            }
            FieldType::Object => {
                if let Some(obj) = json_value.as_object() {
                    if let Some(props) = &field_schema.properties {
                        let mut coerced_obj = serde_json::Map::new();
                        for (k, v) in obj {
                            if let Some(prop_schema) = props.get(k) {
                                let child_path = format!("{path}.{k}");
                                coerced_obj.insert(
                                    k.clone(),
                                    Self::coerce_value_strict(
                                        &QuillValue::from_json(v.clone()),
                                        prop_schema,
                                        &child_path,
                                    )?
                                    .into_json(),
                                );
                            } else {
                                coerced_obj.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(QuillValue::from_json(serde_json::Value::Object(
                            coerced_obj,
                        )))
                    } else {
                        Ok(value.clone())
                    }
                } else {
                    Ok(value.clone())
                }
            }
        }
    }

    fn has_disallowed_nested_object(schema: &FieldSchema, allow_object_here: bool) -> bool {
        if schema.r#type == FieldType::Object {
            if !allow_object_here {
                return true;
            }
            if let Some(props) = &schema.properties {
                for prop_schema in props.values() {
                    if Self::has_disallowed_nested_object(prop_schema, false) {
                        return true;
                    }
                }
            }
        }

        if schema.r#type == FieldType::Array {
            if let Some(items_schema) = &schema.items {
                return Self::has_disallowed_nested_object(items_schema, true);
            }
        }

        false
    }

    /// Parse fields from a JSON Value map, assigning ui.order based on key_order.
    ///
    /// This helper ensures consistent field ordering logic for both top-level
    /// fields and card fields.
    ///
    /// # Arguments
    /// * `fields_map` - The JSON map containing field definitions
    /// * `key_order` - Vector of field names in their definition order
    /// * `context` - Context string for error messages (e.g., "field" or "card 'indorsement' field")
    fn parse_fields_with_order(
        fields_map: &serde_json::Map<String, serde_json::Value>,
        key_order: &[String],
        context: &str,
        errors: &mut Vec<Diagnostic>,
    ) -> BTreeMap<String, FieldSchema> {
        let mut fields = BTreeMap::new();
        let mut fallback_counter = 0;

        for (field_name, field_value) in fields_map {
            if !Self::is_snake_case_identifier(field_name) {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "Invalid {} '{}': field keys must be snake_case \
                             (lowercase letters, digits, and underscores only), \
                             and capitalized field keys are reserved.",
                            context, field_name
                        ),
                    )
                    .with_code("quill::invalid_field_name".to_string()),
                );
                continue;
            }

            // Determine order from key_order, or use fallback counter
            let order = if let Some(idx) = key_order.iter().position(|k| k == field_name) {
                idx as i32
            } else {
                let o = key_order.len() as i32 + fallback_counter;
                fallback_counter += 1;
                o
            };

            let quill_value = QuillValue::from_json(field_value.clone());
            match FieldSchema::from_quill_value(field_name.clone(), &quill_value) {
                Ok(mut schema) => {
                    // Reject standalone object/dict fields — object is only valid inside array items.
                    if schema.r#type == FieldType::Object {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!(
                                    "Field '{}' uses standalone type: object, which is not supported. \
                                    Use separate fields with ui.group instead, or use \
                                    type: array with items: {{type: object, properties: {{...}}}}.",
                                    field_name
                                ),
                            )
                            .with_code("quill::standalone_object_not_supported".to_string()),
                        );
                        continue;
                    }

                    if Self::has_disallowed_nested_object(&schema, false) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!(
                                    "Field '{}' uses nested type: object, which is not supported. \
                                    Only object schemas nested under array.items are supported.",
                                    field_name
                                ),
                            )
                            .with_code("quill::nested_object_not_supported".to_string()),
                        );
                        continue;
                    }

                    // Always set ui.order based on position
                    if schema.ui.is_none() {
                        schema.ui = Some(UiFieldSchema {
                            group: None,
                            order: Some(order),
                            compact: None,
                            multiline: None,
                        });
                    } else if let Some(ui) = &mut schema.ui {
                        if ui.order.is_none() {
                            ui.order = Some(order);
                        }
                    }

                    fields.insert(field_name.clone(), schema);
                }
                Err(e) => {
                    let hint = Self::field_parse_hint(field_value);
                    let mut diag = Diagnostic::new(
                        Severity::Error,
                        format!("Failed to parse {} '{}': {}", context, field_name, e),
                    )
                    .with_code("quill::field_parse_error".to_string());
                    if let Some(h) = hint {
                        diag = diag.with_hint(h);
                    }
                    errors.push(diag);
                }
            }
        }

        fields
    }

    /// Produce an actionable hint for common field schema mistakes based on the raw value.
    fn field_parse_hint(field_value: &serde_json::Value) -> Option<String> {
        if let Some(obj) = field_value.as_object() {
            if obj.contains_key("title") {
                return Some(
                    "'title' is not a valid field key; use 'description' instead.".to_string(),
                );
            }
            if let Some(ui_val) = obj.get("ui") {
                if let Some(ui_obj) = ui_val.as_object() {
                    if ui_obj.contains_key("title") {
                        return Some(
                            "'ui.title' is only valid on card type schemas (under card_types:), \
                             not on individual fields."
                                .to_string(),
                        );
                    }
                }
            }
        }
        None
    }

    fn is_snake_case_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }

        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    fn is_valid_card_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() || c == '_' => {}
            _ => return false,
        }

        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    fn is_valid_quill_name(name: &str) -> bool {
        name == "__default__" || Self::is_snake_case_identifier(name)
    }

    /// Parse QuillConfig from YAML content
    pub fn from_yaml(yaml_content: &str) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        match Self::from_yaml_with_warnings(yaml_content) {
            Ok((config, _warnings)) => Ok(config),
            Err(diags) => {
                let msg = diags
                    .iter()
                    .map(|d| d.fmt_pretty())
                    .collect::<Vec<_>>()
                    .join("\n");
                Err(msg.into())
            }
        }
    }

    /// Parse QuillConfig from YAML content while collecting non-fatal warnings.
    ///
    /// Returns `Ok((config, warnings))` on success, or `Err(errors)` containing all
    /// parse/validation errors when the config is invalid. Errors are always collected
    /// exhaustively — callers see every problem, not just the first.
    pub fn from_yaml_with_warnings(
        yaml_content: &str,
    ) -> Result<(Self, Vec<Diagnostic>), Vec<Diagnostic>> {
        let mut warnings: Vec<Diagnostic> = Vec::new();
        let mut errors: Vec<Diagnostic> = Vec::new();

        // Parse YAML into serde_json::Value via serde_saphyr
        // Note: serde_json with "preserve_order" feature is required for this to work as expected
        let quill_yaml_val: serde_json::Value = match serde_saphyr::from_str(yaml_content) {
            Ok(v) => v,
            Err(e) => {
                return Err(vec![Diagnostic::new(
                    Severity::Error,
                    format!("Failed to parse Quill.yaml: {}", e),
                )
                .with_code("quill::yaml_parse_error".to_string())]);
            }
        };

        // Extract [quill] section (required) — fail immediately if absent since all
        // subsequent validation depends on it.
        let quill_section = match quill_yaml_val.get("quill") {
            Some(v) => v,
            None => {
                return Err(vec![Diagnostic::new(
                    Severity::Error,
                    "Missing required 'quill' section in Quill.yaml".to_string(),
                )
                .with_code("quill::missing_section".to_string())
                .with_hint(
                    "Add a 'quill:' section with name, backend, version, and description."
                        .to_string(),
                )]);
            }
        };

        // Validate that no unknown keys appear in the [quill] section.
        const KNOWN_QUILL_KEYS: &[&str] = &[
            "name",
            "backend",
            "description",
            "version",
            "author",
            "example",
            "example_file",
            "plate_file",
            "ui",
        ];
        if let Some(quill_obj) = quill_section.as_object() {
            for key in quill_obj.keys() {
                if !KNOWN_QUILL_KEYS.contains(&key.as_str()) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Unknown key '{}' in 'quill:' section", key),
                        )
                        .with_code("quill::unknown_key".to_string())
                        .with_hint(format!(
                            "Valid keys are: {}",
                            KNOWN_QUILL_KEYS.join(", ")
                        )),
                    );
                }
            }
        }

        // Extract required fields — collect all missing-field errors before returning.
        let name = match quill_section.get("name").and_then(|v| v.as_str()) {
            Some(n) => {
                if !Self::is_valid_quill_name(n) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "Invalid Quill name '{}': quill.name must be snake_case \
                                 (lowercase letters, digits, and underscores only).",
                                n
                            ),
                        )
                        .with_code("quill::invalid_name".to_string())
                        .with_hint(format!(
                            "Rename '{}' to '{}'",
                            n,
                            n.to_lowercase().replace('-', "_")
                        )),
                    );
                }
                n.to_string()
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'name' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_name".to_string())
                    .with_hint("Add 'name: your_quill_name' under the 'quill:' section.".to_string()),
                );
                String::new()
            }
        };

        let backend = match quill_section.get("backend").and_then(|v| v.as_str()) {
            Some(b) => b.to_string(),
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'backend' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_backend".to_string())
                    .with_hint("Add 'backend: typst' (or another supported backend).".to_string()),
                );
                String::new()
            }
        };

        let description = match quill_section.get("description").and_then(|v| v.as_str()) {
            Some(d) if !d.trim().is_empty() => d.to_string(),
            Some(_) => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "'description' field in 'quill' section cannot be empty".to_string(),
                    )
                    .with_code("quill::empty_description".to_string()),
                );
                String::new()
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'description' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_description".to_string())
                    .with_hint("Add a brief 'description:' of what this quill is for.".to_string()),
                );
                String::new()
            }
        };

        // Extract optional fields (now version is required)
        let version = match quill_section.get("version") {
            Some(version_val) => {
                // Handle version as string or number (YAML might parse 1.0 as number)
                let raw = if let Some(s) = version_val.as_str() {
                    s.to_string()
                } else if let Some(n) = version_val.as_f64() {
                    n.to_string()
                } else {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "Invalid 'version' field format".to_string(),
                        )
                        .with_code("quill::invalid_version".to_string())
                        .with_hint("Use semver format: '1.0' or '1.0.0'.".to_string()),
                    );
                    String::new()
                };
                if !raw.is_empty() {
                    use std::str::FromStr;
                    if let Err(e) = crate::version::Version::from_str(&raw) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!("Invalid version '{}': {}", raw, e),
                            )
                            .with_code("quill::invalid_version".to_string())
                            .with_hint("Use semver format: '1.0' or '1.0.0'.".to_string()),
                        );
                    }
                }
                raw
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'version' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_version".to_string())
                    .with_hint("Add 'version: 1.0' under the 'quill:' section.".to_string()),
                );
                String::new()
            }
        };

        let author = quill_section
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let example_file = quill_section
            .get("example")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                quill_section
                    .get("example_file")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });

        let plate_file = quill_section
            .get("plate_file")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let ui_section: Option<UiCardSchema> = match quill_section.get("ui").cloned() {
            None => None,
            Some(v) => match serde_json::from_value::<UiCardSchema>(v) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Invalid 'quill.ui' block: {}", e),
                        )
                        .with_code("quill::invalid_ui".to_string())
                        .with_hint(
                            "Valid keys under 'ui' are: title, hide_body.".to_string(),
                        ),
                    );
                    None
                }
            },
        };

        // Extract optional backend-specific section (keyed by `quill.backend`).
        let mut backend_config = HashMap::new();
        if !backend.is_empty() {
            if let Some(section_val) = quill_yaml_val.get(&backend) {
                if let Some(table) = section_val.as_object() {
                    for (key, value) in table {
                        backend_config.insert(key.clone(), QuillValue::from_json(value.clone()));
                    }
                }
            }
        }

        // Reject unknown top-level sections. Known sections are: quill, main, card_types,
        // and the backend name (e.g. typst). Everything else is a mistake. `fields` gets
        // a targeted hint since it's the most common shape mistake.
        if let Some(top_obj) = quill_yaml_val.as_object() {
            for key in top_obj.keys() {
                let is_known = key == "quill"
                    || key == "main"
                    || key == "card_types"
                    || (!backend.is_empty() && key == &backend);
                if is_known {
                    continue;
                }

                let mut diag = Diagnostic::new(
                    Severity::Error,
                    format!("Unknown top-level section '{}'", key),
                )
                .with_code("quill::unknown_section".to_string());

                diag = if key == "fields" {
                    diag.with_hint(
                        "Root-level `fields` is not supported; use `main.fields` instead."
                            .to_string(),
                    )
                } else {
                    diag.with_hint(format!(
                        "Valid top-level sections are: quill, main, card_types{}",
                        if backend.is_empty() {
                            String::new()
                        } else {
                            format!(", {}", backend)
                        }
                    ))
                };

                errors.push(diag);
            }
        }

        let main_obj_opt = quill_yaml_val.get("main").and_then(|v| v.as_object());

        // Extract main.fields (optional)
        let fields = if let Some(main_obj) = main_obj_opt {
            if let Some(fields_val) = main_obj.get("fields") {
                if let Some(fields_map) = fields_val.as_object() {
                    // With preserve_order feature, keys iterator respects insertion order
                    let field_order: Vec<String> = fields_map.keys().cloned().collect();
                    Self::parse_fields_with_order(
                        fields_map,
                        &field_order,
                        "field schema",
                        &mut errors,
                    )
                } else {
                    BTreeMap::new()
                }
            } else {
                BTreeMap::new()
            }
        } else {
            BTreeMap::new()
        };

        // Extract main.ui (optional). Fail loudly on malformed UI metadata rather
        // than silently dropping it — see `quill.ui` handling above.
        let main_ui: Option<UiCardSchema> =
            match main_obj_opt.and_then(|main_obj| main_obj.get("ui")).cloned() {
                None => None,
                Some(v) => match serde_json::from_value::<UiCardSchema>(v) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!("Invalid 'main.ui' block: {}", e),
                            )
                            .with_code("quill::invalid_ui".to_string())
                            .with_hint(
                                "Valid keys under 'ui' are: title, hide_body.".to_string(),
                            ),
                        );
                        None
                    }
                },
            };

        // Extract main.description (optional, authored under `main:` like any
        // other card type). This is independent of `quill.description`.
        let main_description = main_obj_opt
            .and_then(|main_obj| main_obj.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // The main entry-point card.
        let main = CardSchema {
            name: "main".to_string(),
            description: main_description,
            fields,
            ui: main_ui.or(ui_section),
        };

        // Extract [card_types] section (optional)
        let mut card_types: Vec<CardSchema> = Vec::new();
        if let Some(card_types_val) = quill_yaml_val.get("card_types") {
            match card_types_val.as_object() {
                None => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "'card_types' section must be an object (mapping of type names to schemas)".to_string(),
                        )
                        .with_code("quill::invalid_card_types".to_string()),
                    );
                }
                Some(card_types_table) => {
                    for (card_name, card_value) in card_types_table {
                        if !Self::is_valid_card_identifier(card_name) {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Invalid card-type name '{}': names must match \
                                         [a-z_][a-z0-9_]* (lowercase letters, digits, and underscores only).",
                                        card_name
                                    ),
                                )
                                .with_code("quill::invalid_card_name".to_string()),
                            );
                            continue;
                        }

                        // Parse card basic info using serde
                        let card_def: CardSchemaDef =
                            match serde_json::from_value(card_value.clone()) {
                                Ok(d) => d,
                                Err(e) => {
                                    errors.push(
                                        Diagnostic::new(
                                            Severity::Error,
                                            format!(
                                                "Failed to parse card_type '{}': {}",
                                                card_name, e
                                            ),
                                        )
                                        .with_code("quill::invalid_card_schema".to_string()),
                                    );
                                    continue;
                                }
                            };

                        // Parse card fields
                        let card_fields = if let Some(card_fields_table) =
                            card_value.get("fields").and_then(|v| v.as_object())
                        {
                            let card_field_order: Vec<String> =
                                card_fields_table.keys().cloned().collect();
                            Self::parse_fields_with_order(
                                card_fields_table,
                                &card_field_order,
                                &format!("card_type '{}' field", card_name),
                                &mut errors,
                            )
                        } else {
                            BTreeMap::new()
                        };

                        card_types.push(CardSchema {
                            name: card_name.clone(),
                            description: card_def.description,
                            fields: card_fields,
                            ui: card_def.ui,
                        });
                    }
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok((
            QuillConfig {
                name,
                description,
                main,
                card_types,
                backend,
                version,
                author,
                example_file,
                example_markdown: None,
                plate_file,
                backend_config,
            },
            warnings,
        ))
    }
}
