//! The per-field **zero value** — the type-minimal valid value for a field.
//!
//! This is the single source of truth for "the zero value for this field,"
//! shared by two callers (see `prose/canon/SCHEMAS.md` and `BLUEPRINT.md`):
//!
//! - **blueprint/example emission** ([`super::blueprint`]) — the `example`
//!   document's fallback, when a field carries neither an `example:` nor a
//!   `default:`.
//! - **zero-filled render** (the render path in `quillmark::orchestration`) —
//!   each absent field is filled with its zero value in the plate-JSON
//!   projection only, never in the persisted document.

use serde_json::json;

use super::{FieldSchema, FieldType};
use crate::value::QuillValue;

/// The type-empty (zero) value for `field`: the leanest value that satisfies
/// the field's declared type.
///
/// Honestly blank for almost every type — `""` (string, markdown, datetime:
/// the validator accepts the empty string for datetime), `0`, `false`, `[]`.
/// The lone seam is `enum`: there is no empty enum
/// member, so the zero value is the first declared variant (`first_enum`).
///
/// An `object` with `properties` is *shape-valid only when every property is
/// present*, so its zero value is the object whose each property carries that
/// property's zero value (recursively). A bare `{}` would fail validation —
/// the validator reports every absent property as a `FieldAbsent`. Only a
/// property-less object (schema-invalid in practice) degrades to `{}`.
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
        // String / Markdown / DateTime: `""` is schema-valid for all three.
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
