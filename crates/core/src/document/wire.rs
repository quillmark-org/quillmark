//! Canonical **live** wire form of a [`Card`] for language-binding APIs.
//!
//! [`CardWire`] is the single, core-owned translation between a [`Card`] and
//! the flat `{ kind, payloadItems, … }` shape that the WASM and Python bindings
//! exchange with JS/Python. Bindings serialize/deserialize this type instead of
//! hand-rolling their own per-card conversion, so the field/comment/`$`-entry
//! mapping lives in exactly one place.
//!
//! ## Why this is separate from the storage DTO
//!
//! The versioned storage DTO (`document::dto`, e.g. `CardV0_82_0`) is **frozen**
//! per schema version so persisted documents keep loading forever. `CardWire`
//! is the **current** API shape and is free to evolve with the bindings. They
//! are structurally similar today, but coupling the live API to a frozen
//! storage schema would chain one to the other's change cadence — so they are
//! deliberately distinct, both built on the live [`Card`]/[`Payload`] model.
//!
//! ## Shape
//!
//! The `$` system entries are hoisted to named fields (`kind`, `quill`, `id`,
//! `ext`, `seed`); `payload_items` carries only user fields and comments, in order.
//! Field/`$ext` *nested* comments are not represented here — they survive the
//! Markdown and storage round-trips, not this editable projection.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::payload::{MetaKey, Payload, PayloadItem};
use super::Card;
use crate::value::{PathSegment, QuillValue};
use crate::version::QuillReference;
use quillmark_richtext::RichText;

/// One entry in a [`CardWire`]'s `payload_items`: a user field or a comment.
/// The `$` system entries are hoisted onto [`CardWire`] itself, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemWire {
    /// A user-defined field.
    Field {
        key: String,
        value: JsonValue,
        /// `true` when the field itself is `key: !must_fill <value>` in source.
        #[serde(default)]
        fill: bool,
        /// Paths to `!must_fill` markers nested *inside* `value` (e.g. a leaf
        /// property of an object, or a key within an array element). The JSON
        /// `value` projection is fill-free, so these carry the nested markers
        /// across the wire. Empty for a top-level-only or no-fill field.
        #[serde(
            default,
            rename = "nestedFills",
            alias = "nested_fills",
            skip_serializing_if = "Vec::is_empty"
        )]
        nested_fills: Vec<Vec<PathStepWire>>,
    },
    /// A YAML comment line (text excludes the leading `#`).
    Comment {
        text: String,
        /// `true` for a trailing inline comment (`field: value # text`).
        #[serde(default)]
        inline: bool,
    },
}

/// One step in a nested fill path: an object key or an array index. Serializes
/// **untagged** — a key as a JSON string, an index as a JSON number — so a path
/// is a plain JS array like `["addr", "street"]` or `["recipients", 0, "name"]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathStepWire {
    Index(usize),
    Key(String),
}

impl From<&PathSegment> for PathStepWire {
    fn from(seg: &PathSegment) -> Self {
        match seg {
            PathSegment::Key(k) => PathStepWire::Key(k.clone()),
            PathSegment::Index(i) => PathStepWire::Index(*i),
        }
    }
}

impl From<&PathStepWire> for PathSegment {
    fn from(seg: &PathStepWire) -> Self {
        match seg {
            PathStepWire::Key(k) => PathSegment::Key(k.clone()),
            PathStepWire::Index(i) => PathSegment::Index(*i),
        }
    }
}

/// Canonical live wire form of a [`Card`]. See the module docs.
///
/// Serializes to JS-facing camelCase (`payloadItems`); the snake_case
/// `payload_items` is also accepted on input for the Python binding.
/// `deny_unknown_fields` makes a stale flat `{ kind, fields }` shape fail
/// loudly rather than deserialize into an empty card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardWire {
    /// The block's `$kind` (e.g. `"endorsement"`); empty string when the block
    /// declares no `$kind`. Kept non-optional to match the binding read shape.
    #[serde(default)]
    pub kind: String,
    /// The block's `$quill` reference string (`name@version`), present on the
    /// main card only. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quill: Option<String>,
    /// The block's `$id`, if any. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The block's opaque `$ext` map, if declared. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<JsonMap<String, JsonValue>>,
    /// The block's `$seed` map (keyed by card-kind), if declared. Present on
    /// the main card only. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<JsonMap<String, JsonValue>>,
    /// User fields and comments, in source order.
    #[serde(default, alias = "payload_items")]
    pub payload_items: Vec<PayloadItemWire>,
    /// The card body as canonical RichText-JSON — the source-of-truth content
    /// model (a corpus object, `{text, lines, marks, islands}`). The empty corpus
    /// when absent. A markdown string is also accepted on input (imported), so an
    /// LLM/markdown writer can still hand a string here.
    #[serde(default)]
    pub body: JsonValue,
    /// The body's markdown projection (`export ∘ body`) — a read convenience, not
    /// stored state. Always emitted (empty string for an empty body, so consumers
    /// need not guard for absence); ignored on input (`body` is authoritative, so
    /// a round-trip through this shape is lossless regardless of `bodyMarkdown`).
    /// The snake_case `body_markdown` is also accepted on input (the Python dict
    /// shape), like `payload_items`.
    #[serde(default, alias = "body_markdown")]
    pub body_markdown: String,
}

/// Failure converting a [`CardWire`] back into a [`Card`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// The `quill` string is not a valid `name@version` reference.
    InvalidQuillReference { value: String, reason: String },
    /// A field violates the payload invariant: a name failing
    /// `[A-Za-z_][A-Za-z0-9_]*`, or a value (including `$ext`) nesting past the
    /// §8 depth limit.
    InvalidField { key: String, reason: String },
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::InvalidQuillReference { value, reason } => {
                write!(f, "invalid `quill` reference {value:?}: {reason}")
            }
            WireError::InvalidField { key, reason } => {
                write!(f, "invalid field {key:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for WireError {}

impl From<&Card> for CardWire {
    fn from(card: &Card) -> Self {
        let mut wire = CardWire {
            kind: String::new(),
            quill: None,
            id: None,
            ext: None,
            seed: None,
            payload_items: Vec::new(),
            body: quillmark_richtext::serial::to_canonical_value(card.body()),
            body_markdown: card.body_markdown(),
        };
        for item in card.payload().items() {
            match item {
                PayloadItem::Quill { reference } => wire.quill = Some(reference.to_string()),
                PayloadItem::Kind { value } => wire.kind = value.clone(),
                PayloadItem::Id { value } => wire.id = Some(value.clone()),
                PayloadItem::Meta {
                    key: MetaKey::Ext,
                    value,
                    ..
                } => wire.ext = Some(value.clone()),
                PayloadItem::Meta {
                    key: MetaKey::Seed,
                    value,
                    ..
                } => wire.seed = Some(value.clone()),
                PayloadItem::Field {
                    key, value, fill, ..
                } => {
                    let nested_fills = value
                        .nonroot_fill_paths()
                        .map(|p| p.iter().map(PathStepWire::from).collect())
                        .collect();
                    wire.payload_items.push(PayloadItemWire::Field {
                        key: key.clone(),
                        value: value.as_json().clone(),
                        fill: *fill,
                        nested_fills,
                    })
                }
                PayloadItem::Comment { text, inline } => {
                    wire.payload_items.push(PayloadItemWire::Comment {
                        text: text.clone(),
                        inline: *inline,
                    })
                }
            }
        }
        wire
    }
}

impl TryFrom<CardWire> for Card {
    type Error = WireError;

    fn try_from(wire: CardWire) -> Result<Self, Self::Error> {
        let items = wire
            .payload_items
            .into_iter()
            .map(|item| match item {
                PayloadItemWire::Field {
                    key,
                    value,
                    fill,
                    nested_fills,
                } => {
                    validate_wire_field(&key, &value)?;
                    let mut qv = QuillValue::from_json(value);
                    for path in &nested_fills {
                        let segs: Vec<PathSegment> = path.iter().map(PathSegment::from).collect();
                        qv.set_fill_at(&segs);
                    }
                    Ok(PayloadItem::Field {
                        key,
                        value: qv,
                        fill,
                        nested_comments: Vec::new(),
                    })
                }
                PayloadItemWire::Comment { text, inline } => {
                    Ok(PayloadItem::Comment { text, inline })
                }
            })
            .collect::<Result<Vec<_>, WireError>>()?;

        // Build the user fields/comments, then apply each `$` entry through its
        // setter so the canonical `$quill < $kind < $id < $ext < $seed` ordering
        // holds regardless of input order.
        let mut payload = Payload::from_items(items);
        if let Some(value) = wire.quill {
            let reference = QuillReference::from_str(&value)
                .map_err(|reason| WireError::InvalidQuillReference { value, reason })?;
            payload.set_quill(reference);
        }
        if !wire.kind.is_empty() {
            payload.set_kind(wire.kind);
        }
        if let Some(id) = wire.id {
            payload.set_id(id);
        }
        if let Some(ext) = wire.ext {
            let as_value = JsonValue::Object(ext);
            if crate::value::json_depth_exceeds(&as_value, crate::document::limits::MAX_YAML_DEPTH)
            {
                return Err(WireError::InvalidField {
                    key: "$ext".to_string(),
                    reason: format!(
                        "nests deeper than the maximum of {} levels",
                        crate::document::limits::MAX_YAML_DEPTH
                    ),
                });
            }
            let JsonValue::Object(ext) = as_value else {
                unreachable!("constructed as Object above")
            };
            payload.set_ext(ext);
        }
        if let Some(seed) = wire.seed {
            let as_value = JsonValue::Object(seed);
            if crate::value::json_depth_exceeds(&as_value, crate::document::limits::MAX_YAML_DEPTH)
            {
                return Err(WireError::InvalidField {
                    key: "$seed".to_string(),
                    reason: format!(
                        "nests deeper than the maximum of {} levels",
                        crate::document::limits::MAX_YAML_DEPTH
                    ),
                });
            }
            let JsonValue::Object(seed) = as_value else {
                unreachable!("constructed as Object above")
            };
            payload.set_seed(seed);
        }
        let body = body_from_wire(&wire.body)?;
        Ok(Card::from_parts(payload, body))
    }
}

/// Read a [`CardWire::body`] into a [`RichText`] corpus. The body is the source
/// of truth in two accepted encodings: a **corpus object** (an editor / a
/// re-serialized card) is deserialized and validated; a **markdown string** (an
/// LLM / markdown writer) is imported. `null`/absent is the empty corpus; any
/// other shape is an invalid `$body`. `body_markdown` is never read.
fn body_from_wire(body: &JsonValue) -> Result<RichText, WireError> {
    let invalid = |reason: String| WireError::InvalidField {
        key: "$body".to_string(),
        reason,
    };
    match super::decode_richtext_value(body) {
        Some(result) => result.map_err(|e| invalid(e.into_message())),
        // `null`/absent is the empty corpus; every other non-decodable shape is
        // an invalid `$body`.
        None => match body {
            JsonValue::Null => Ok(RichText::empty()),
            other => Err(invalid(format!(
                "expected a richtext corpus object or a markdown string, got {}",
                match other {
                    JsonValue::Bool(_) => "a boolean",
                    JsonValue::Number(_) => "a number",
                    JsonValue::Array(_) => "an array",
                    _ => "an unsupported value",
                }
            ))),
        },
    }
}

/// Validate a wire field against the payload invariant (see
/// `edit::validate_field`), mapping a violation to [`WireError::InvalidField`].
fn validate_wire_field(key: &str, value: &JsonValue) -> Result<(), WireError> {
    use super::edit::{validate_field, FieldViolation};
    validate_field(key, value).map_err(|v| WireError::InvalidField {
        key: key.to_string(),
        reason: match v {
            FieldViolation::InvalidName => {
                "field names must match [A-Za-z_][A-Za-z0-9_]*".to_string()
            }
            FieldViolation::TooDeep => format!(
                "nests deeper than the maximum of {} levels",
                crate::document::limits::MAX_YAML_DEPTH
            ),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Nested `!must_fill` markers inside a field value survive Card → wire →
    /// Card via the `nestedFills` path list (the JSON projection is fill-free).
    #[test]
    fn card_wire_round_trips_nested_fill() {
        let mut addr = QuillValue::from_json(json!({"street": null, "city": "Anytown"}));
        assert!(addr.set_fill_at(&[PathSegment::Key("street".to_string())]));
        let payload = Payload::from_items(vec![PayloadItem::Field {
            key: "addr".to_string(),
            value: addr,
            fill: false,
            nested_comments: Vec::new(),
        }]);
        let card = Card::from_parts(payload, quillmark_richtext::RichText::empty());

        let wire = CardWire::from(&card);
        let as_json = serde_json::to_value(&wire).unwrap();
        assert_eq!(
            as_json["payloadItems"][0]["nestedFills"],
            json!([["street"]]),
            "nested fill path rides the wire as a JS array; JSON value stays fill-free"
        );
        assert_eq!(
            as_json["payloadItems"][0]["value"],
            json!({"street": null, "city": "Anytown"})
        );

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card, "nested fill must survive Card → wire → Card");
    }

    /// A richtext field stored as a canonical corpus object rides the wire
    /// **structurally and losslessly** — the same opaque-JSON `Field` carrier as
    /// any object value, so identity marks (an `underline` with no markdown
    /// projection) survive Card → wire → Card. This is the lossless carrier the
    /// card-yaml markdown projection (emit) deliberately is not.
    #[test]
    fn card_wire_round_trips_corpus_field_losslessly() {
        use quillmark_richtext::model::{Mark, MarkKind};

        let mut card = Card::new("note").unwrap();
        let mut corpus = quillmark_richtext::import::from_markdown("underlined intro").unwrap();
        corpus.marks.push(Mark {
            start: 0,
            end: 10,
            kind: MarkKind::Underline,
        });
        corpus.normalize();
        let json = quillmark_richtext::serial::to_canonical_value(&corpus);
        card.set_field_richtext("intro", &json, false).unwrap();

        let wire = CardWire::from(&card);
        // Carried as the corpus object, verbatim — not a markdown projection.
        let as_json = serde_json::to_value(&wire).unwrap();
        assert!(as_json["payloadItems"][0]["value"].is_object());

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card, "corpus field must survive Card → wire → Card");
        // Underline (corpus-only, no markdown form) is intact after the round-trip.
        let read = back.field_richtext("intro").unwrap().unwrap();
        assert!(read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)));
    }

    /// A field-and-comment card with `$kind` round-trips Card → wire → Card.
    #[test]
    fn card_wire_round_trips_fields_and_comment() {
        let mut payload = Payload::from_items(vec![
            PayloadItem::comment("a note"),
            PayloadItem::field("title", QuillValue::from_json(json!("Hi"))),
            PayloadItem::Field {
                key: "count".to_string(),
                value: QuillValue::from_json(json!(3)),
                fill: true,
                nested_comments: Vec::new(),
            },
        ]);
        payload.set_kind("note");
        let card = Card::from_parts(payload, crate::document::import_body("body text").unwrap());

        let wire = CardWire::from(&card);
        assert_eq!(wire.kind, "note");
        assert_eq!(wire.payload_items.len(), 3);

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card, "Card → wire → Card must be identity");
    }

    /// `$quill` (main card) survives the round-trip and parses back.
    #[test]
    fn card_wire_round_trips_quill() {
        let mut payload = Payload::from_index_map(Default::default());
        payload.set_quill("memo@1.2.3".parse().unwrap());
        payload.set_kind("main");
        let card = Card::from_parts(payload, quillmark_richtext::RichText::empty());

        let wire = CardWire::from(&card);
        assert_eq!(wire.quill.as_deref(), Some("memo@1.2.3"));

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card);
    }

    /// The wire JSON uses camelCase `payloadItems` and the `type`-tagged items.
    #[test]
    fn card_wire_json_shape() {
        let card = Card::try_from(CardWire {
            kind: "note".to_string(),
            quill: None,
            id: None,
            ext: None,
            seed: None,
            payload_items: vec![PayloadItemWire::Field {
                key: "x".to_string(),
                value: json!(1),
                fill: false,
                nested_fills: Vec::new(),
            }],
            body: JsonValue::Null,
            body_markdown: String::new(),
        })
        .unwrap();
        let json = serde_json::to_value(CardWire::from(&card)).unwrap();
        assert_eq!(json["kind"], json!("note"));
        assert_eq!(json["payloadItems"][0]["type"], json!("field"));
        assert_eq!(json["payloadItems"][0]["key"], json!("x"));
        assert!(json.get("quill").is_none(), "absent quill is omitted");
    }

    /// A malformed `quill` string is a typed error, not a panic.
    #[test]
    fn card_wire_rejects_bad_quill() {
        let err = Card::try_from(CardWire {
            kind: String::new(),
            quill: Some("@nope".to_string()),
            id: None,
            ext: None,
            seed: None,
            payload_items: Vec::new(),
            body: JsonValue::Null,
            body_markdown: String::new(),
        })
        .unwrap_err();
        assert!(matches!(err, WireError::InvalidQuillReference { .. }));
    }
}
