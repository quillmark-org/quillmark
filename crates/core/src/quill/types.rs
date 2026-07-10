//! Quill schema and core type definitions.
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::value::QuillValue;

/// UI-specific metadata for field rendering
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFieldSchema {
    /// Display label for the field — decoupled from the snake_case wire key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Automatically generated based on field position in Quill.yaml.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    /// Valid on `string` fields (plain text with newlines preserved) and `richtext` fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
}

/// Body namespace configuration for a card kind
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyCardSchema {
    /// When false, consumers must not accept or store body content for instances of this card kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Embedded verbatim in the blueprint body region; falls back to `Write <card> body here.` when absent.
    /// Has no effect when `enabled` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// Canonical-corpus form of [`example`](Self::example), imported once at
    /// quill load (`QuillConfig::from_yaml`) and cached here — a pure function of
    /// the Quill.yaml bytes, never serialized. Seeding commits this instead of
    /// re-importing the markdown per document, so a seeded body is corpus from
    /// birth. `None` when there is no example or the schema was built outside the
    /// loader (e.g. a hand-built test schema), in which case consumers fall back
    /// to importing `example`.
    #[serde(skip)]
    pub example_corpus: Option<QuillValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCardSchema {
    /// Display label for the card kind — literal string or `{field_name}`
    /// template. See `docs/quills/quill-yaml-reference.md`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Schema definition for a card kind (composable content blocks)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: BTreeMap<String, FieldSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiCardSchema>,
    /// Controls whether a body editor is shown and provides optional guide text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyCardSchema>,
}

impl CardSchema {
    /// Default values declared on this card's fields, keyed by field name. Fields with no `default` are omitted.
    pub fn defaults(&self) -> HashMap<String, QuillValue> {
        self.fields
            .iter()
            .filter_map(|(name, field)| field.default.as_ref().map(|v| (name.clone(), v.clone())))
            .collect()
    }

    /// Returns true if body content is permitted for instances of this card.
    /// Defaults to true when no `body` namespace is declared.
    pub fn body_enabled(&self) -> bool {
        self.body.as_ref().and_then(|b| b.enabled).unwrap_or(true)
    }
}

/// Field type hint enum for type-safe field type definitions.
///
/// Serializes as its type expression (`FieldType::as_str`) and deserializes by
/// parsing that string (`FieldType::from_str`), so a YAML `type:` value round-
/// trips as the one token that names it. Richtext inline shape is declared on
/// [`FieldSchema::inline`], not in the `type:` token.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    /// Integers and decimals
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    /// Formatted as string; validates against the YAML 1.1 timestamp grammar
    /// (bare `YYYY-MM-DD` through full RFC 3339 with offset).
    DateTime,
    /// Rich text — the canonical corpus content model ([`RichText`]). Surfaced
    /// as `type: richtext`; single-line shape is declared with [`FieldSchema::inline`].
    /// The transform schema marks it `contentMediaType:
    /// application/quillmark-richtext+json` and, when inline, `quillmark:inline:
    /// true`. The pre-richtext `markdown` spelling is not accepted — a
    /// Quill.yaml must declare `richtext` explicitly (`from_str` returns `None`
    /// for `markdown`, so the loader raises a schema load error).
    ///
    /// [`RichText`]: quillmark_richtext::RichText
    RichText {
        /// When `true`, the field is `richtext(inline)` — exactly one `Para`
        /// line, no container, no islands. Populated from [`FieldSchema::inline`]
        /// at load; editors mount a one-line surface. Enforced at coercion,
        /// validation, and load-time example import.
        inline: bool,
    },
}

impl FieldType {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "string" => Some(FieldType::String),
            "number" => Some(FieldType::Number),
            "integer" => Some(FieldType::Integer),
            "boolean" => Some(FieldType::Boolean),
            "array" => Some(FieldType::Array),
            "object" => Some(FieldType::Object),
            "datetime" => Some(FieldType::DateTime),
            "richtext" => Some(FieldType::RichText { inline: false }),
            // The pre-richtext `markdown` spelling is not a recognized type: it
            // returns `None` here so the loader reports it as an unknown type
            // rather than silently aliasing it to block richtext.
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Number => "number",
            FieldType::Integer => "integer",
            FieldType::Boolean => "boolean",
            FieldType::Array => "array",
            FieldType::Object => "object",
            FieldType::DateTime => "datetime",
            FieldType::RichText { .. } => "richtext",
        }
    }
}

impl Serialize for FieldType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Migration message for the retired `type: richtext(inline)` token — the
/// single source of truth shared by this deserializer's error and
/// `QuillConfig::field_parse_hint`'s hint text, so the two can't drift.
pub(crate) const RICHTEXT_INLINE_TOKEN_MSG: &str =
    "richtext(inline) is no longer accepted as a type token; use type: richtext with inline: true";

impl<'de> Deserialize<'de> for FieldType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.trim() == "richtext(inline)" {
            return Err(serde::de::Error::custom(RICHTEXT_INLINE_TOKEN_MSG));
        }
        FieldType::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown field type: {s:?}")))
    }
}

/// Schema definition for a template field.
///
/// `default` and `example` are both type-valid values with opposite intent:
/// `default` is the value most authors want (interpolated when the field is
/// omitted), while `example` matches the desired type and shape but is not
/// the value most authors want (it documents shape only, never rendering).
///
/// A field's *cell* is determined by `default`: a field with a `default:`
/// is **Endorsed** (the rendered value is shippable as-is), while a field
/// without a `default:` is **Unendorsed** (the author endorsed no value, so
/// the blueprint carries a `!must_fill` placeholder to ask for one). Absence is
/// not a requirement: a missing or null Unendorsed field zero-fills at
/// render. A surviving `!must_fill` placeholder is surfaced as the non-fatal
/// `validation::must_fill` warning. There is no separate `required:` axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    pub r#type: FieldType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The value most authors want; interpolated when the field is omitted.
    /// Its presence makes the field Endorsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<QuillValue>,
    /// A value matching the desired type and shape but not the value most
    /// authors want; documents shape only and never renders as the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<QuillValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiFieldSchema>,
    /// Restricts valid values on string fields.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Nested field schemas for `object` types (the typed dictionary's
    /// properties).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, Box<FieldSchema>>>,
    /// Element schema for `array` types. Required on every `array` field:
    /// the element type gives the array a concrete element type (`string[]`,
    /// `integer[]`, `richtext[]`, …). For a typed table the element is an
    /// `object` carrying its own `properties`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<FieldSchema>>,
    /// When `true` on a `richtext` field, the corpus must be exactly one `Para`
    /// line (no container, no islands). Omitted on block richtext and on every
    /// other type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline: Option<bool>,
    /// Canonical-corpus form of [`default`](Self::default) for a richtext-bearing
    /// field, imported once at quill load and cached — never serialized. The
    /// render floor (`resolve_fields`) commits this for an absent field, so a
    /// richtext default crosses the seam as corpus, not a re-imported string.
    /// `None` for a non-richtext field, a null/absent default, or a schema built
    /// outside the loader.
    #[serde(skip)]
    pub default_corpus: Option<QuillValue>,
    /// Canonical-corpus form of [`example`](Self::example) for a richtext-bearing
    /// field, imported once at quill load and cached — never serialized. Seeding
    /// commits this so a seeded field is corpus from birth. `None` under the same
    /// conditions as [`default_corpus`](Self::default_corpus).
    #[serde(skip)]
    pub example_corpus: Option<QuillValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchemaDef {
    pub r#type: FieldType,
    pub description: Option<String>,
    pub default: Option<QuillValue>,
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    // Nested schema support
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    // Element schema for arrays.
    pub items: Option<serde_json::Value>,
    pub inline: Option<bool>,
}

impl FieldSchema {
    /// Sort key for blueprint/form/seed field ordering: `ui.order` when set,
    /// else `i32::MAX` (declaration order is assigned to `ui.order` at parse
    /// time, so the fallback is defensive). The one shared ordering producer.
    pub fn ui_order(&self) -> i32 {
        self.ui.as_ref().and_then(|u| u.order).unwrap_or(i32::MAX)
    }

    pub fn new(name: String, r#type: FieldType, description: Option<String>) -> Self {
        Self {
            name,
            r#type,
            description,
            default: None,
            example: None,
            ui: None,
            enum_values: None,
            properties: None,
            items: None,
            inline: None,
            default_corpus: None,
            example_corpus: None,
        }
    }

    pub fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        let r#type = Self::resolve_richtext_inline(def.r#type, def.inline)?;
        let inline = match &r#type {
            FieldType::RichText { inline: true } => Some(true),
            _ => None,
        };
        Ok(Self {
            name: key.clone(),
            r#type,
            description: def.description,
            default: def.default,
            example: def.example,
            ui: def.ui,
            enum_values: def.enum_values,
            inline,
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
            items: if let Some(items) = def.items {
                Some(Box::new(FieldSchema::from_quill_value(
                    format!("{key}[]"),
                    &QuillValue::from_json(items),
                )?))
            } else {
                None
            },
            // Corpus caches are populated by the loader's post-pass
            // (`QuillConfig::from_yaml`), which alone imports and validates the
            // markdown literals; a bare `from_quill_value` leaves them empty.
            default_corpus: None,
            example_corpus: None,
        })
    }

    fn resolve_richtext_inline(
        r#type: FieldType,
        inline: Option<bool>,
    ) -> Result<FieldType, String> {
        match (r#type, inline) {
            (FieldType::RichText { .. }, Some(true)) => Ok(FieldType::RichText { inline: true }),
            (FieldType::RichText { .. }, Some(false) | None) => {
                Ok(FieldType::RichText { inline: false })
            }
            (_, Some(_)) => Err(
                "inline is only valid on type: richtext fields; omit inline or declare type: richtext"
                    .to_string(),
            ),
            (other, None) => Ok(other),
        }
    }
}
