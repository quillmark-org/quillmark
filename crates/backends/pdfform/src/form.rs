//! `form.json` wire types and parsing.
//!
//! `form.json` is the durable, value-free field-definition layer of a `pdfform`
//! quill — the *format* side of Quillmark's quill/document dichotomy. It is
//! complete enough to rebuild every widget yet flat and diffable. Unknown keys
//! are ignored (additive evolution needs no version bump); invalid type/payload
//! combinations are unrepresentable thanks to the internally-tagged `type`.

use serde::{Deserialize, Serialize};

/// The `schema` tag prefix every `form.json` must carry, following the Document
/// DTO convention (`quillmark/form@<version>`). V1 adopts only the field+value
/// format; the chained-migration machinery lands when a breaking change first
/// does.
pub const SCHEMA_PREFIX: &str = "quillmark/form@";

/// `skip_serializing_if` predicate for a `bool` that is `false` — keeps a
/// non-multiline text field from emitting `"multiline": false`, matching the
/// hand-authored fixture's shape.
fn is_false(b: &bool) -> bool {
    !*b
}

/// A parsed `form.json`: the schema tag plus the field reconstruction list.
///
/// Derives `Serialize` as well as `Deserialize` so the qualification layer can
/// emit a clean `form.json` from a reconstructed spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FormSpec {
    pub schema: String,
    pub fields: Vec<FormField>,
}

/// One field definition: identity + geometry + binding + kind.
///
/// `rect` is **top-left** `{x,y,w,h}` in PDF points, page-relative — the
/// loader flips it to the spine's bottom-left origin. `schema_field` is the
/// document field this binds to; `None` means unbound (a signer fills it).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FormField {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_field: Option<String>,
    pub page: usize,
    pub rect: Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    #[serde(flatten)]
    pub kind: FieldKind,
}

/// A top-left rectangle in PDF points (1/72").
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The kind of a [`FormField`] and its kind-specific definition. Internally
/// tagged by `type` and flattened into the field, so the JSON stays flat while
/// invalid combinations (a `signature` with `options`) are unrepresentable.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldKind {
    Text {
        #[serde(default, skip_serializing_if = "is_false")]
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
}

impl std::fmt::Display for FormParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormParseError::Json(e) => write!(f, "form.json is not valid: {e}"),
            FormParseError::BadSchema(s) => write!(
                f,
                "form.json `schema` is {s:?}, expected a \"{SCHEMA_PREFIX}<version>\" tag"
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
    fn serialize_round_trips_through_parse_and_omits_defaults() {
        let spec = FormSpec {
            schema: "quillmark/form@0.1.0".to_string(),
            fields: vec![
                FormField {
                    name: "FullName".into(),
                    schema_field: Some("full_name".into()),
                    page: 0,
                    rect: Rect {
                        x: 180.0,
                        y: 57.0,
                        w: 340.0,
                        h: 20.0,
                    },
                    tooltip: Some("Full legal name".into()),
                    kind: FieldKind::Text { multiline: false },
                },
                FormField {
                    name: "Agree".into(),
                    schema_field: None,
                    page: 0,
                    rect: Rect {
                        x: 180.0,
                        y: 90.0,
                        w: 14.0,
                        h: 14.0,
                    },
                    tooltip: None,
                    kind: FieldKind::Checkbox,
                },
            ],
        };

        let json = serde_json::to_string_pretty(&spec).expect("serialize ok");

        // The non-multiline text field must omit `multiline`; the checkbox must
        // omit `options`; the unbound checkbox must omit `schema_field` and
        // `tooltip`.
        assert!(
            !json.contains("multiline"),
            "non-multiline text must omit `multiline`: {json}"
        );
        assert!(
            !json.contains("options"),
            "checkbox must omit `options`: {json}"
        );
        // The first (bound) field keeps schema_field + tooltip; the second
        // (unbound) field has neither, so each key appears exactly once.
        assert_eq!(
            json.matches("schema_field").count(),
            1,
            "schema_field emitted only for the bound field: {json}"
        );
        assert_eq!(
            json.matches("tooltip").count(),
            1,
            "tooltip emitted only when present: {json}"
        );

        // The tagged `type` discriminant is flat (not nested under `kind`).
        assert!(json.contains("\"type\": \"text\""));
        assert!(json.contains("\"type\": \"checkbox\""));
        assert!(
            !json.contains("kind"),
            "the flattened, tagged enum must not surface a `kind` key: {json}"
        );

        // Round-trip back through the validating parser.
        let reparsed = FormSpec::parse(json.as_bytes()).expect("re-parse ok");
        assert_eq!(reparsed.schema, spec.schema);
        assert_eq!(reparsed.fields.len(), 2);
        assert_eq!(reparsed.fields[0].name, "FullName");
        assert_eq!(
            reparsed.fields[0].kind,
            FieldKind::Text { multiline: false }
        );
        assert_eq!(
            reparsed.fields[0].schema_field.as_deref(),
            Some("full_name")
        );
        assert_eq!(reparsed.fields[1].kind, FieldKind::Checkbox);
        assert_eq!(reparsed.fields[1].schema_field, None);
        assert_eq!(reparsed.fields[1].tooltip, None);
    }

    #[test]
    fn serialize_multiline_text_emits_multiline() {
        let spec = FormSpec {
            schema: "quillmark/form@0.1.0".to_string(),
            fields: vec![FormField {
                name: "Comments".into(),
                schema_field: Some("comments".into()),
                page: 0,
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                },
                tooltip: None,
                kind: FieldKind::Text { multiline: true },
            }],
        };
        let json = serde_json::to_string(&spec).expect("serialize ok");
        assert!(
            json.contains("\"multiline\":true"),
            "multiline text must emit `multiline: true`: {json}"
        );
    }
}
