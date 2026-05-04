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
    /// Short label for the field
    pub const TITLE: &str = "title";
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
    /// Whether the field or specific component is hide-body (no body editor)
    pub const HIDE_BODY: &str = "hide_body";
    /// Default title template for card instances
    pub const DEFAULT_TITLE: &str = "default_title";
    /// Compact rendering hint for UI consumers
    pub const COMPACT: &str = "compact";
    /// Multi-line text box hint for string and markdown fields
    pub const MULTILINE: &str = "multiline";
}

/// UI-specific metadata for field rendering
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFieldSchema {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiContainerSchema {
    /// Whether to hide the body editor for this element (metadata only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_body: Option<bool>,
    /// Template for generating a default per-instance title in UI consumers.
    /// Uses `{field_name}` tokens interpolated with live field values.
    /// Example: `"{name}"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_title: Option<String>,
}

/// Schema definition for a card type (composable content blocks)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSchema {
    /// Card type name (e.g., "indorsements"). The map key carries this on the
    /// wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    /// Short label for the card type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Detailed description of this card type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// List of fields in the card
    pub fields: BTreeMap<String, FieldSchema>,
    /// UI layout hints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiContainerSchema>,
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
    /// Short label for the field (used in JSON Schema title)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    /// Item schema for array types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<FieldSchema>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchemaDef {
    pub title: Option<String>,
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
    pub items: Option<serde_json::Value>,
}

impl FieldSchema {
    /// Create a new FieldSchema with default values
    pub fn new(name: String, r#type: FieldType, description: Option<String>) -> Self {
        Self {
            name,
            title: None,
            r#type,
            description,
            default: None,
            example: None,
            ui: None,
            required: false,
            enum_values: None,
            properties: None,
            items: None,
        }
    }

    /// Parse a FieldSchema from a QuillValue
    pub fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        Ok(Self {
            name: key,
            title: def.title,
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
            items: if let Some(item_def) = def.items {
                Some(Box::new(FieldSchema::from_quill_value(
                    "items".to_string(),
                    &QuillValue::from_json(item_def),
                )?))
            } else {
                None
            },
        })
    }
}
