//! Quill schema and core type definitions.
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::value::QuillValue;

/// UI-specific metadata for field rendering
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFieldSchema {
    /// Display label for the field — decoupled from the snake_case wire key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Automatically generated based on field position in Quill.yaml.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    /// Valid on `string` fields (plain text with newlines preserved) and `markdown` fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
}

/// Body namespace configuration for a card kind
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyCardSchema {
    /// When false, consumers must not accept or store body content for instances of this card kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Embedded verbatim in the blueprint body region; falls back to `Write <card> body here.` when absent.
    /// Has no effect when `enabled` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCardSchema {
    /// Display label for the card kind — literal string or `{field_name}`
    /// template. See `docs/quills/quill-yaml-reference.md`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Schema definition for a card kind (composable content blocks)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: BTreeMap<String, FieldSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiCardSchema>,
    /// Controls whether a body editor is shown and provides optional guide text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyCardSchema>,
}

impl CardSchema {
    /// Default values declared on this card's fields, keyed by field name. Fields with no `default` are omitted.
    pub fn defaults(&self) -> HashMap<String, QuillValue> {
        self.fields
            .iter()
            .filter_map(|(name, field)| field.default.as_ref().map(|v| (name.clone(), v.clone())))
            .collect()
    }

    /// Returns true if body content is permitted for instances of this card.
    /// Defaults to true when no `body` namespace is declared.
    pub fn body_enabled(&self) -> bool {
        self.body.as_ref().and_then(|b| b.enabled).unwrap_or(true)
    }
}

/// Field type hint enum for type-safe field type definitions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    /// Integers and decimals
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    /// Formatted as string; validates against the YAML 1.1 timestamp grammar
    /// (bare `YYYY-MM-DD` through full RFC 3339 with offset).
    DateTime,
    /// String with markdown content, `contentMediaType: text/markdown`
    Markdown,
}

impl FieldType {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "string" => Some(FieldType::String),
            "number" => Some(FieldType::Number),
            "integer" => Some(FieldType::Integer),
            "boolean" => Some(FieldType::Boolean),
            "array" => Some(FieldType::Array),
            "object" => Some(FieldType::Object),
            "datetime" => Some(FieldType::DateTime),
            "markdown" => Some(FieldType::Markdown),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Number => "number",
            FieldType::Integer => "integer",
            FieldType::Boolean => "boolean",
            FieldType::Array => "array",
            FieldType::Object => "object",
            FieldType::DateTime => "datetime",
            FieldType::Markdown => "markdown",
        }
    }
}

/// Schema definition for a template field.
///
/// `default` and `example` are both type-valid values with opposite intent:
/// `default` is the value most authors want (interpolated when the field is
/// omitted), while `example` matches the desired type and shape but is not
/// the value most authors want (it documents shape only, never rendering).
///
/// A field's *cell* is determined by `default`: a field with a `default:`
/// is **Endorsed** (the rendered value is shippable as-is), while a field
/// without a `default:` is **Unendorsed** (the author endorsed no value, so
/// the blueprint carries a `!must_fill` placeholder to ask for one). Absence is
/// not a requirement: a missing or null Unendorsed field zero-fills at
/// render. A surviving `!must_fill` placeholder is surfaced as the non-fatal
/// `validation::must_fill` warning. There is no separate `required:` axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    pub r#type: FieldType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The value most authors want; interpolated when the field is omitted.
    /// Its presence makes the field Endorsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<QuillValue>,
    /// A value matching the desired type and shape but not the value most
    /// authors want; documents shape only and never renders as the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<QuillValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiFieldSchema>,
    /// Restricts valid values on string fields.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Nested field schemas for `object` types (the typed dictionary's
    /// properties).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, Box<FieldSchema>>>,
    /// Element schema for `array` types. Required on every `array` field:
    /// the element type gives the array a concrete element type (`string[]`,
    /// `integer[]`, `markdown[]`, …). For a typed table the element is an
    /// `object` carrying its own `properties`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<FieldSchema>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchemaDef {
    pub r#type: FieldType,
    pub description: Option<String>,
    pub default: Option<QuillValue>,
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    // Nested schema support
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    // Element schema for arrays.
    pub items: Option<serde_json::Value>,
}

impl FieldSchema {
    /// Sort key for blueprint/form/seed field ordering: `ui.order` when set,
    /// else `i32::MAX` (declaration order is assigned to `ui.order` at parse
    /// time, so the fallback is defensive). The one shared ordering producer.
    pub fn ui_order(&self) -> i32 {
        self.ui.as_ref().and_then(|u| u.order).unwrap_or(i32::MAX)
    }

    pub fn new(name: String, r#type: FieldType, description: Option<String>) -> Self {
        Self {
            name,
            r#type,
            description,
            default: None,
            example: None,
            ui: None,
            enum_values: None,
            properties: None,
            items: None,
        }
    }

    pub fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        Ok(Self {
            name: key.clone(),
            r#type: def.r#type,
            description: def.description,
            default: def.default,
            example: def.example,
            ui: def.ui,
            enum_values: def.enum_values,
            properties: if let Some(props) = def.properties {
                let mut p = BTreeMap::new();
                for (key, value) in props {
                    p.insert(
                        key.clone(),
                        Box::new(FieldSchema::from_quill_value(
                            key,
                            &QuillValue::from_json(value),
                        )?),
                    );
                }
                Some(p)
            } else {
                None
            },
            items: if let Some(items) = def.items {
                Some(Box::new(FieldSchema::from_quill_value(
                    format!("{key}[]"),
                    &QuillValue::from_json(items),
                )?))
            } else {
                None
            },
        })
    }
}
