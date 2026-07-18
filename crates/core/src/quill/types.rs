//! Quill schema and core type definitions.
use std::collections::HashMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::QuillValue;

/// UI-specific metadata for field rendering.
///
/// Field display order is not a `ui` knob: declaration order in Quill.yaml
/// **is** display order, carried structurally by the schema's ordered field
/// maps ([`CardSchema::fields`], [`FieldSchema::properties`]) and by key order
/// on the emitted-schema wire. The retired `ui.order` key is rejected with a
/// pointed message (`UI_ORDER_REMOVED_MSG`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UiFieldSchema {
    /// Display label for the field — decoupled from the snake_case wire key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    /// Valid on `string` fields (plain text with newlines preserved) and `richtext` fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
}

/// Migration message for the retired `ui.order` key — shared by the
/// [`UiFieldSchema`] deserializer's error and `QuillConfig::field_parse_hint`'s
/// hint text, so the two can't drift.
pub(crate) const UI_ORDER_REMOVED_MSG: &str = "ui.order is no longer accepted; \
     field display order is declaration order — reorder the fields in Quill.yaml instead";

/// Wire shape of a `ui:` block. `order` is declared so its rejection carries
/// the migration message rather than serde's generic unknown-key error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiFieldSchemaDef {
    title: Option<String>,
    group: Option<String>,
    order: Option<serde_json::Value>,
    compact: Option<bool>,
    multiline: Option<bool>,
}

impl<'de> Deserialize<'de> for UiFieldSchema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let def = UiFieldSchemaDef::deserialize(deserializer)?;
        if def.order.is_some() {
            return Err(serde::de::Error::custom(UI_ORDER_REMOVED_MSG));
        }
        Ok(UiFieldSchema {
            title: def.title,
            group: def.group,
            compact: def.compact,
            multiline: def.multiline,
        })
    }
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
    /// Canonical-content form of [`example`](Self::example), imported once at
    /// quill load (`QuillConfig::from_yaml`) and cached here — a pure function of
    /// the Quill.yaml bytes, never serialized. Seeding commits this instead of
    /// re-importing the markdown per document, so a seeded body is content from
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
    /// The card's group registry: the visible table of contents that names
    /// every group a field may reference and fixes their display order. A
    /// field's `ui.group` is a *reference* into this registry, validated at
    /// load. Absent when the card declares no groups (or uses the deprecated
    /// implicit-group form).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<GroupRegistry>,
}

/// One entry in a card's [`GroupRegistry`]: a snake_case identity plus an
/// optional display-label override. The `id` decouples identity from label the
/// same way a field's snake_case key decouples from its `ui.title` — a rename
/// of the label touches one line and never breaks a `ui.group` reference or
/// persisted per-group editor state. When `title` is `None`, consumers derive
/// the label from `id` (`memo_for` → "Memo For"), exactly as they derive a
/// field label from its key.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSchema {
    /// snake_case identity; rides the registry map key (or list item) on the wire.
    pub id: String,
    /// Display-label override; `None` means derive the label from `id`.
    pub title: Option<String>,
}

/// A card's ordered group registry (`main.ui.groups` or a card kind's
/// `ui.groups`). Declaration order **is** display order — the same contract
/// fields carry through their ordered map — so it is held as a `Vec` that
/// survives regardless of surface form. Authored as either a sequence of ids
/// (`[addressing, letterhead]`, titles derived) or a mapping of id to
/// attributes (`{ letterhead: { title: … } }`, for label overrides); both fold
/// into the same ordered list. Serializes back to the canonical mapping form,
/// declaration order preserved.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRegistry(pub Vec<GroupSchema>);

/// The attribute block of a registry entry in the mapping authoring/emission
/// form (`id: { title: … }`). A bare `id:` (null) or `id: {}` carries no
/// override.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupEntryDef {
    title: Option<String>,
}

impl<'de> Deserialize<'de> for GroupRegistry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RegistryVisitor;
        impl<'de> serde::de::Visitor<'de> for RegistryVisitor {
            type Value = GroupRegistry;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a sequence of group ids or a mapping of group id to attributes")
            }

            // Sequence form: `[addressing, letterhead]` — bare ids, titles derived.
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<GroupRegistry, A::Error> {
                let mut groups = Vec::new();
                while let Some(id) = seq.next_element::<String>()? {
                    groups.push(GroupSchema { id, title: None });
                }
                Ok(GroupRegistry(groups))
            }

            // Mapping form: `{ addressing: {}, letterhead: { title: … } }`.
            // A null or `{}` value carries no override; declaration order is
            // preserved by serde_json's `preserve_order`.
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<GroupRegistry, A::Error> {
                let mut groups = Vec::new();
                while let Some((id, def)) = map.next_entry::<String, Option<GroupEntryDef>>()? {
                    groups.push(GroupSchema {
                        id,
                        title: def.and_then(|d| d.title),
                    });
                }
                Ok(GroupRegistry(groups))
            }
        }
        deserializer.deserialize_any(RegistryVisitor)
    }
}

impl Serialize for GroupRegistry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // Canonical form: the mapping, so a title override has a home and the
        // registry key (identity) is explicit. A title-less entry emits an
        // empty object; the map's declaration order carries the display-order
        // contract on the wire.
        #[derive(Serialize)]
        struct GroupEntryOut<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            title: Option<&'a str>,
        }
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for group in &self.0 {
            map.serialize_entry(
                &group.id,
                &GroupEntryOut {
                    title: group.title.as_deref(),
                },
            )?;
        }
        map.end()
    }
}

/// Schema definition for a card kind (composable content blocks)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Declaration order is display order: the map preserves Quill.yaml key
    /// order end-to-end (parse, iteration, `schema()` emission), so ordering
    /// needs no side-channel knob.
    pub fields: IndexMap<String, FieldSchema>,
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
/// trips as the one token that names it. Richtext inline shape is declared with
/// the sibling `inline:` key (folded into `FieldType::RichText { inline }`), not
/// in the `type:` token.
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
    /// Rich text — the canonical content content model ([`Content`]). Surfaced
    /// as `type: richtext`; single-line shape is declared with the sibling `inline:` key.
    /// The transform schema marks it `contentMediaType:
    /// application/quillmark-content+json` and, when inline, `quillmark:inline:
    /// true`. The pre-richtext `markdown` spelling is not accepted — a
    /// Quill.yaml must declare `richtext` explicitly (`from_str` returns `None`
    /// for `markdown`, so the loader raises a schema load error).
    ///
    /// [`Content`]: quillmark_content::Content
    RichText {
        /// When `true`, the field is `richtext(inline)` — exactly one `Para`
        /// line, no container, no islands. Populated from the wire `inline:` key
        /// at load; editors mount a one-line surface. Enforced at coercion,
        /// validation, and load-time example import.
        inline: bool,
    },
    /// Plain text — the same [`Content`] as the `richtext` codec produces,
    /// constrained mark-free and island-free (all `Para` lines, no containers),
    /// but authored and projected through a *literal* codec
    /// ([`from_plaintext`]/[`to_plaintext`]) rather than markdown: `*hi*` is four
    /// literal characters, verbatim both ways, never emphasis. Surfaced as
    /// `type: plaintext`; single-line shape is declared with the sibling
    /// `inline:` key, exactly as richtext. The transform schema marks it with the
    /// same `contentMediaType: application/quillmark-content+json` (so it
    /// inherits the whole nav/region/preview stack with no backend edits) plus a
    /// `quillmark:plain: true` annotation (so editors mount a formatting-free
    /// surface). Enforced at coercion, validation (`NotPlain`), and load-time
    /// example import via [`Content::is_plain`].
    ///
    /// [`Content`]: quillmark_content::Content
    /// [`Content::is_plain`]: quillmark_content::Content::is_plain
    /// [`from_plaintext`]: quillmark_content::from_plaintext
    /// [`to_plaintext`]: quillmark_content::to_plaintext
    PlainText {
        /// When `true`, the field is single-line plaintext — one `Para` line, no
        /// container. Populated from the wire `inline:` key at load, mirroring
        /// richtext. Enforced at coercion, validation, and load-time import.
        inline: bool,
    },
    /// A closed finite domain of string values — the "branch on this" data type.
    /// Surfaced as `type: enum` with a required `values:` list (carried in
    /// [`FieldSchema::enum_values`], the single storage shared with the
    /// deprecated `enum:` modifier on `string`). Projects to the idiomatic
    /// JSON-Schema `{type: string, enum: [...]}` — exactly the shape backends
    /// already consume, so promoting the token costs zero backend edits. Scoped
    /// to string-valued members in v1 (an enum is a branching key; numeric
    /// domains are range constraints on `number`, not enums).
    Enum,
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
            "plaintext" => Some(FieldType::PlainText { inline: false }),
            "enum" => Some(FieldType::Enum),
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
            FieldType::PlainText { .. } => "plaintext",
            FieldType::Enum => "enum",
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
///
/// The richtext single-line constraint has **one** carrier — the
/// `FieldType::RichText { inline }` enum. The wire's sibling `inline:` key folds
/// into that enum at deserialize (via [`from_quill_value`](Self::from_quill_value),
/// which the custom `Deserialize` below routes through), and the custom
/// `Serialize` re-emits it from the enum, so the flag can never live in two
/// places that disagree.
///
/// `Serialize`/`Deserialize` are hand-written (below) rather than derived: the
/// wire folds a sibling `inline:` key into the `FieldType` enum, `name` rides
/// the map key, and the `*_corpus` caches never serialize.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    /// The map key carries this on the wire; not serialized, to avoid duplication.
    pub name: String,
    pub r#type: FieldType,
    pub description: Option<String>,
    /// The value most authors want; interpolated when the field is omitted.
    /// Its presence makes the field Endorsed.
    pub default: Option<QuillValue>,
    /// A value matching the desired type and shape but not the value most
    /// authors want; documents shape only and never renders as the value.
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    /// Restricts valid values on string fields. Serializes as `enum`.
    pub enum_values: Option<Vec<String>>,
    /// Nested field schemas for `object` types (the typed dictionary's
    /// properties). Ordered: declaration order is display order at every
    /// nesting level, carried by the map itself.
    pub properties: Option<IndexMap<String, Box<FieldSchema>>>,
    /// Element schema for `array` types. Required on every `array` field:
    /// the element type gives the array a concrete element type (`string[]`,
    /// `integer[]`, `richtext[]`, …). For a typed table the element is an
    /// `object` carrying its own `properties`.
    pub items: Option<Box<FieldSchema>>,
    /// Canonical-content form of [`default`](Self::default) for a richtext-bearing
    /// field, imported once at quill load and cached — never serialized. The
    /// render floor (`resolve_fields`) commits this for an absent field, so a
    /// richtext default crosses the seam as content, not a re-imported string.
    /// `None` for a non-richtext field, a null/absent default, or a schema built
    /// outside the loader.
    pub default_corpus: Option<QuillValue>,
    /// Canonical-content form of [`example`](Self::example) for a richtext-bearing
    /// field, imported once at quill load and cached — never serialized. Seeding
    /// commits this so a seeded field is content from birth. `None` under the same
    /// conditions as [`default_corpus`](Self::default_corpus).
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
    /// The deprecated `enum:` modifier on `type: string`. Accepted for one
    /// release alongside the promoted `type: enum` + `values:` spelling; both
    /// land in [`FieldSchema::enum_values`].
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    /// The `values:` list required by the promoted `type: enum`. Merged with
    /// `enum_values` into the one carrier at load.
    pub values: Option<Vec<String>>,
    // Nested schema support
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    // Element schema for arrays.
    pub items: Option<serde_json::Value>,
    pub inline: Option<bool>,
}

impl FieldSchema {
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
            default_corpus: None,
            example_corpus: None,
        }
    }

    pub fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        // The sole inline sync point: the wire `inline:` key folds into the enum
        // here, so `FieldType::RichText { inline }` (and its `PlainText` sibling)
        // is the one carrier thereafter.
        let r#type = Self::resolve_prose_inline(def.r#type, def.inline)?;
        // Fold the two enum spellings into the one carrier: the promoted
        // `type: enum` requires `values:`; the deprecated `enum:` modifier is
        // accepted only on `string`. On any other type an `enum:`/`values:` key
        // is a hard error, not the old silent no-op.
        let enum_values = Self::resolve_enum_values(&r#type, def.enum_values, def.values)?;
        Ok(Self {
            name: key.clone(),
            r#type,
            description: def.description,
            default: def.default,
            example: def.example,
            ui: def.ui,
            enum_values,
            properties: if let Some(props) = def.properties {
                // Declaration order (preserved by serde_json's `preserve_order`)
                // carries straight into the `IndexMap`, so nested properties
                // render in authored order at every depth.
                let mut p = IndexMap::new();
                for (key, value) in props {
                    let prop =
                        FieldSchema::from_quill_value(key.clone(), &QuillValue::from_json(value))?;
                    p.insert(key, Box::new(prop));
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
            // Content caches are populated by the loader's post-pass
            // (`QuillConfig::from_yaml`), which alone imports and validates the
            // markdown literals; a bare `from_quill_value` leaves them empty.
            default_corpus: None,
            example_corpus: None,
        })
    }

    /// Fold the sibling `inline:` key into a prose type's enum payload. Both
    /// `richtext` and `plaintext` carry the single-line constraint; every other
    /// type rejects `inline:`.
    fn resolve_prose_inline(
        r#type: FieldType,
        inline: Option<bool>,
    ) -> Result<FieldType, String> {
        match (r#type, inline) {
            (FieldType::RichText { .. }, inline) => Ok(FieldType::RichText {
                inline: inline.unwrap_or(false),
            }),
            (FieldType::PlainText { .. }, inline) => Ok(FieldType::PlainText {
                inline: inline.unwrap_or(false),
            }),
            (_, Some(_)) => Err(
                "inline is only valid on prose types (type: richtext or type: plaintext); \
                 omit inline or declare a prose type"
                    .to_string(),
            ),
            (other, None) => Ok(other),
        }
    }

    /// Reconcile the two enum spellings into [`FieldSchema::enum_values`]:
    /// the promoted `type: enum` requires a non-empty `values:` list; `type:
    /// string` accepts the deprecated `enum:` modifier; any other type carrying
    /// either key is an error (the old silent no-op, made loud).
    fn resolve_enum_values(
        r#type: &FieldType,
        enum_key: Option<Vec<String>>,
        values_key: Option<Vec<String>>,
    ) -> Result<Option<Vec<String>>, String> {
        match r#type {
            FieldType::Enum => {
                if enum_key.is_some() {
                    return Err(
                        "type: enum declares its domain with values:, not enum:; rename the key"
                            .to_string(),
                    );
                }
                match values_key {
                    Some(v) if !v.is_empty() => Ok(Some(v)),
                    _ => Err("type: enum requires a non-empty values: list".to_string()),
                }
            }
            FieldType::String => {
                if values_key.is_some() {
                    return Err(
                        "values: is only valid on type: enum; on a string use the enum: modifier \
                         (deprecated) or declare type: enum"
                            .to_string(),
                    );
                }
                Ok(enum_key)
            }
            other => {
                if enum_key.is_some() || values_key.is_some() {
                    return Err(format!(
                        "enum:/values: is only valid on type: enum (or the deprecated enum: on \
                         type: string), not on type: {}",
                        other.as_str()
                    ));
                }
                Ok(None)
            }
        }
    }
}

impl Serialize for FieldSchema {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // `inline: true` is projected back out of the enum — the flag's single
        // carrier — so the wire round-trips. `name` rides the map key; the
        // `*_corpus` caches are load-time derivations, never serialized.
        let inline = matches!(self.r#type, FieldType::RichText { inline: true }).then_some(true);
        let len = 1
            + inline.is_some() as usize
            + self.description.is_some() as usize
            + self.default.is_some() as usize
            + self.example.is_some() as usize
            + self.ui.is_some() as usize
            + self.enum_values.is_some() as usize
            + self.properties.is_some() as usize
            + self.items.is_some() as usize;
        // Field order matches the struct declaration (the prior derived output),
        // so `inline` trails the block — golden schema snapshots don't drift.
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("type", &self.r#type)?;
        if let Some(v) = &self.description {
            map.serialize_entry("description", v)?;
        }
        if let Some(v) = &self.default {
            map.serialize_entry("default", v)?;
        }
        if let Some(v) = &self.example {
            map.serialize_entry("example", v)?;
        }
        if let Some(v) = &self.ui {
            map.serialize_entry("ui", v)?;
        }
        if let Some(v) = &self.enum_values {
            // The promoted type re-emits its domain as `values:`; the deprecated
            // modifier on `string` re-emits as `enum:`, so each spelling round-trips.
            let key = if matches!(self.r#type, FieldType::Enum) {
                "values"
            } else {
                "enum"
            };
            map.serialize_entry(key, v)?;
        }
        if let Some(v) = &self.properties {
            map.serialize_entry("properties", v)?;
        }
        if let Some(v) = &self.items {
            map.serialize_entry("items", v)?;
        }
        if let Some(v) = inline {
            map.serialize_entry("inline", &v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for FieldSchema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // One deserialize path, shared with `from_quill_value`, so the sibling
        // `inline:` key always folds into `FieldType::RichText { inline }` and
        // two-carrier drift is impossible. `name` is filled from the map key by
        // the container; a bare schema deserializes nameless.
        let value = serde_json::Value::deserialize(deserializer)?;
        FieldSchema::from_quill_value(String::new(), &QuillValue::from_json(value))
            .map_err(serde::de::Error::custom)
    }
}
