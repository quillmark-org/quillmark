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
///
/// `$body` is injected into a kind's `properties` only when that kind's
/// `body.enabled` is not `false`. A body-disabled kind's `$body` is absent,
/// not present-and-empty: absence cascades through the `__meta__` address
/// tables so `form-field(field:)` rejects `$body` addresses on that
/// kind at compile time, matching `Quill::validate`'s hard error on authored
/// body content for the same kind.
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
                // The element schema is emitted recursively, so a scalar
                // element yields `items: {type: string}` (and markdown carries
                // its `contentMediaType`), while an object element yields
                // `items: {type: object, properties: …}`.
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
    if config.main.body_enabled() {
        properties.insert(
            "$body".to_string(),
            serde_json::json!({ "type": "string", "contentMediaType": "text/markdown" }),
        );
    }

    let mut defs = serde_json::Map::new();
    for card in &config.card_kinds {
        let mut card_properties = serde_json::Map::new();
        for (name, field) in &card.fields {
            card_properties.insert(name.clone(), field_to_schema(field));
        }
        if card.body_enabled() {
            card_properties.insert(
                "$body".to_string(),
                serde_json::json!({ "type": "string", "contentMediaType": "text/markdown" }),
            );
        }
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
    fn typed_table_emits_items_with_object_and_properties() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string }
          year: { type: integer }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let refs = &json["properties"]["refs"];
        assert_eq!(refs["type"], "array");
        assert_eq!(refs["items"]["type"], "object");
        assert_eq!(refs["items"]["properties"]["org"]["type"], "string");
        assert_eq!(refs["items"]["properties"]["year"]["type"], "integer");
    }

    #[test]
    fn scalar_array_emits_items_with_element_type() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    counts:
      type: array
      items: { type: integer }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let counts = &json["properties"]["counts"];
        assert_eq!(counts["type"], "array");
        assert_eq!(counts["items"]["type"], "integer");
    }

    #[test]
    fn markdown_array_emits_items_with_content_media_type() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    sections:
      type: array
      items: { type: markdown }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let sections = &json["properties"]["sections"];
        assert_eq!(sections["type"], "array");
        assert_eq!(sections["items"]["type"], "string");
        assert_eq!(sections["items"]["contentMediaType"], "text/markdown");
    }

    #[test]
    fn typed_dict_emits_object_with_properties() {
        let yaml = r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string }
        city: { type: string }
"#;
        let schema = build_from_yaml(yaml);
        let json = schema.as_json();
        let address = &json["properties"]["address"];
        assert_eq!(address["type"], "object");
        assert_eq!(address["properties"]["street"]["type"], "string");
        assert_eq!(address["properties"]["city"]["type"], "string");
    }

    #[test]
    fn injects_body_as_markdown_for_main_and_each_card_kind() {
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

card_kinds:
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

        let main_body = &json["properties"]["$body"];
        assert_eq!(main_body["type"], "string");
        assert_eq!(main_body["contentMediaType"], "text/markdown");

        for def_name in ["indorsement_card", "note_card"] {
            let card_body = &json["$defs"][def_name]["properties"]["$body"];
            assert_eq!(
                card_body["type"], "string",
                "{def_name} $body type should be string"
            );
            assert_eq!(
                card_body["contentMediaType"], "text/markdown",
                "{def_name} $body should be markdown"
            );
        }
    }

    #[test]
    fn body_disabled_kind_omits_body_from_schema() {
        let yaml = r#"
quill:
  name: example
  version: 0.1.0
  backend: typst
  description: example

main:
  body:
    enabled: false
  fields:
    title:
      type: string

card_kinds:
  indorsement:
    body:
      enabled: false
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

        assert!(
            json["properties"].get("$body").is_none(),
            "body-disabled main should not carry $body"
        );
        assert!(
            json["$defs"]["indorsement_card"]["properties"]
                .get("$body")
                .is_none(),
            "body-disabled card kind should not carry $body"
        );
        assert!(
            json["$defs"]["note_card"]["properties"]
                .get("$body")
                .is_some(),
            "body-enabled card kind should still carry $body"
        );
    }
}
