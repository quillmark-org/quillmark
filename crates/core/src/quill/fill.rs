//! The per-field **zero value** — the type-minimal valid value for a field.
//!
//! This is the single source of truth for "the zero value for this field,"
//! shared by two callers (see `prose/canon/SCHEMAS.md` and `BLUEPRINT.md`):
//!
//! - **blueprint/example emission** ([`super::blueprint`]) — the `example`
//!   document's fallback, when a field carries neither an `example:` nor a
//!   `default:`.
//! - **zero-filled render** ([`QuillConfig::compile_data`](super::QuillConfig::compile_data),
//!   invoked from `quillmark::orchestration`) — each absent field is filled
//!   with its zero value in the plate-JSON projection only, never in the
//!   persisted document.

use serde_json::json;

use super::{FieldSchema, FieldType};
use crate::value::QuillValue;

/// The type-empty (zero) value for `field`: the leanest value satisfying its
/// declared type.
///
/// Blank for most types — `""` (string/datetime), `0`, `false`, `[]`, and the
/// empty content for richtext. `enum` has no empty member, so it zeroes to the
/// first declared variant. An `object` with `properties` is shape-valid only
/// when every property is present, so it zeroes (recursively) to an object with
/// every property at its own zero value, not a bare `{}` (which only a
/// property-less object degrades to).
pub fn zero_value(field: &FieldSchema) -> QuillValue {
    if let Some(values) = &field.enum_values {
        let first = values.first().cloned().unwrap_or_default();
        return QuillValue::from_json(json!(first));
    }
    let json = match field.r#type {
        FieldType::Array => json!([]),
        FieldType::Object => match &field.properties {
            // Recurse so each property is zero-filled to its own type-empty
            // leaf — the result is a shape-valid object, not a bare `{}`.
            Some(properties) => serde_json::Value::Object(
                properties
                    .iter()
                    .map(|(name, schema)| (name.clone(), zero_value(schema).into_json()))
                    .collect(),
            ),
            // A property-less object is schema-invalid; `{}` is its only
            // type-correct zero.
            None => json!({}),
        },
        FieldType::Integer | FieldType::Number => json!(0),
        FieldType::Boolean => json!(false),
        // A content field's zero is the empty content, not `""` — the seam carries
        // canonical Content-JSON, so the render floor must zero-fill an absent
        // richtext or plaintext field with a content the backend can lower. The
        // empty content is single-`Para`, so it satisfies `inline` and is `plain`.
        FieldType::RichText { .. } | FieldType::PlainText { .. } => {
            quillmark_content::serial::to_canonical_value(&quillmark_content::Content::empty())
        }
        // String / DateTime: `""` is schema-valid for both.
        _ => json!(""),
    };
    QuillValue::from_json(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(yaml: &str) -> FieldSchema {
        let value = QuillValue::from_yaml_str(yaml).unwrap();
        FieldSchema::from_quill_value("field".to_string(), &value).unwrap()
    }

    #[test]
    fn object_with_properties_zero_fills_each_scalar_leaf() {
        let schema = field(
            r#"
type: object
properties:
  street: { type: string }
  zip: { type: integer }
  active: { type: boolean }
"#,
        );

        assert_eq!(
            zero_value(&schema).into_json(),
            json!({ "street": "", "zip": 0, "active": false })
        );
    }

    #[test]
    fn nested_object_recurses_to_type_empty_leaves() {
        let schema = field(
            r#"
type: object
properties:
  name: { type: string }
  address:
    type: object
    properties:
      city: { type: string }
      tags: { type: array, items: { type: string } }
"#,
        );

        assert_eq!(
            zero_value(&schema).into_json(),
            json!({
                "name": "",
                "address": { "city": "", "tags": [] }
            })
        );
    }

    #[test]
    fn property_less_object_degrades_to_empty_object() {
        // A property-less object is schema-invalid in practice; `{}` is the
        // only type-correct zero it can carry.
        let schema = field("type: object\n");
        assert_eq!(zero_value(&schema).into_json(), json!({}));
    }
}
