//! Quill schema and core type definitions.
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::value::QuillValue;

fn is_false(value: &bool) -> bool {
    !*value
}
/// Semantic constants for field schema keys used in parsing and JSON Schema generation.
/// Using constants provides IDE support (find references, autocomplete) and ensures
/// consistency between parsing and output.
pub mod field_key {
    /// Field type (string, number, boolean, array, etc.)
    pub const TYPE: &str = "type";
    /// Detailed field description
    pub const DESCRIPTION: &str = "description";
    /// Default value for the field
    pub const DEFAULT: &str = "default";
    /// Example value for the field (single, used as template placeholder)
    pub const EXAMPLE: &str = "example";
    /// UI-specific metadata
    pub const UI: &str = "ui";
    /// Whether the field is required
    pub const REQUIRED: &str = "required";
    /// Enum values for string fields
    pub const ENUM: &str = "enum";
    /// Date format specifier (JSON Schema)
    pub const FORMAT: &str = "format";
}

/// Semantic constants for UI schema keys
pub mod ui_key {
    /// Group name for field organization
    pub const GROUP: &str = "group";
    /// Display order within the UI
    pub const ORDER: &str = "order";
    /// Display label for a card kind. May be a literal string or a template
    /// containing `{field_name}` tokens interpolated per-instance by UI consumers.
    pub const TITLE: &str = "title";
    /// Compact rendering hint for UI consumers
    pub const COMPACT: &str = "compact";
    /// Multi-line text box hint for string and markdown fields
    pub const MULTILINE: &str = "multiline";
}

/// UI-specific metadata for field rendering
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFieldSchema {
    /// Display label for the field — decoupled from the snake_case wire key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Group name for organizing fields (e.g., "Personal Info", "Preferences")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Order of the field in the UI (automatically generated based on field position in Quill.yaml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    /// Compact rendering hint: when true, the UI should render this field in a compact style
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    /// Multi-line text box hint: when true, the UI should start with a larger text box.
    /// Valid on `string` fields (plain text with newlines preserved) and `markdown` fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
}

/// Body namespace configuration for a card kind
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyCardSchema {
    /// Whether the body editor is enabled for this card (default: true).
    /// When false, consumers must not accept or store body content for instances
    /// of this card kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Example body content embedded verbatim in the blueprint body region.
    /// When absent, the blueprint falls back to `Write <card> body here.`.
    /// Has no effect when `enabled` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCardSchema {
    /// Display label for the card kind — literal string or `{field_name}`
    /// template. See `docs/format-designer/quill-yaml-reference.md`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Schema definition for a card kind (composable content blocks)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSchema {
    /// Card kind name (e.g., "indorsements"). The map key carries this on the
    /// wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    /// Detailed description of this card kind
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// List of fields in the card
    pub fields: BTreeMap<String, FieldSchema>,
    /// UI layout hints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiCardSchema>,
    /// Body namespace: controls whether a body editor is shown and provides
    /// optional guide text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyCardSchema>,
}

impl CardSchema {
    /// Default values declared on this card's fields, keyed by field name.
    /// Fields with no `default` are omitted.
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
    /// String type
    String,
    /// Numeric type (integers and decimals)
    Number,
    /// Integer type
    Integer,
    /// Boolean type
    Boolean,
    /// Array type
    Array,
    /// Object type
    Object,
    /// Date type (formatted as string with date format)
    Date,
    /// DateTime type (formatted as string with date-time format)
    DateTime,
    /// Markdown type (string with markdown content, contentMediaType: text/markdown)
    Markdown,
}

impl FieldType {
    /// Parse a FieldType from a string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "string" => Some(FieldType::String),
            "number" => Some(FieldType::Number),
            "integer" => Some(FieldType::Integer),
            "boolean" => Some(FieldType::Boolean),
            "array" => Some(FieldType::Array),
            "object" => Some(FieldType::Object),
            "date" => Some(FieldType::Date),
            "datetime" => Some(FieldType::DateTime),
            "markdown" => Some(FieldType::Markdown),
            _ => None,
        }
    }

    /// Get the canonical string representation for this type
    pub fn as_str(&self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Number => "number",
            FieldType::Integer => "integer",
            FieldType::Boolean => "boolean",
            FieldType::Array => "array",
            FieldType::Object => "object",
            FieldType::Date => "date",
            FieldType::DateTime => "datetime",
            FieldType::Markdown => "markdown",
        }
    }
}

/// Schema definition for a template field
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    /// Field name. The map key carries this on the wire; skipped during
    /// serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    /// Field type (required)
    pub r#type: FieldType,
    /// Detailed description of the field (used in JSON Schema description)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default value for the field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<QuillValue>,
    /// Example value for the field (single; used as template placeholder)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<QuillValue>,
    /// UI layout hints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiFieldSchema>,
    /// Whether this field is required (fields are optional by default)
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Enum values for string fields (restricts valid values)
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Properties for dict/object types (nested field schemas)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, Box<FieldSchema>>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchemaDef {
    pub r#type: FieldType,
    pub description: Option<String>,
    pub default: Option<QuillValue>,
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    // Nested schema support
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

impl FieldSchema {
    /// Create a new FieldSchema with default values
    pub fn new(name: String, r#type: FieldType, description: Option<String>) -> Self {
        Self {
            name,
            r#type,
            description,
            default: None,
            example: None,
            ui: None,
            required: false,
            enum_values: None,
            properties: None,
        }
    }

    /// Parse a FieldSchema from a QuillValue
    pub fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        Ok(Self {
            name: key,
            r#type: def.r#type,
            description: def.description,
            default: def.default,
            example: def.example,
            ui: def.ui,
            required: def.required,
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
        })
    }
}
