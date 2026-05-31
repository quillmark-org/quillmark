//! The per-field **zero value** — the type-minimal valid value for a field.
//!
//! This is the single source of truth for "the zero value for this field,"
//! shared by two callers (see `prose/proposals/`):
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
/// Honestly blank for almost every type — `""` (string, markdown, date,
/// datetime: the validator accepts the empty string for date/datetime), `0`,
/// `false`, `[]`, `{}`. The lone seam is `enum`: there is no empty enum
/// member, so the zero value is the first declared variant (`first_enum`).
pub fn zero_value(field: &FieldSchema) -> QuillValue {
    if let Some(values) = &field.enum_values {
        let first = values.first().cloned().unwrap_or_default();
        return QuillValue::from_json(json!(first));
    }
    let json = match field.r#type {
        FieldType::Array => json!([]),
        // A bare object reaching this point would be schema-invalid (objects
        // require a `properties` map). `{}` is the type-correct zero regardless.
        FieldType::Object => json!({}),
        FieldType::Integer | FieldType::Number => json!(0),
        FieldType::Boolean => json!(false),
        // String / Markdown / Date / DateTime: `""` is schema-valid for all four.
        _ => json!(""),
    };
    QuillValue::from_json(json)
}
