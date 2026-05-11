//! Value type for unified representation of TOML/YAML/JSON values.
//!
//! This module provides [`QuillValue`], a newtype wrapper around `serde_json::Value`
//! that centralizes all value conversions across the Quillmark system.

use serde::{Deserialize, Serialize};
use std::ops::Deref;

/// Unified value type backed by `serde_json::Value`.
///
/// This type is used throughout Quillmark to represent metadata, fields, and other
/// dynamic values. It provides conversion methods for TOML and YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuillValue(serde_json::Value);

impl QuillValue {
    // from_yaml removed as we use serde_json::Value directly

    /// Create a QuillValue from a YAML string
    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, serde_saphyr::Error> {
        let json_val: serde_json::Value = serde_saphyr::from_str(yaml_str)?;
        Ok(QuillValue(json_val))
    }

    /// Get a reference to the underlying JSON value
    pub fn as_json(&self) -> &serde_json::Value {
        &self.0
    }

    /// Convert into the underlying JSON value
    pub fn into_json(self) -> serde_json::Value {
        self.0
    }

    /// Create a QuillValue directly from a JSON value
    pub fn from_json(json_val: serde_json::Value) -> Self {
        QuillValue(json_val)
    }

    /// String value.
    pub fn string(s: impl Into<String>) -> Self {
        QuillValue(serde_json::Value::String(s.into()))
    }

    /// Integer value.
    pub fn integer(n: i64) -> Self {
        QuillValue(serde_json::Value::Number(n.into()))
    }

    /// Boolean value.
    pub fn bool(b: bool) -> Self {
        QuillValue(serde_json::Value::Bool(b))
    }

    /// Null value.
    pub fn null() -> Self {
        QuillValue(serde_json::Value::Null)
    }
}

impl Deref for QuillValue {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Implement common delegating methods for convenience
impl QuillValue {
    /// Check if the value is null
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// Get the value as a string reference
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// Get the value as a boolean
    pub fn as_bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    /// Get the value as an i64
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    /// Get the value as a u64
    pub fn as_u64(&self) -> Option<u64> {
        self.0.as_u64()
    }

    /// Get the value as an f64
    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    /// Get the value as an array reference
    pub fn as_array(&self) -> Option<&Vec<serde_json::Value>> {
        self.0.as_array()
    }

    /// Get the value as an object reference
    pub fn as_object(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.0.as_object()
    }

    /// Get a field from an object by key
    pub fn get(&self, key: &str) -> Option<QuillValue> {
        self.0.get(key).map(|v| QuillValue(v.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_yaml_value() {
        let yaml_str = r#"
            package:
              name: test
              version: 1.0.0
        "#;
        let json_val: serde_json::Value = serde_saphyr::from_str(yaml_str).unwrap();
        let quill_val = QuillValue::from_json(json_val);

        assert!(quill_val.as_object().is_some());
        assert_eq!(
            quill_val
                .get("package")
                .unwrap()
                .get("name")
                .unwrap()
                .as_str(),
            Some("test")
        );
    }

    #[test]
    fn test_from_yaml_str() {
        let yaml_str = r#"
            title: Test Document
            author: John Doe
            count: 42
        "#;
        let quill_val = QuillValue::from_yaml_str(yaml_str).unwrap();

        assert_eq!(
            quill_val.get("title").as_ref().and_then(|v| v.as_str()),
            Some("Test Document")
        );
        assert_eq!(
            quill_val.get("author").as_ref().and_then(|v| v.as_str()),
            Some("John Doe")
        );
        assert_eq!(
            quill_val.get("count").as_ref().and_then(|v| v.as_i64()),
            Some(42)
        );
    }

    #[test]
    fn test_as_json() {
        let json_val = serde_json::json!({"key": "value"});
        let quill_val = QuillValue::from_json(json_val.clone());

        assert_eq!(quill_val.as_json(), &json_val);
    }

    #[test]
    fn test_into_json() {
        let json_val = serde_json::json!({"key": "value"});
        let quill_val = QuillValue::from_json(json_val.clone());

        assert_eq!(quill_val.into_json(), json_val);
    }

    #[test]
    fn test_delegating_methods() {
        let quill_val = QuillValue::from_json(serde_json::json!({
            "name": "test",
            "count": 42,
            "active": true,
            "items": [1, 2, 3]
        }));

        assert_eq!(
            quill_val.get("name").as_ref().and_then(|v| v.as_str()),
            Some("test")
        );
        assert_eq!(
            quill_val.get("count").as_ref().and_then(|v| v.as_i64()),
            Some(42)
        );
        assert_eq!(
            quill_val.get("active").as_ref().and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(quill_val
            .get("items")
            .as_ref()
            .and_then(|v| v.as_array())
            .is_some());
    }

    #[test]
    fn test_yaml_with_tags() {
        // Note: serde_saphyr handles tags differently - this tests basic parsing
        let yaml_str = r#"
            value: 42
        "#;
        let quill_val = QuillValue::from_yaml_str(yaml_str).unwrap();

        // Values should be converted to their underlying value
        assert!(quill_val.as_object().is_some());
    }

    #[test]
    fn test_null_value() {
        let quill_val = QuillValue::from_json(serde_json::Value::Null);
        assert!(quill_val.is_null());
    }

    #[test]
    fn test_yaml_custom_tags_ignored_at_value_level() {
        // At the raw `QuillValue::from_yaml_str` layer, custom YAML tags
        // (including `!fill`) pass through serde_saphyr which drops the
        // tag and returns the underlying scalar.  The tag is recovered at
        // the `Document` layer by `document::prescan`: see
        // `document::tests::lossiness_tests::custom_tags_lose_tag_but_keep_value`.
        let yaml_str = "memo_from: !fill 2d lt example";
        let quill_val = QuillValue::from_yaml_str(yaml_str).unwrap();

        assert_eq!(
            quill_val.get("memo_from").as_ref().and_then(|v| v.as_str()),
            Some("2d lt example")
        );
    }
}
