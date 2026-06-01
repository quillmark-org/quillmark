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
use super::{BodyCardSchema, CardSchema, FieldSchema, FieldType, UiCardSchema, UiFieldSchema};

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
    /// Named, composable card-kind schemas (parsed from the Quill.yaml
    /// `card_kinds:` section). Does not include `main`.
    pub card_kinds: Vec<CardSchema>,
    /// Backend to use for rendering (e.g., "typst", "html")
    pub backend: String,
    /// Version of the Quillmark spec
    pub version: String,
    /// Author of the project
    pub author: String,
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
    // Declared so `deny_unknown_fields` accepts a `fields:` block on a card.
    // Fields are re-parsed via `parse_fields_with_order` for ordering.
    #[allow(dead_code)]
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
    pub ui: Option<UiCardSchema>,
    pub body: Option<BodyCardSchema>,
}

/// Depth context for [`QuillConfig::validate_field_schema_shape`]. Encodes
/// which shapes are legal at the current nesting level, so the one-level
/// nesting contract is enforced by a single recursive walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShapePosition {
    /// A field declared directly on a card: scalar, object, or array.
    Top,
    /// An array's `items`: scalar or object (typed-table row), not an array.
    ArrayItem,
    /// An object's property: scalar only.
    Leaf,
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
    /// Returns a named card-kind schema by name.
    pub fn card_kind(&self, name: &str) -> Option<&CardSchema> {
        self.card_kinds.iter().find(|card| card.name == name)
    }

    /// Full schema including `ui` hints.
    ///
    /// Describes the user-fillable fields of the main card and each named
    /// card kind. The quill reference (constructed as `name@version` from
    /// quill metadata) and card-kind discriminators are document-level
    /// metadata, not fields, so they do not appear here.
    pub fn schema(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        let main_value = serde_json::to_value(&self.main).unwrap_or(serde_json::Value::Null);
        obj.insert("main".to_string(), main_value);

        if !self.card_kinds.is_empty() {
            let card_kinds: BTreeMap<String, serde_json::Value> = self
                .card_kinds
                .iter()
                .map(|card| {
                    let card_value = serde_json::to_value(card).unwrap_or(serde_json::Value::Null);
                    (card.name.clone(), card_value)
                })
                .collect();
            obj.insert(
                "card_kinds".to_string(),
                serde_json::to_value(&card_kinds).unwrap_or(serde_json::Value::Null),
            );
        }

        serde_json::Value::Object(obj)
    }

    /// Coerce typed payload fields (IndexMap of user fields only).
    pub fn coerce_payload(
        &self,
        payload: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in payload {
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

    /// Coerce typed fields for a single card (IndexMap of user fields only).
    ///
    /// Returns the input unchanged when the card kind is unknown.
    pub fn coerce_card(
        &self,
        card_kind: &str,
        fields: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let Some(card_schema) = self.card_kind(card_kind) else {
            return Ok(fields.clone());
        };
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in fields {
            if let Some(field_schema) = card_schema.fields.get(field_name) {
                let path = format!("card_kinds.{card_kind}.{field_name}");
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

        // Sentinel pass-through: the literal `<must-fill>` string survives
        // coercion unchanged so the validation layer can surface a
        // placeholder diagnostic instead of a type-coercion error. For
        // markdown the sentinel sits inside the block scalar, which already
        // decodes as a string, so the same comparison covers both cases.
        if let Some(s) = json_value.as_str() {
            let candidate = if matches!(field_schema.r#type, FieldType::Markdown) {
                s.trim()
            } else {
                s
            };
            if candidate == super::validation::MUST_FILL_SENTINEL {
                return Ok(value.clone());
            }
        }

        match field_schema.r#type {
            FieldType::Array => {
                let arr = if let Some(a) = json_value.as_array() {
                    a.clone()
                } else {
                    vec![json_value.clone()]
                };

                // Every array carries an element schema (`items`). Coerce each
                // element against it: scalar items (`string[]`, `integer[]`,
                // `markdown[]`) coerce element-wise; object items recurse into
                // the element's `properties` via the Object branch.
                if let Some(items) = &field_schema.items {
                    let mut out = Vec::with_capacity(arr.len());
                    for (idx, elem) in arr.iter().enumerate() {
                        let coerced = Self::coerce_value_strict(
                            &QuillValue::from_json(elem.clone()),
                            items,
                            &format!("{path}[{idx}]"),
                        )?;
                        out.push(coerced.into_json());
                    }
                    Ok(QuillValue::from_json(serde_json::Value::Array(out)))
                } else {
                    // Defensive fallback: schema-load rejects any array without
                    // `items` (quill::array_missing_items), so a validated
                    // config never reaches here — pass the array through as-is.
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

    /// Recursively validate a field's structural shape, enforcing the
    /// one-level nesting contract in a single pass. The `position` records
    /// what shapes are legal at the current depth:
    ///
    /// - [`ShapePosition::Top`] — a field declared directly on a card: scalar,
    ///   `object` (typed dictionary), or `array` (primitive list or typed
    ///   table).
    /// - [`ShapePosition::ArrayItem`] — an array's `items`: a scalar or an
    ///   `object` (the typed-table row), but **not** another array.
    /// - [`ShapePosition::Leaf`] — an object's property (whether a top-level
    ///   typed dictionary or a typed-table row): scalar only. No deeper
    ///   containers, so `array<object<array>>` and `object<array>` are
    ///   rejected here.
    ///
    /// Returns the first violation as a ready-to-push [`Diagnostic`] whose
    /// message names `owner` (the field-name path, e.g. `rows[].tags`), or
    /// `None` when the shape is valid.
    fn validate_field_schema_shape(
        schema: &FieldSchema,
        owner: &str,
        position: ShapePosition,
    ) -> Option<Diagnostic> {
        let err = |code: &str, message: String| {
            Some(Diagnostic::new(Severity::Error, message).with_code(code.to_string()))
        };

        // `items` is only meaningful on arrays; `properties` only on objects.
        if schema.r#type != FieldType::Array && schema.items.is_some() {
            return err(
                "quill::items_not_supported",
                format!(
                    "Field '{owner}' declares 'items' but is not type: array. \
                     'items' (the element schema) is only valid on array fields."
                ),
            );
        }

        match schema.r#type {
            FieldType::Object => {
                // An object nested inside another object (a Leaf position) is
                // the classic "nested type: object" rejection.
                if position == ShapePosition::Leaf {
                    return err(
                        "quill::nested_object_not_supported",
                        format!(
                            "Field '{owner}' uses a nested type: object, which is not supported. \
                             An object's properties may only be scalars."
                        ),
                    );
                }
                let Some(props) = &schema.properties else {
                    return err(
                        "quill::object_missing_properties",
                        format!(
                            "Field '{owner}' has type: object but no properties defined. \
                             Declare a properties map, or use type: array with \
                             items: {{ type: object, properties: … }} for a list of objects."
                        ),
                    );
                };
                if props.is_empty() {
                    return err(
                        "quill::object_empty_properties",
                        format!(
                            "Field '{owner}' has type: object with an empty properties map. \
                             Declare at least one property, or remove the field entirely."
                        ),
                    );
                }
                // Object properties are leaves — scalars only.
                props.iter().find_map(|(name, prop)| {
                    Self::validate_field_schema_shape(
                        prop,
                        &format!("{owner}.{name}"),
                        ShapePosition::Leaf,
                    )
                })
            }
            FieldType::Array => {
                // An array may sit at the top level only; an array element may
                // not itself be an array, and neither may an object property.
                if position != ShapePosition::Top {
                    return err(
                        "quill::nested_array_not_supported",
                        format!(
                            "Field '{owner}' declares a nested array, which is not supported. \
                             Array elements must be scalars or objects, and object properties \
                             may only be scalars."
                        ),
                    );
                }
                if schema.properties.is_some() {
                    return err(
                        "quill::array_properties_not_supported",
                        format!(
                            "Field '{owner}' is type: array with a bare 'properties' map. \
                             Declare the element type under 'items' instead — for a list \
                             of objects use items: {{ type: object, properties: … }}."
                        ),
                    );
                }
                let Some(items) = &schema.items else {
                    return err(
                        "quill::array_missing_items",
                        format!(
                            "Field '{owner}' has type: array but no 'items' element schema. \
                             Declare the element type, e.g. items: {{ type: string }} \
                             for a list of strings or items: {{ type: object, \
                             properties: … }} for a list of objects."
                        ),
                    );
                };
                Self::validate_field_schema_shape(
                    items,
                    &format!("{owner}[]"),
                    ShapePosition::ArrayItem,
                )
            }
            // Scalars are leaves; nothing further to validate.
            _ => None,
        }
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

    /// Recursively validate field-level blueprint constraints across the field,
    /// any nested object properties, and an array's element schema (`items`).
    fn validate_field_blueprint_constraints(
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        Self::validate_description_singleline(schema.description.as_deref(), owner_label, errors);
        Self::validate_enum_literals(schema, owner_label, errors);
        Self::validate_example_default_types(schema, owner_label, errors);
        if let Some(props) = &schema.properties {
            for (name, prop) in props {
                let nested = format!("{}.{}", owner_label, name);
                Self::validate_field_blueprint_constraints(prop, &nested, errors);
            }
        }
        if let Some(items) = &schema.items {
            let nested = format!("{}[]", owner_label);
            Self::validate_field_blueprint_constraints(items, &nested, errors);
        }
    }

    /// Classify a YAML/JSON value into the "shape" label used in error messages.
    ///
    /// Distinguishes integers from floats so that the loaded value `20.04`
    /// (which arrives as a JSON float) is reported as `float`, not `number`.
    fn value_shape(value: &serde_json::Value) -> &'static str {
        match value {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    "integer"
                } else {
                    "float"
                }
            }
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        }
    }

    /// Returns true when `value` is type-compatible with the declared
    /// [`FieldType`]. Mirrors JSON Schema semantics modulo a couple of project
    /// conventions: dates/datetimes/markdown are author-time strings (we do
    /// not have native YAML date/datetime types).
    fn example_compatible(field_type: &FieldType, value: &serde_json::Value) -> bool {
        // `null` is treated as "absent" — callers should not reach here with
        // null, but if they do, accept it as compatible with any type.
        if value.is_null() {
            return true;
        }
        match field_type {
            FieldType::String
            | FieldType::Markdown
            | FieldType::Date
            | FieldType::DateTime => value.is_string(),
            FieldType::Integer => value.is_i64() || value.is_u64(),
            FieldType::Number => value.is_number(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Array => value.is_array(),
            FieldType::Object => value.is_object(),
        }
    }

    /// Render a short, quoted preview of the value for use inside an error
    /// message. Long strings/sequences/maps are truncated.
    fn value_preview(value: &serde_json::Value) -> String {
        let raw = match value {
            serde_json::Value::String(s) => format!("\"{}\"", s),
            other => other.to_string(),
        };
        const MAX: usize = 60;
        if raw.chars().count() > MAX {
            let truncated: String = raw.chars().take(MAX).collect();
            format!("{}…", truncated)
        } else {
            raw
        }
    }

    /// Reject `example:` / `default:` values whose YAML shape does not match
    /// the declared `type:`. Catches the common "unquoted version string"
    /// mistake (`example: 20.04` for `type: string`) at schema-load time,
    /// before it reaches the LLM as a confusing copy-the-example failure.
    ///
    /// Also enforces that an `example:` on an enum-constrained field is one of
    /// the declared enum values — a strictly tighter check than the type alone.
    fn validate_example_default_types(
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        let check = |slot: &str, value_opt: Option<&QuillValue>, errors: &mut Vec<Diagnostic>| {
            let Some(qv) = value_opt else {
                return;
            };
            let raw = qv.as_json();
            if raw.is_null() {
                return;
            }
            if !Self::example_compatible(&schema.r#type, raw) {
                let actual = Self::value_shape(raw);
                let declared = schema.r#type.as_str();
                let preview = Self::value_preview(raw);
                let hint = if actual == "float" || actual == "integer" {
                    format!(
                        "Quote the {slot} as \"{val}\" if the value is intentionally a string, \
                         or change the field type to '{actual}'.",
                        slot = slot,
                        val = raw.to_string().trim_matches('"'),
                        actual = if actual == "integer" { "integer" } else { "number" },
                    )
                } else if actual == "string" {
                    format!(
                        "Remove the quotes around the {slot} value to keep it a {declared}.",
                        slot = slot,
                        declared = declared,
                    )
                } else {
                    format!(
                        "Make the {slot} value a {declared}, or change the field type to match.",
                        slot = slot,
                        declared = declared,
                    )
                };
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{owner} declares type '{declared}' but {slot} is {actual} ({preview}).",
                            owner = owner_label,
                            declared = declared,
                            slot = slot,
                            actual = actual,
                            preview = preview,
                        ),
                    )
                    .with_code(format!("quill::{slot}_type_mismatch"))
                    .with_hint(hint),
                );
            }
        };

        check("example", schema.example.as_ref(), errors);
        check("default", schema.default.as_ref(), errors);

        // Enum membership check: when `enum:` is declared, the example/default
        // must be one of the declared values (a tighter check than type alone).
        if let Some(enum_vals) = &schema.enum_values {
            let mut check_enum = |slot: &str, value_opt: Option<&QuillValue>| {
                let Some(qv) = value_opt else {
                    return;
                };
                let raw = qv.as_json();
                if raw.is_null() {
                    return;
                }
                // Only meaningful when the value parsed as a string (which is
                // the only shape an enum value can take). If it parsed as some
                // other shape, the type-compat check above already flagged it.
                let Some(s) = raw.as_str() else {
                    return;
                };
                if !enum_vals.iter().any(|v| v == s) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{owner} {slot} \"{value}\" is not one of the declared enum values [{values}].",
                                owner = owner_label,
                                slot = slot,
                                value = s,
                                values = enum_vals
                                    .iter()
                                    .map(|v| format!("\"{}\"", v))
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ),
                        )
                        .with_code(format!("quill::{slot}_not_in_enum"))
                        .with_hint(format!(
                            "Set the {slot} to one of: {}.",
                            enum_vals
                                .iter()
                                .map(|v| format!("\"{}\"", v))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                }
            };
            check_enum("example", schema.example.as_ref());
            check_enum("default", schema.default.as_ref());
        }
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
                    // One recursive pass enforces the whole shape contract:
                    // containers carry the right child schema (`object` →
                    // `properties`, `array` → `items`), and nesting stops after
                    // one structural level (a typed table is the deepest shape).
                    if let Some(diag) = Self::validate_field_schema_shape(
                        &schema,
                        field_name,
                        ShapePosition::Top,
                    ) {
                        errors.push(diag);
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

        // Parse YAML into serde_json::Value via serde_saphyr. The depth budget
        // bounds nesting so an untrusted Quill.yaml cannot overflow the stack.
        // Note: serde_json with "preserve_order" feature is required for this to work as expected
        let quill_yaml_val: serde_json::Value = match serde_saphyr::from_str_with_options(
            yaml_content,
            crate::document::limits::yaml_parse_options(),
        ) {
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

        // Reject unknown top-level sections. Known sections are: quill, main, card_kinds,
        // and the backend name (e.g. typst). Everything else is a mistake. `fields` gets
        // a targeted hint since it's the most common shape mistake.
        if let Some(top_obj) = quill_yaml_val.as_object() {
            for key in top_obj.keys() {
                let is_known = key == "quill"
                    || key == "main"
                    || key == "card_kinds"
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
                        "Valid top-level sections are: quill, main, card_kinds{}",
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
        // other card kind). This is independent of `quill.description`.
        let main_description = main_obj_opt
            .and_then(|main_obj| main_obj.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Self::validate_description_singleline(main_description.as_deref(), "main", &mut errors);

        // The main entry-point card.
        let main = CardSchema {
            name: "main".to_string(),
            description: main_description,
            fields,
            ui: main_ui.or(ui_section),
            body: main_body,
        };

        // Extract [card_kinds] section (optional)
        let mut card_kinds: Vec<CardSchema> = Vec::new();
        if let Some(card_kinds_val) = quill_yaml_val.get("card_kinds") {
            match card_kinds_val.as_object() {
                None => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "'card_kinds' section must be an object (mapping of kind names to schemas)".to_string(),
                        )
                        .with_code("quill::invalid_card_kinds".to_string()),
                    );
                }
                Some(card_kinds_table) => {
                    for (card_name, card_value) in card_kinds_table {
                        if !Self::is_valid_card_identifier(card_name) {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Invalid card-kind name '{}': names must match \
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
                                                "Failed to parse card_kind '{}': {}",
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
                                &format!("card_kind '{}' field", card_name),
                                &mut errors,
                            )
                        } else {
                            BTreeMap::new()
                        };

                        Self::validate_description_singleline(
                            card_def.description.as_deref(),
                            &format!("card_kind '{}'", card_name),
                            &mut errors,
                        );
                        card_kinds.push(CardSchema {
                            name: card_name.clone(),
                            description: card_def.description,
                            fields: card_fields,
                            ui: card_def.ui,
                            body: card_def.body,
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
        for card in &card_kinds {
            if let Some(d) = warn_example_unused(&format!("card_kinds.{}", card.name), &card.body) {
                warnings.push(d);
            }
        }

        // Error when `body.example` contains a line that the document parser
        // would interpret as a `~~~` card-yaml block opener. Such a line would
        // start a new metadata block, corrupting document structure.
        let err_example_contains_fence = |label: &str,
                                          body: &Option<BodyCardSchema>|
         -> Option<Diagnostic> {
            let example = body.as_ref()?.example.as_deref()?;
            if example_contains_fence_line(example) {
                Some(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{label}.body.example` contains a line that would be parsed as a `~~~` card-yaml block opener; this would corrupt the blueprint"
                        ),
                    )
                    .with_code("quill::body_example_contains_fence".to_string())
                    .with_hint(
                        "Remove or reword any column-zero line that opens a card-yaml block (`~~~`, a longer tilde run, or `~~~card-yaml`). For a literal fenced code block, use a backtick fence (```).".to_string(),
                    ),
                )
            } else {
                None
            }
        };
        if let Some(d) = err_example_contains_fence("main", &main.body) {
            errors.push(d);
        }
        for card in &card_kinds {
            if let Some(d) =
                err_example_contains_fence(&format!("card_kinds.{}", card.name), &card.body)
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
                card_kinds,
                backend,
                version,
                author,
                plate_file,
                backend_config,
            },
            warnings,
        ))
    }
}

/// Returns true if any line in `text` would be parsed as a card-yaml block
/// opener by the document parser, which would corrupt the blueprint's document
/// structure when the example is embedded verbatim as body content.
///
/// Delegates to the parser's own opener predicate
/// ([`crate::document::fences::is_card_yaml_opener_line`]) so the guard stays
/// in lock-step with fence detection: a column-zero tilde fence (three or more
/// tildes) whose info string is empty or `card-yaml`. Backtick fences,
/// language-tagged `~~~` fences, and indented fences are ordinary code blocks
/// and are not flagged.
fn example_contains_fence_line(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        crate::document::fences::is_card_yaml_opener_line(line)
    })
}
