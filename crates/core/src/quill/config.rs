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
use super::{BodyCardSchema, LeafSchema, FieldSchema, FieldType, UiCardSchema, UiFieldSchema};

/// Top-level configuration for a Quillmark project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuillConfig {
    /// Quill package name
    pub name: String,
    /// Human-readable description of the quill itself (parsed from
    /// `quill.description`). Distinct from `main.description`, which describes
    /// the main leaf's schema.
    pub description: String,
    /// The entry-point leaf schema (parsed from the Quill.yaml `main:` section).
    pub main: LeafSchema,
    /// Named, composable leaf-type schemas (parsed from the Quill.yaml
    /// `leaf_kinds:` section). Does not include `main`.
    pub leaf_kinds: Vec<LeafSchema>,
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
struct LeafSchemaDef {
    pub description: Option<String>,
    // Declared so `deny_unknown_fields` accepts a `fields:` block on a leaf.
    // Fields are re-parsed via `parse_fields_with_order` for ordering.
    #[allow(dead_code)]
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
    pub ui: Option<UiCardSchema>,
    pub body: Option<BodyCardSchema>,
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
    /// Returns a named leaf-type schema by name.
    pub fn leaf_kind(&self, name: &str) -> Option<&LeafSchema> {
        self.leaf_kinds.iter().find(|leaf| leaf.name == name)
    }

    /// Full schema including `ui` hints.
    ///
    /// `main.fields` is prefixed with a required `QUILL` entry (`const = name@version`);
    /// each `leaf_kinds[<name>].fields` is prefixed with a required `KIND` entry
    /// (`const = <name>`). Identity (`name`, `version`, etc.) and the bundled
    /// example live elsewhere on the host's metadata surface.
    pub fn schema(&self) -> serde_json::Value {
        let canonical_ref = format!("{}@{}", self.name, self.version);

        let mut obj = serde_json::Map::new();

        let mut main_value = serde_json::to_value(&self.main).unwrap_or(serde_json::Value::Null);
        Self::prepend_sentinel_field(
            &mut main_value,
            "QUILL",
            &canonical_ref,
            "Canonical quill reference. Must be exactly this value as the QUILL: sentinel in the document frontmatter.",
        );
        obj.insert("main".to_string(), main_value);

        if !self.leaf_kinds.is_empty() {
            let leaf_kinds: BTreeMap<String, serde_json::Value> = self
                .leaf_kinds
                .iter()
                .map(|leaf| {
                    let mut leaf_value =
                        serde_json::to_value(leaf).unwrap_or(serde_json::Value::Null);
                    Self::prepend_sentinel_field(
                        &mut leaf_value,
                        "KIND",
                        &leaf.name,
                        "Leaf kind name. Must be exactly this value as the KIND: sentinel in the leaf body.",
                    );
                    (leaf.name.clone(), leaf_value)
                })
                .collect();
            obj.insert(
                "leaf_kinds".to_string(),
                serde_json::to_value(&leaf_kinds).unwrap_or(serde_json::Value::Null),
            );
        }

        serde_json::Value::Object(obj)
    }

    /// Insert a `QUILL`/`KIND` sentinel as the first entry of a leaf's `fields`.
    fn prepend_sentinel_field(
        leaf_value: &mut serde_json::Value,
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
        if let Some(serde_json::Value::Object(fields)) = leaf_value.get_mut("fields") {
            let existing = std::mem::take(fields);
            fields.insert(key.to_string(), sentinel);
            fields.extend(existing);
        }
    }

    /// Coerce typed frontmatter fields (IndexMap, no LEAVES/BODY keys).
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

    /// Coerce typed fields for a single leaf (IndexMap, no KIND/BODY keys).
    ///
    /// Returns the input unchanged when the leaf tag is unknown.
    pub fn coerce_leaf(
        &self,
        leaf_tag: &str,
        fields: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let Some(leaf_schema) = self.leaf_kind(leaf_tag) else {
            return Ok(fields.clone());
        };
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in fields {
            if let Some(field_schema) = leaf_schema.fields.get(field_name) {
                let path = format!("leaf_kinds.{leaf_tag}.{field_name}");
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

                if let Some(props) = &field_schema.properties {
                    let mut out = Vec::with_capacity(arr.len());
                    for (idx, elem) in arr.iter().enumerate() {
                        if let Some(obj) = elem.as_object() {
                            let coerced_obj =
                                Self::coerce_object_props(obj, props, &format!("{path}[{idx}]"))?;
                            out.push(serde_json::Value::Object(coerced_obj));
                        } else {
                            out.push(elem.clone());
                        }
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
                        let coerced_obj = Self::coerce_object_props(obj, props, path)?;
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

    /// Walk `obj`'s keys, coercing any that match `props` against the matching
    /// schema and copying any others through verbatim. `parent_path` is the
    /// breadcrumb for the enclosing scope (e.g. `"foo[3]"` or `"foo"`); each
    /// child's path is `"{parent_path}.{k}"`.
    fn coerce_object_props(
        obj: &serde_json::Map<String, serde_json::Value>,
        props: &std::collections::BTreeMap<String, Box<super::FieldSchema>>,
        parent_path: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, CoercionError> {
        let mut out = serde_json::Map::new();
        for (k, v) in obj {
            if let Some(prop_schema) = props.get(k) {
                let child_path = format!("{parent_path}.{k}");
                out.insert(
                    k.clone(),
                    Self::coerce_value_strict(
                        &QuillValue::from_json(v.clone()),
                        prop_schema,
                        &child_path,
                    )?
                    .into_json(),
                );
            } else {
                out.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
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
            if let Some(props) = &schema.properties {
                for prop_schema in props.values() {
                    if Self::has_disallowed_nested_object(prop_schema, false) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Reject multi-line descriptions. Single-line is required so the leading
    /// `# <description>` blueprint slot stays one line and the field-comment
    /// stack remains parseable for LLM consumers.
    fn validate_description_singleline(
        desc: Option<&str>,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        if let Some(d) = desc {
            if d.contains('\n') {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{} description must be a single line; multi-line \
                             descriptions are not allowed.",
                            owner_label
                        ),
                    )
                    .with_code("quill::description_multiline".to_string()),
                );
            }
        }
    }

    /// Reject `>`, `;`, `|` in enum literals. These characters are reserved by
    /// the blueprint inline annotation grammar (`<format>` close, role
    /// separator, enum value separator) and have no escape syntax.
    fn validate_enum_literals(
        field: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        if let Some(values) = &field.enum_values {
            for v in values {
                if v.contains('>') || v.contains(';') || v.contains('|') {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{} enum value '{}' contains a reserved character \
                                 ('>', ';', or '|') that conflicts with the \
                                 blueprint inline annotation grammar.",
                                owner_label, v
                            ),
                        )
                        .with_code("quill::format_literal_reserved_char".to_string()),
                    );
                }
            }
        }
    }

    /// Recursively validate field-level blueprint constraints across the field
    /// and any nested object properties.
    fn validate_field_blueprint_constraints(
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        Self::validate_description_singleline(schema.description.as_deref(), owner_label, errors);
        Self::validate_enum_literals(schema, owner_label, errors);
        if let Some(props) = &schema.properties {
            for (name, prop) in props {
                let nested = format!("{}.{}", owner_label, name);
                Self::validate_field_blueprint_constraints(prop, &nested, errors);
            }
        }
    }

    /// Parse fields from a JSON Value map, assigning ui.order based on key_order.
    ///
    /// This helper ensures consistent field ordering logic for both top-level
    /// fields and leaf fields.
    ///
    /// # Arguments
    /// * `fields_map` - The JSON map containing field definitions
    /// * `key_order` - Vector of field names in their definition order
    /// * `context` - Context string for error messages (e.g., "field" or "leaf 'indorsement' field")
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
                    // Typed dictionaries (type: object with properties) are supported.
                    // Freeform objects (no properties) and objects nested inside
                    // typed-dictionary properties are not.
                    if schema.r#type == FieldType::Object {
                        if schema.properties.is_none() {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Field '{}' has type: object but no properties defined. \
                                        Declare a properties map, or use type: array with \
                                        a properties map for a list of objects.",
                                        field_name
                                    ),
                                )
                                .with_code("quill::object_missing_properties".to_string()),
                            );
                            continue;
                        }
                        // Properties of a typed dictionary may not themselves be objects.
                        if Self::has_disallowed_nested_object(&schema, true) {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Field '{}' contains a nested type: object property, \
                                        which is not supported. Properties of a typed dictionary \
                                        may not themselves be objects.",
                                        field_name
                                    ),
                                )
                                .with_code("quill::nested_object_not_supported".to_string()),
                            );
                            continue;
                        }
                        // Typed dictionary — fall through to normal processing.
                    } else if Self::has_disallowed_nested_object(&schema, false) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!(
                                    "Field '{}' uses nested type: object, which is not supported. \
                                    Use type: array with a properties map for a list of objects.",
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
                            title: None,
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

                    let owner = format!("{} '{}'", context, field_name);
                    Self::validate_field_blueprint_constraints(&schema, &owner, errors);

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
                        .with_hint(format!("Valid keys are: {}", KNOWN_QUILL_KEYS.join(", "))),
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
                    .with_hint(
                        "Add 'name: your_quill_name' under the 'quill:' section.".to_string(),
                    ),
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
            Some(d) if !d.trim().is_empty() => {
                Self::validate_description_singleline(Some(d), "quill", &mut errors);
                d.to_string()
            }
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
                        .with_hint("Valid key under 'ui' is: title.".to_string()),
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

        // Reject unknown top-level sections. Known sections are: quill, main, leaf_kinds,
        // and the backend name (e.g. typst). Everything else is a mistake. `fields` gets
        // a targeted hint since it's the most common shape mistake.
        if let Some(top_obj) = quill_yaml_val.as_object() {
            for key in top_obj.keys() {
                let is_known = key == "quill"
                    || key == "main"
                    || key == "leaf_kinds"
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
                        "Valid top-level sections are: quill, main, leaf_kinds{}",
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
        let main_ui: Option<UiCardSchema> = match main_obj_opt
            .and_then(|main_obj| main_obj.get("ui"))
            .cloned()
        {
            None => None,
            Some(v) => match serde_json::from_value::<UiCardSchema>(v) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    errors.push(
                        Diagnostic::new(Severity::Error, format!("Invalid 'main.ui' block: {}", e))
                            .with_code("quill::invalid_ui".to_string())
                            .with_hint("Valid key under 'ui' is: title.".to_string()),
                    );
                    None
                }
            },
        };

        // Extract main.body (optional). Fail loudly on malformed body metadata.
        let main_body: Option<BodyCardSchema> = match main_obj_opt
            .and_then(|main_obj| main_obj.get("body"))
            .cloned()
        {
            None => None,
            Some(v) => match serde_json::from_value::<BodyCardSchema>(v) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Invalid 'main.body' block: {}", e),
                        )
                        .with_code("quill::invalid_body".to_string())
                        .with_hint("Valid keys under 'body' are: enabled, example.".to_string()),
                    );
                    None
                }
            },
        };

        // Extract main.description (optional, authored under `main:` like any
        // other leaf type). This is independent of `quill.description`.
        let main_description = main_obj_opt
            .and_then(|main_obj| main_obj.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Self::validate_description_singleline(main_description.as_deref(), "main", &mut errors);

        // The main entry-point leaf.
        let main = LeafSchema {
            name: "main".to_string(),
            description: main_description,
            fields,
            ui: main_ui.or(ui_section),
            body: main_body,
        };

        // Extract [leaf_kinds] section (optional)
        let mut leaf_kinds: Vec<LeafSchema> = Vec::new();
        if let Some(leaf_kinds_val) = quill_yaml_val.get("leaf_kinds") {
            match leaf_kinds_val.as_object() {
                None => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "'leaf_kinds' section must be an object (mapping of type names to schemas)".to_string(),
                        )
                        .with_code("quill::invalid_card_types".to_string()),
                    );
                }
                Some(leaf_kinds_table) => {
                    for (leaf_name, leaf_value) in leaf_kinds_table {
                        if !crate::document::sentinel::is_valid_tag_name(leaf_name) {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Invalid leaf-type name '{}': names must match \
                                         [a-z_][a-z0-9_]* (lowercase letters, digits, and underscores only).",
                                        leaf_name
                                    ),
                                )
                                .with_code("quill::invalid_card_name".to_string()),
                            );
                            continue;
                        }

                        // Parse leaf basic info using serde
                        let leaf_def: LeafSchemaDef =
                            match serde_json::from_value(leaf_value.clone()) {
                                Ok(d) => d,
                                Err(e) => {
                                    errors.push(
                                        Diagnostic::new(
                                            Severity::Error,
                                            format!(
                                                "Failed to parse leaf_kind '{}': {}",
                                                leaf_name, e
                                            ),
                                        )
                                        .with_code("quill::invalid_card_schema".to_string()),
                                    );
                                    continue;
                                }
                            };

                        // Parse leaf fields
                        let leaf_fields = if let Some(leaf_fields_table) =
                            leaf_value.get("fields").and_then(|v| v.as_object())
                        {
                            let leaf_field_order: Vec<String> =
                                leaf_fields_table.keys().cloned().collect();
                            Self::parse_fields_with_order(
                                leaf_fields_table,
                                &leaf_field_order,
                                &format!("leaf_kind '{}' field", leaf_name),
                                &mut errors,
                            )
                        } else {
                            BTreeMap::new()
                        };

                        Self::validate_description_singleline(
                            leaf_def.description.as_deref(),
                            &format!("leaf_kind '{}'", leaf_name),
                            &mut errors,
                        );
                        leaf_kinds.push(LeafSchema {
                            name: leaf_name.clone(),
                            description: leaf_def.description,
                            fields: leaf_fields,
                            ui: leaf_def.ui,
                            body: leaf_def.body,
                        });
                    }
                }
            }
        }

        // Warn when `body.example` is set together with `body.enabled: false` —
        // the example has no effect since the body editor is disabled.
        let warn_example_unused = |label: &str,
                                   body: &Option<BodyCardSchema>|
         -> Option<Diagnostic> {
            let body = body.as_ref()?;
            if body.enabled == Some(false) && body.example.is_some() {
                Some(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "`{label}.body.example` is set but `{label}.body.enabled` is false; the example will have no effect"
                        ),
                    )
                    .with_code("quill::body_example_unused".to_string())
                    .with_hint(
                        "Set `body.enabled: true` to surface the example, or remove `body.example`."
                            .to_string(),
                    ),
                )
            } else {
                None
            }
        };
        if let Some(d) = warn_example_unused("main", &main.body) {
            warnings.push(d);
        }
        for leaf in &leaf_kinds {
            if let Some(d) = warn_example_unused(&format!("leaf_kinds.{}", leaf.name), &leaf.body) {
                warnings.push(d);
            }
        }

        // Error when `body.example` contains a line that the document parser
        // would interpret as a metadata fence (`---` with up to 3 leading
        // spaces and optional trailing whitespace). Such a line would split the
        // blueprint body region into a new fence, corrupting document structure.
        let err_example_contains_fence = |label: &str,
                                          body: &Option<BodyCardSchema>|
         -> Option<Diagnostic> {
            let example = body.as_ref()?.example.as_deref()?;
            if example_contains_fence_line(example) {
                Some(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{label}.body.example` contains a line that would be parsed as a metadata fence (`---`); this would corrupt the blueprint"
                        ),
                    )
                    .with_code("quill::body_example_contains_fence".to_string())
                    .with_hint(
                        "Remove or reword any line that is exactly `---` (with up to 3 leading spaces and optional trailing whitespace).".to_string(),
                    ),
                )
            } else {
                None
            }
        };
        if let Some(d) = err_example_contains_fence("main", &main.body) {
            errors.push(d);
        }
        for leaf in &leaf_kinds {
            if let Some(d) =
                err_example_contains_fence(&format!("leaf_kinds.{}", leaf.name), &leaf.body)
            {
                errors.push(d);
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
                leaf_kinds,
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

/// Returns true if any line in `text` would be parsed as a metadata-fence
/// marker by the document parser. Mirrors `document::fences::is_fence_marker_line`:
/// up to 3 leading spaces (no leading tab), then `---`, then only whitespace.
fn example_contains_fence_line(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let indent = line.bytes().take_while(|&b| b == b' ').count();
        if indent > 3 || line.as_bytes().first() == Some(&b'\t') {
            return false;
        }
        matches!(
            line[indent..].strip_prefix("---"),
            Some(rest) if rest.chars().all(|c| c == ' ' || c == '\t')
        )
    })
}
