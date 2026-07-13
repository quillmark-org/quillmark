//! `form.json` wire types and parsing (`form@0.2.0`).
//!
//! `form.json` is the durable, value-free **placement + binding + widget-identity**
//! layer of a `pdfform` quill — the *format* side of Quillmark's quill/document
//! dichotomy. As of `form@0.2.0` it no longer restates what the quill schema
//! already carries: a bound field names a `schema_field` and inherits its widget
//! kind, options, multiline, and tooltip from the resolved
//! [`FieldSchema`](quillmark_core::FieldSchema) (see [`crate::bind`]). Only the
//! two things the schema cannot know — where the widget sits (`page`/`rect`) and
//! which logical field it binds — live here.
//!
//! Two field populations, at different altitudes:
//! - **`fields`** — bound widgets. Each references a `schema_field`; its kind is
//!   *derived*, never declared, so `form.json` and the schema cannot drift.
//! - **`widgets`** — unbound widgets with no schema field (a signer-filled
//!   signature, an interactively-filled box). Having no schema to inherit from,
//!   each carries its own `type`.
//!
//! Unknown keys are ignored (additive evolution needs no version bump); invalid
//! type/payload combinations on an unbound widget are unrepresentable thanks to
//! the internally-tagged `type`.

use serde::Deserialize;

/// The `schema` tag prefix every `form.json` must carry, following the Document
/// DTO convention (`quillmark/form@<version>`).
pub const SCHEMA_PREFIX: &str = "quillmark/form@";

/// The `form.json` format version this backend reads. `0.2.0` slimmed bound
/// fields to a binding layer that derives widget intrinsics from the quill
/// schema; `0.1.0` (which restated `type`/`options`/`multiline`) is rejected at
/// load with a migration pointer.
pub const SCHEMA_VERSION: &str = "0.2.0";

/// The retired format version, rejected with migration guidance.
const RETIRED_VERSION_MAJOR_MINOR: &str = "0.1";
/// The accepted major.minor; a matching patch is tolerated.
const SUPPORTED_MAJOR_MINOR: &str = "0.2";

/// The working migration guide the version error points a stranded `0.1.0`
/// author at.
const MIGRATION_GUIDE: &str = "docs/migrations/0.93-to-0.94.md";

/// A parsed `form.json`: the two field populations. The `schema` tag is read and
/// version-gated separately ([`SchemaTag`]) before this is deserialized, so it
/// is not restated here — any `schema` key in the JSON is simply ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct FormSpec {
    /// Schema-bound widgets — kind/options/multiline/tooltip inherited from the
    /// referenced [`FieldSchema`](quillmark_core::FieldSchema).
    #[serde(default)]
    pub fields: Vec<BoundField>,
    /// Unbound widgets — no schema field, so each declares its own `type`.
    #[serde(default)]
    pub widgets: Vec<UnboundWidget>,
}

/// One **bound** field: identity + geometry + binding. Its widget kind, choice
/// options, and multiline flag are *not* here — they are derived from the
/// resolved [`FieldSchema`](quillmark_core::FieldSchema) at load
/// ([`crate::bind`]). `tooltip` is an optional override; when absent the field
/// inherits the schema field's `description`.
///
/// `rect` is **top-left** `{x,y,w,h}` in PDF points, page-relative — the loader
/// flips it to the spine's bottom-left origin.
#[derive(Debug, Clone, Deserialize)]
pub struct BoundField {
    pub name: String,
    /// The document field this widget binds to. Resolved against the quill
    /// schema at load; a dangling path is a load error, not a silent blank.
    pub schema_field: String,
    pub page: usize,
    pub rect: Rect,
    #[serde(default)]
    pub tooltip: Option<String>,
}

/// One **unbound** widget: identity + geometry + an explicit kind. Bound to no
/// schema field (a signer fills it), so its intrinsics are declared, not derived.
#[derive(Debug, Clone, Deserialize)]
pub struct UnboundWidget {
    pub name: String,
    pub page: usize,
    pub rect: Rect,
    #[serde(default)]
    pub tooltip: Option<String>,
    #[serde(flatten)]
    pub kind: WidgetKind,
}

/// A top-left rectangle in PDF points (1/72").
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The declared kind of an [`UnboundWidget`]. Internally tagged by `type` and
/// flattened into the widget, so the JSON stays flat while invalid combinations
/// (a `signature` with `options`) are unrepresentable. Bound fields carry no
/// such tag — their kind is derived from the schema.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WidgetKind {
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
    /// The `schema` tag names the retired `form@0.1.0` format. Surfaced with its
    /// own error code and a migration pointer, distinct from a foreign tag.
    RetiredVersion(String),
    /// Two fields/widgets share the same `name`. AcroForm top-level field names
    /// must be unique across the whole form — duplicates would stamp two
    /// `/T`-colliding fields and render as a single malformed field, so reject
    /// them at parse time (mirroring the Typst producer, which rejects duplicate
    /// `form-field` names).
    DuplicateField(String),
}

impl std::fmt::Display for FormParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormParseError::Json(e) => write!(f, "form.json is not valid: {e}"),
            FormParseError::BadSchema(s) => write!(
                f,
                "form.json `schema` is {s:?}, expected a \"{SCHEMA_PREFIX}{SCHEMA_VERSION}\" tag"
            ),
            FormParseError::RetiredVersion(s) => write!(
                f,
                "form.json `schema` is {s:?}; the `form@{RETIRED_VERSION_MAJOR_MINOR}.x` format is \
                 retired — bound fields no longer restate `type`/`options`/`multiline` (they are \
                 derived from the quill schema). Migrate to \"{SCHEMA_PREFIX}{SCHEMA_VERSION}\"; \
                 see {MIGRATION_GUIDE}"
            ),
            FormParseError::DuplicateField(name) => write!(
                f,
                "form.json declares field name {name:?} more than once; field names must be unique"
            ),
        }
    }
}

impl FormParseError {
    /// The stable error code a caller stamps on the surfaced diagnostic.
    pub fn code(&self) -> &'static str {
        match self {
            FormParseError::RetiredVersion(_) => "pdfform::form_schema_version",
            _ => "pdfform::invalid_form_json",
        }
    }
}

/// Just the `schema` tag, deserialized ahead of the full spec so the version
/// gate runs *before* field-shape validation. A retired `form@0.1.0` file
/// carries `0.1.0`-shaped fields (an unbound `signature` in `fields`, no
/// `schema_field`) that no longer deserialize into a [`BoundField`]; gating on
/// the tag first guarantees such a file gets the migration error, not a generic
/// "missing field" JSON error.
#[derive(Debug, Deserialize)]
struct SchemaTag {
    schema: String,
}

impl FormSpec {
    /// Parse and validate a `form.json` byte slice.
    pub fn parse(bytes: &[u8]) -> Result<FormSpec, FormParseError> {
        // Gate the version on the tag alone, before the field shapes are read.
        let tag: SchemaTag = serde_json::from_slice(bytes).map_err(FormParseError::Json)?;
        check_version(&tag.schema)?;
        let spec: FormSpec = serde_json::from_slice(bytes).map_err(FormParseError::Json)?;
        spec.check_unique_names()?;
        Ok(spec)
    }

    /// AcroForm `/T` names must be unique across *both* populations.
    fn check_unique_names(&self) -> Result<(), FormParseError> {
        let mut seen = std::collections::HashSet::new();
        for name in self.field_names() {
            if !seen.insert(name) {
                return Err(FormParseError::DuplicateField(name.to_string()));
            }
        }
        Ok(())
    }

    /// Every widget name, bound then unbound, in declaration order.
    fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .map(|f| f.name.as_str())
            .chain(self.widgets.iter().map(|w| w.name.as_str()))
    }
}

/// The `schema` tag must name the supported `form@0.2.x`. The retired `0.1.x`
/// gets a targeted migration error; anything else is a foreign tag.
fn check_version(schema: &str) -> Result<(), FormParseError> {
    let version = schema
        .strip_prefix(SCHEMA_PREFIX)
        .ok_or_else(|| FormParseError::BadSchema(schema.to_string()))?;
    if version_matches(version, SUPPORTED_MAJOR_MINOR) {
        Ok(())
    } else if version_matches(version, RETIRED_VERSION_MAJOR_MINOR) {
        Err(FormParseError::RetiredVersion(schema.to_string()))
    } else {
        Err(FormParseError::BadSchema(schema.to_string()))
    }
}

/// A version string matches `<major.minor>` when it is exactly that or carries a
/// `.patch` suffix — so `0.2` and `0.2.7` both match `"0.2"`, while `0.20` does
/// not.
fn version_matches(version: &str, major_minor: &str) -> bool {
    version == major_minor
        || version
            .strip_prefix(major_minor)
            .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bound_fields_and_unbound_widgets_ignoring_unknown_keys() {
        let json = br#"{
          "schema": "quillmark/form@0.2.0",
          "fields": [
            { "name": "FullName", "schema_field": "full_name", "page": 0,
              "rect": { "x": 180, "y": 57, "w": 340, "h": 20 },
              "tooltip": "Full legal name", "future_key": 42 },
            { "name": "Comments", "schema_field": "comments", "page": 0,
              "rect": { "x": 180, "y": 120, "w": 340, "h": 80 } }
          ],
          "widgets": [
            { "name": "Signature", "page": 0,
              "rect": { "x": 180, "y": 190, "w": 340, "h": 40 }, "type": "signature" }
          ]
        }"#;
        let spec = FormSpec::parse(json).expect("parse ok");
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].schema_field, "full_name");
        assert_eq!(spec.fields[0].tooltip.as_deref(), Some("Full legal name"));
        assert_eq!(spec.fields[1].tooltip, None);
        assert_eq!(spec.widgets.len(), 1);
        assert_eq!(spec.widgets[0].kind, WidgetKind::Signature);
    }

    #[test]
    fn unbound_widgets_carry_every_kind() {
        let json = br#"{
          "schema": "quillmark/form@0.2.0",
          "widgets": [
            { "name": "T", "page": 0, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 },
              "type": "text", "multiline": true },
            { "name": "C", "page": 0, "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "type": "checkbox" },
            { "name": "Ch", "page": 0, "rect": { "x": 0, "y": 4, "w": 1, "h": 1 },
              "type": "choice", "options": ["a", "b"] },
            { "name": "S", "page": 0, "rect": { "x": 0, "y": 6, "w": 1, "h": 1 }, "type": "signature" }
          ]
        }"#;
        let spec = FormSpec::parse(json).expect("parse ok");
        assert_eq!(spec.widgets[0].kind, WidgetKind::Text { multiline: true });
        assert_eq!(spec.widgets[1].kind, WidgetKind::Checkbox);
        assert_eq!(
            spec.widgets[2].kind,
            WidgetKind::Choice {
                options: vec!["a".into(), "b".into()]
            }
        );
        assert_eq!(spec.widgets[3].kind, WidgetKind::Signature);
    }

    #[test]
    fn empty_populations_default_to_empty_vecs() {
        let spec = FormSpec::parse(br#"{ "schema": "quillmark/form@0.2.0" }"#).expect("parse ok");
        assert!(spec.fields.is_empty());
        assert!(spec.widgets.is_empty());
    }

    #[test]
    fn accepts_patch_within_supported_minor() {
        assert!(FormSpec::parse(br#"{ "schema": "quillmark/form@0.2.7", "fields": [] }"#).is_ok());
    }

    #[test]
    fn rejects_retired_v1_with_migration_code() {
        let json = br#"{ "schema": "quillmark/form@0.1.0", "fields": [] }"#;
        match FormSpec::parse(json) {
            Err(e @ FormParseError::RetiredVersion(_)) => {
                assert_eq!(e.code(), "pdfform::form_schema_version");
                assert!(e.to_string().contains(MIGRATION_GUIDE));
            }
            other => panic!("expected RetiredVersion, got {other:?}"),
        }
    }

    #[test]
    fn retired_v1_with_v1_shaped_fields_still_gets_migration_error() {
        // A real 0.1.0 file carries an unbound `signature` in `fields` with no
        // `schema_field` — which no longer deserializes into a BoundField. The
        // version gate must fire on the tag *before* that shape is read, so the
        // author gets the migration pointer, not a generic "missing field" error.
        let json = br#"{
          "schema": "quillmark/form@0.1.0",
          "fields": [
            { "name": "FullName", "schema_field": "full_name", "page": 0,
              "rect": { "x": 0, "y": 0, "w": 1, "h": 1 }, "type": "text" },
            { "name": "Signature", "page": 0,
              "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "type": "signature" }
          ]
        }"#;
        match FormSpec::parse(json) {
            Err(e @ FormParseError::RetiredVersion(_)) => {
                assert_eq!(e.code(), "pdfform::form_schema_version");
            }
            other => panic!("expected RetiredVersion, got {other:?}"),
        }
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
    fn rejects_unknown_form_version_as_bad_schema() {
        let json = br#"{ "schema": "quillmark/form@9.9.9", "fields": [] }"#;
        assert!(matches!(
            FormSpec::parse(json),
            Err(FormParseError::BadSchema(_))
        ));
    }

    #[test]
    fn rejects_duplicate_names_across_populations() {
        let json = br#"{
          "schema": "quillmark/form@0.2.0",
          "fields": [
            { "name": "Dup", "schema_field": "a", "page": 0, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 } }
          ],
          "widgets": [
            { "name": "Dup", "page": 0, "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "type": "signature" }
          ]
        }"#;
        match FormSpec::parse(json) {
            Err(FormParseError::DuplicateField(name)) => assert_eq!(name, "Dup"),
            other => panic!("expected DuplicateField, got {other:?}"),
        }
    }

    #[test]
    fn version_matches_guards_adjacent_minors() {
        assert!(version_matches("0.2", "0.2"));
        assert!(version_matches("0.2.0", "0.2"));
        assert!(version_matches("0.2.15", "0.2"));
        assert!(!version_matches("0.20", "0.2"));
        assert!(!version_matches("0.21.0", "0.2"));
    }
}
