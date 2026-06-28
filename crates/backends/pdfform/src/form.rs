//! `form.json` wire types and parsing.
//!
//! `form.json` is the durable, value-free field-definition layer of a `pdfform`
//! quill — the *format* side of Quillmark's quill/document dichotomy. It is
//! complete enough to rebuild every widget yet flat and diffable. Unknown keys
//! are ignored (additive evolution needs no version bump); invalid type/payload
//! combinations are unrepresentable thanks to the internally-tagged `type`.

use serde::Deserialize;

/// The `schema` tag prefix every `form.json` must carry, following the Document
/// DTO convention (`quillmark/form@<version>`). V1 adopts only the field+value
/// format; the chained-migration machinery lands when a breaking change first
/// does.
pub const SCHEMA_PREFIX: &str = "quillmark/form@";

/// A parsed `form.json`: the schema tag plus the field reconstruction list.
#[derive(Debug, Clone, Deserialize)]
pub struct FormSpec {
    pub schema: String,
    pub fields: Vec<FormField>,
}

/// One field definition: identity + geometry + binding + kind.
///
/// `rect` is **top-left** `{x,y,w,h}` in PDF points, page-relative — the
/// loader flips it to the spine's bottom-left origin. `schema_field` is the
/// document field this binds to; `None` means unbound (a signer fills it).
#[derive(Debug, Clone, Deserialize)]
pub struct FormField {
    pub name: String,
    #[serde(default)]
    pub schema_field: Option<String>,
    pub page: usize,
    pub rect: Rect,
    #[serde(default)]
    pub tooltip: Option<String>,
    #[serde(flatten)]
    pub kind: FieldKind,
}

/// A top-left rectangle in PDF points (1/72").
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The kind of a [`FormField`] and its kind-specific definition. Internally
/// tagged by `type` and flattened into the field, so the JSON stays flat while
/// invalid combinations (a `signature` with `options`) are unrepresentable.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldKind {
    Text {
        #[serde(default)]
        multiline: bool,
    },
    Checkbox,
    Choice {
        options: Vec<String>,
    },
    Signature,
}

/// Why a `form.json` failed to parse.
#[derive(Debug)]
pub enum FormParseError {
    /// The bytes were not valid JSON, or did not match the schema.
    Json(serde_json::Error),
    /// The `schema` tag is not a recognized `quillmark/form@…` string.
    BadSchema(String),
    /// Two fields share the same `name`. AcroForm top-level field names must be
    /// unique — duplicates would stamp two `/T`-colliding fields and render as a
    /// single malformed field, so reject them at parse time (mirroring the
    /// Typst producer, which rejects duplicate `form-field` names).
    DuplicateField(String),
}

impl std::fmt::Display for FormParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormParseError::Json(e) => write!(f, "form.json is not valid: {e}"),
            FormParseError::BadSchema(s) => write!(
                f,
                "form.json `schema` is {s:?}, expected a \"{SCHEMA_PREFIX}<version>\" tag"
            ),
            FormParseError::DuplicateField(name) => write!(
                f,
                "form.json declares field name {name:?} more than once; field names must be unique"
            ),
        }
    }
}

impl FormSpec {
    /// Parse and validate a `form.json` byte slice.
    pub fn parse(bytes: &[u8]) -> Result<FormSpec, FormParseError> {
        let spec: FormSpec = serde_json::from_slice(bytes).map_err(FormParseError::Json)?;
        if !spec.schema.starts_with(SCHEMA_PREFIX) {
            return Err(FormParseError::BadSchema(spec.schema));
        }
        let mut seen = std::collections::HashSet::new();
        for field in &spec.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(FormParseError::DuplicateField(field.name.clone()));
            }
        }
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_four_kinds_and_ignores_unknown_keys() {
        let json = br#"{
          "schema": "quillmark/form@0.1.0",
          "fields": [
            { "name": "FullName", "schema_field": "full_name", "page": 0,
              "rect": { "x": 180, "y": 57, "w": 340, "h": 20 }, "type": "text",
              "tooltip": "Full legal name", "future_key": 42 },
            { "name": "Comments", "schema_field": "comments", "page": 0,
              "rect": { "x": 180, "y": 120, "w": 340, "h": 80 }, "type": "text", "multiline": true },
            { "name": "Agree", "schema_field": "agree", "page": 0,
              "rect": { "x": 180, "y": 90, "w": 14, "h": 14 }, "type": "checkbox" },
            { "name": "FavoriteColor", "schema_field": "favorite_color", "page": 0,
              "rect": { "x": 180, "y": 150, "w": 340, "h": 20 }, "type": "choice",
              "options": ["red", "green", "blue"] },
            { "name": "Signature", "page": 0,
              "rect": { "x": 180, "y": 190, "w": 340, "h": 40 }, "type": "signature" }
          ]
        }"#;
        let spec = FormSpec::parse(json).expect("parse ok");
        assert_eq!(spec.fields.len(), 5);
        assert_eq!(spec.fields[0].kind, FieldKind::Text { multiline: false });
        assert_eq!(spec.fields[0].schema_field.as_deref(), Some("full_name"));
        assert_eq!(spec.fields[1].kind, FieldKind::Text { multiline: true });
        assert_eq!(spec.fields[2].kind, FieldKind::Checkbox);
        assert_eq!(
            spec.fields[3].kind,
            FieldKind::Choice {
                options: vec!["red".into(), "green".into(), "blue".into()]
            }
        );
        assert_eq!(spec.fields[4].kind, FieldKind::Signature);
        // Unbound signature.
        assert_eq!(spec.fields[4].schema_field, None);
    }

    #[test]
    fn rejects_foreign_schema_tag() {
        let json = br#"{ "schema": "something/else@1", "fields": [] }"#;
        assert!(matches!(
            FormSpec::parse(json),
            Err(FormParseError::BadSchema(_))
        ));
    }

    #[test]
    fn rejects_duplicate_field_names() {
        let json = br#"{
          "schema": "quillmark/form@0.1.0",
          "fields": [
            { "name": "Dup", "page": 0, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 }, "type": "text" },
            { "name": "Dup", "page": 0, "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "type": "text" }
          ]
        }"#;
        match FormSpec::parse(json) {
            Err(FormParseError::DuplicateField(name)) => assert_eq!(name, "Dup"),
            other => panic!("expected DuplicateField, got {other:?}"),
        }
    }
}
