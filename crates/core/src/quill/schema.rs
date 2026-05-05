//! Schema construction utilities for Quill bundles.
//!
//! This module contains `build_transform_schema`, which maps the abstract
//! [`FieldSchema`] / [`FieldType`] model to a JSON-Schema-shaped
//! [`QuillValue`]. The schema is backend-agnostic (no Typst specifics);
//! backends consume it to drive per-field transforms such as markdown →
//! backend-markup conversion.

use super::{FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

/// Build a JSON-Schema-shaped descriptor of a [`QuillConfig`]'s main + card fields.
///
/// The descriptor marks markdown fields with `contentMediaType: "text/markdown"`
/// and date/date-time fields with the corresponding JSON Schema `format`.
pub fn build_transform_schema(config: &QuillConfig) -> QuillValue {
    fn field_to_schema(field: &FieldSchema) -> serde_json::Value {
        let mut schema = serde_json::Map::new();
        match field.r#type {
            FieldType::String => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
            }
            FieldType::Markdown => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
                schema.insert(
                    "contentMediaType".to_string(),
                    serde_json::Value::String("text/markdown".to_string()),
                );
            }
            FieldType::Number => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("number".to_string()),
                );
            }
            FieldType::Integer => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("integer".to_string()),
                );
            }
            FieldType::Boolean => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("boolean".to_string()),
                );
            }
            FieldType::Array => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("array".to_string()),
                );
                if let Some(items) = &field.items {
                    schema.insert("items".to_string(), field_to_schema(items));
                }
            }
            FieldType::Object => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("object".to_string()),
                );
                if let Some(properties) = &field.properties {
                    let mut props = serde_json::Map::new();
                    for (name, prop) in properties {
                        props.insert(name.clone(), field_to_schema(prop));
                    }
                    schema.insert("properties".to_string(), serde_json::Value::Object(props));
                }
            }
            FieldType::Date => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
                schema.insert(
                    "format".to_string(),
                    serde_json::Value::String("date".to_string()),
                );
            }
            FieldType::DateTime => {
                schema.insert(
                    "type".to_string(),
                    serde_json::Value::String("string".to_string()),
                );
                schema.insert(
                    "format".to_string(),
                    serde_json::Value::String("date-time".to_string()),
                );
            }
        }
        serde_json::Value::Object(schema)
    }

    let mut properties = serde_json::Map::new();
    for (name, field) in &config.main.fields {
        properties.insert(name.clone(), field_to_schema(field));
    }
    properties.insert(
        "BODY".to_string(),
        serde_json::json!({ "type": "string", "contentMediaType": "text/markdown" }),
    );

    let mut defs = serde_json::Map::new();
    for card in &config.card_types {
        let mut card_properties = serde_json::Map::new();
        for (name, field) in &card.fields {
            card_properties.insert(name.clone(), field_to_schema(field));
        }
        card_properties.insert(
            "BODY".to_string(),
            serde_json::json!({ "type": "string", "contentMediaType": "text/markdown" }),
        );
        defs.insert(
            format!("{}_card", card.name),
            serde_json::json!({
                "type": "object",
                "properties": card_properties,
            }),
        );
    }

    QuillValue::from_json(serde_json::json!({
        "type": "object",
        "properties": properties,
        "$defs": defs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_from_yaml(yaml: &str) -> QuillValue {
        let config = QuillConfig::from_yaml(yaml).expect("yaml parses");
        build_transform_schema(&config)
    }

    #[test]
    fn injects_body_as_markdown_for_main_and_each_card_type() {
        let yaml = r#"
quill:
  name: example
  version: 0.1.0
  backend: typst
  description: example

main:
  fields:
    title:
      type: string

card_types:
  indorsement:
    fields:
      signature_block:
        type: string
  note:
    fields:
      author:
        type: string
"#;

        let schema = build_from_yaml(yaml);
        let json = schema.as_json();

        let main_body = &json["properties"]["BODY"];
        assert_eq!(main_body["type"], "string");
        assert_eq!(main_body["contentMediaType"], "text/markdown");

        for def_name in ["indorsement_card", "note_card"] {
            let card_body = &json["$defs"][def_name]["properties"]["BODY"];
            assert_eq!(
                card_body["type"], "string",
                "{def_name} BODY type should be string"
            );
            assert_eq!(
                card_body["contentMediaType"], "text/markdown",
                "{def_name} BODY should be markdown"
            );
        }
    }
}
