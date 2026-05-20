//! Versioned, storage-stable serialization for [`Document`].
//!
//! [`Document`] and its component types (`Card`, `CardMetadata`, `Payload`,
//! …) track the evolving Quillmark model; their in-memory layout is an
//! internal detail and is deliberately *not* serialized directly. To persist
//! a document — e.g. in a database — it is converted to a [`StoredDocument`]:
//! a versioned envelope whose wire format is frozen per schema version.
//!
//! `Document` itself serializes through this envelope via
//! `#[serde(into / try_from)]`, so the ordinary serde entry points produce
//! and consume the versioned form transparently.
//!
//! ## Schema versioning
//!
//! The schema version tracks the crate version at which the `Document` wire
//! format was last changed — currently `0.81.0` (see [`SCHEMA_V0_81_0`]). A
//! wire-format change adds a new [`StoredDocument`] variant with its own
//! frozen type tree and a migration; older variants stay frozen so previously
//! stored rows keep deserializing.
//!
//! The canonical design — including the step-by-step procedure for adding a
//! schema version — is `prose/canon/DOCUMENT_STORAGE.md`.
//!
//! ## Wire-format naming
//!
//! The V0_81_0 wire format uses the names current at the time it was frozen
//! (`sentinel`, `frontmatter`, `tag`). The in-memory model has since renamed
//! these to `meta`, `payload`, and `kind` respectively; the conversion code
//! below maps between the two. The `#@id` system-metadata field, added after
//! V0_81_0 was frozen, is carried as an optional, soft-additive field on
//! the wire-format sentinel variants — documents without an `id` serialize
//! to byte-identical output, preserving the V0_81_0 byte-stability contract
//! for the original field set.

// Storage DTO types are deliberately named after the crate version that
// fixed their shape (e.g. `DocumentV0_81_0`); the underscores are intentional.
#![allow(non_camel_case_types)]

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::meta::{is_valid_kind_name, CardMetadata};
use super::payload::{Payload, PayloadItem};
use super::prescan::{CommentPathSegment, NestedComment};
use super::{Card, Document};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Schema version for the Document model as established in crate version
/// `0.81.0`. Bumped only when the wire format itself changes.
pub const SCHEMA_V0_81_0: &str = "quillmark/document@0.81.0";

/// Read the `schema` field from a raw storage DTO payload without performing
/// full deserialization.
///
/// Returns `None` if `json` is not valid JSON, is not an object, or has no
/// `schema` field. The returned string is **not** validated against the set
/// of supported schema versions — callers use this to distinguish "unknown
/// future version" from "corrupt payload" when [`Document`] deserialization
/// fails.
pub fn peek_schema_version(json: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Peek {
        schema: Option<String>,
    }
    serde_json::from_str::<Peek>(json).ok()?.schema
}

/// Versioned envelope for a persisted [`Document`].
///
/// The `schema` field selects the payload version. Deserialization
/// dispatches on it; unknown values are rejected. New schema versions are
/// added as new variants, leaving existing ones byte-stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema")]
pub enum StoredDocument {
    /// Document model as of crate version `0.81.0`.
    #[serde(rename = "quillmark/document@0.81.0")]
    V0_81_0(DocumentV0_81_0),
}

/// Frozen `0.81.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_81_0 {
    /// The document entry card.
    pub main: CardV0_81_0,
    /// Composable cards, in order.
    #[serde(default)]
    pub cards: Vec<CardV0_81_0>,
}

/// Frozen `0.81.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_81_0 {
    /// Card discriminator.
    pub sentinel: SentinelV0_81_0,
    /// Ordered frontmatter.
    #[serde(default)]
    pub frontmatter: FrontmatterV0_81_0,
    /// Markdown body following the card fence.
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.81.0` representation of a card discriminator.
///
/// Maps the V0_81_0 wire enum (`Main { quill }` vs `Card { tag }`) onto the
/// model's unified [`CardMetadata`]. The `id` field is a soft-additive
/// extension: absent on documents written before `#@id` existed, optional
/// on documents written after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SentinelV0_81_0 {
    /// Document entry card. `quill` is the rendered quill reference string
    /// (e.g. `usaf_memo@0.1`), parsed back via [`QuillReference::from_str`].
    /// The root's `#@kind` is always `"main"` per the spec (§3.3); the wire
    /// format omits it because the variant discriminator already implies it.
    Main {
        /// Quill reference string.
        quill: String,
        /// `#@id` opaque identifier, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Composable card with a kind tag.
    Card {
        /// Card kind tag (matches the model's `#@kind`).
        tag: String,
        /// `#@id` opaque identifier, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
}

/// Frozen `0.81.0` representation of a card payload.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrontmatterV0_81_0 {
    /// Ordered fields and top-level comments.
    #[serde(default)]
    pub items: Vec<FrontmatterItemV0_81_0>,
    /// Comments captured inside nested mappings/sequences.
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_81_0>,
}

/// Frozen `0.81.0` representation of a payload item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FrontmatterItemV0_81_0 {
    /// A YAML field.
    Field {
        /// Field key.
        key: String,
        /// Field value as raw JSON.
        value: serde_json::Value,
        /// `true` when tagged `!fill` in source.
        #[serde(default)]
        fill: bool,
    },
    /// A YAML comment.
    Comment {
        /// Comment text (no leading `#`).
        text: String,
        /// `true` for trailing inline comments.
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.81.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_81_0 {
    /// Path to the immediate parent container.
    pub container_path: Vec<CommentPathSegmentV0_81_0>,
    /// Slot or host index (see [`NestedComment`]).
    pub position: usize,
    /// Comment text.
    pub text: String,
    /// `true` for trailing inline comments.
    pub inline: bool,
}

/// Frozen `0.81.0` representation of a [`CommentPathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_81_0 {
    /// Mapping key.
    Key(String),
    /// Sequence index.
    Index(usize),
}

/// Failure while reconstructing a [`Document`] from a [`StoredDocument`].
#[derive(Debug, Clone, PartialEq)]
pub enum StorageError {
    /// A stored quill reference string could not be parsed.
    InvalidQuillReference {
        /// The offending string.
        value: String,
        /// Parser explanation.
        reason: String,
    },
    /// The document's `main` card carried a non-`Main` sentinel. The entry
    /// card must carry a `QUILL` reference.
    MainCardNotMain,
    /// A composable card carried a `Main` sentinel; only `main` may.
    ComposableCardIsMain,
    /// A composable card's tag was not a valid `[a-z_][a-z0-9_]*` identifier,
    /// or it was `"main"` — the reserved root kind.
    InvalidCardTag {
        /// The offending tag.
        tag: String,
    },
    /// A card carried more payload fields than
    /// [`MAX_FIELD_COUNT`](crate::error::MAX_FIELD_COUNT).
    TooManyFields {
        /// The field count found.
        count: usize,
    },
    /// A payload field used a reserved sentinel key
    /// (`BODY`, `CARDS`, `QUILL`, `CARD`).
    ReservedFieldName {
        /// The offending key.
        key: String,
    },
    /// Two payload fields in the same card shared a key.
    DuplicateFieldKey {
        /// The duplicated key.
        key: String,
    },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::InvalidQuillReference { value, reason } => {
                write!(f, "invalid quill reference {value:?}: {reason}")
            }
            StorageError::MainCardNotMain => {
                write!(f, "main card must carry a QUILL sentinel, not a card tag")
            }
            StorageError::ComposableCardIsMain => write!(
                f,
                "composable cards must carry a card tag, not a QUILL sentinel"
            ),
            StorageError::InvalidCardTag { tag } => {
                if tag == "main" {
                    write!(
                        f,
                        "card tag {tag:?} is reserved for the document root \
                         and may not appear on a composable card"
                    )
                } else {
                    write!(f, "invalid card tag {tag:?}: must match [a-z_][a-z0-9_]*")
                }
            }
            StorageError::TooManyFields { count } => write!(
                f,
                "card has {count} payload fields, exceeding the maximum of {}",
                crate::error::MAX_FIELD_COUNT
            ),
            StorageError::ReservedFieldName { key } => {
                write!(f, "reserved name {key:?} cannot be used as a field name")
            }
            StorageError::DuplicateFieldKey { key } => {
                write!(f, "duplicate payload field key {key:?}")
            }
        }
    }
}

impl std::error::Error for StorageError {}

/// Reject a payload no markdown-parsed `Document` could produce: too many
/// fields, a reserved sentinel key, or a duplicate key. The markdown parser
/// already rejects all three, so this only guards hand-crafted storage DTOs.
fn validate_dto_payload(payload: &Payload) -> Result<(), StorageError> {
    if payload.len() > crate::error::MAX_FIELD_COUNT {
        return Err(StorageError::TooManyFields {
            count: payload.len(),
        });
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for key in payload.keys() {
        if super::edit::is_reserved_name(key) {
            return Err(StorageError::ReservedFieldName { key: key.clone() });
        }
        if !seen.insert(key.as_str()) {
            return Err(StorageError::DuplicateFieldKey { key: key.clone() });
        }
    }
    Ok(())
}

// ── Document → StoredDocument (infallible) ────────────────────────────────────

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V0_81_0(DocumentV0_81_0::from(&doc))
    }
}

impl From<&Document> for DocumentV0_81_0 {
    fn from(doc: &Document) -> Self {
        DocumentV0_81_0 {
            main: CardV0_81_0::from(doc.main()),
            cards: doc.cards().iter().map(CardV0_81_0::from).collect(),
        }
    }
}

impl From<&Card> for CardV0_81_0 {
    fn from(card: &Card) -> Self {
        CardV0_81_0 {
            sentinel: SentinelV0_81_0::from(card.meta()),
            frontmatter: FrontmatterV0_81_0::from(card.payload()),
            body: card.body().to_string(),
        }
    }
}

impl From<&CardMetadata> for SentinelV0_81_0 {
    fn from(meta: &CardMetadata) -> Self {
        // A valid Card has either `quill` set (the main/entry card) or `kind`
        // set (a composable card); the model invariant rules out both being
        // `None` on a card produced by parsing or by the `Card::new` editor.
        // If both happen to be set (only constructable via low-level
        // `Card::from_parts`), preferring `quill` keeps the entry-card shape
        // intact on the wire — the root's `kind` is always `"main"` per the
        // spec and is implied by the `Main` variant itself.
        if let Some(reference) = &meta.quill {
            SentinelV0_81_0::Main {
                quill: reference.to_string(),
                id: meta.id.clone(),
            }
        } else {
            let tag = meta.kind.clone().unwrap_or_default();
            SentinelV0_81_0::Card {
                tag,
                id: meta.id.clone(),
            }
        }
    }
}

impl From<&Payload> for FrontmatterV0_81_0 {
    fn from(payload: &Payload) -> Self {
        FrontmatterV0_81_0 {
            items: payload
                .items()
                .iter()
                .map(FrontmatterItemV0_81_0::from)
                .collect(),
            nested_comments: payload
                .nested_comments()
                .iter()
                .map(NestedCommentV0_81_0::from)
                .collect(),
        }
    }
}

impl From<&PayloadItem> for FrontmatterItemV0_81_0 {
    fn from(item: &PayloadItem) -> Self {
        match item {
            PayloadItem::Field { key, value, fill } => FrontmatterItemV0_81_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
                fill: *fill,
            },
            PayloadItem::Comment { text, inline } => FrontmatterItemV0_81_0::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV0_81_0 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV0_81_0 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV0_81_0::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&CommentPathSegment> for CommentPathSegmentV0_81_0 {
    fn from(seg: &CommentPathSegment) -> Self {
        match seg {
            CommentPathSegment::Key(k) => CommentPathSegmentV0_81_0::Key(k.clone()),
            CommentPathSegment::Index(i) => CommentPathSegmentV0_81_0::Index(*i),
        }
    }
}

// ── StoredDocument → Document (fallible) ──────────────────────────────────────

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        match stored {
            StoredDocument::V0_81_0(payload) => Document::try_from(payload),
        }
    }
}

impl TryFrom<DocumentV0_81_0> for Document {
    type Error = StorageError;

    fn try_from(payload: DocumentV0_81_0) -> Result<Self, Self::Error> {
        let main = Card::try_from(payload.main)?;
        if main.meta().quill.is_none() {
            return Err(StorageError::MainCardNotMain);
        }
        let cards = payload
            .cards
            .into_iter()
            .map(Card::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        if cards.iter().any(|c| c.meta().quill.is_some()) {
            return Err(StorageError::ComposableCardIsMain);
        }
        Ok(Document::from_main_and_cards(main, cards, Vec::new()))
    }
}

impl TryFrom<CardV0_81_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_81_0) -> Result<Self, Self::Error> {
        let meta = CardMetadata::try_from(card.sentinel)?;
        let payload = Payload::from(card.frontmatter);
        validate_dto_payload(&payload)?;
        Ok(Card::from_parts(meta, payload, card.body))
    }
}

impl TryFrom<SentinelV0_81_0> for CardMetadata {
    type Error = StorageError;

    fn try_from(sentinel: SentinelV0_81_0) -> Result<Self, Self::Error> {
        match sentinel {
            SentinelV0_81_0::Main { quill, id } => {
                let reference = QuillReference::from_str(&quill).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: quill.clone(),
                        reason,
                    }
                })?;
                // The `Main` variant implies `#@kind: main` (spec §3.3); the
                // reconstructed model carries the canonical kind so the
                // markdown emit emits `#@kind: main` and the round-trip is
                // symmetric with the parser, which requires it.
                Ok(CardMetadata {
                    quill: Some(reference),
                    kind: Some("main".to_string()),
                    id,
                })
            }
            SentinelV0_81_0::Card { tag, id } => {
                if !is_valid_kind_name(&tag) || tag == "main" {
                    return Err(StorageError::InvalidCardTag { tag });
                }
                Ok(CardMetadata {
                    quill: None,
                    kind: Some(tag),
                    id,
                })
            }
        }
    }
}

impl From<FrontmatterV0_81_0> for Payload {
    fn from(fm: FrontmatterV0_81_0) -> Self {
        let items = fm.items.into_iter().map(PayloadItem::from).collect();
        let nested = fm
            .nested_comments
            .into_iter()
            .map(NestedComment::from)
            .collect();
        Payload::from_items_with_nested(items, nested)
    }
}

impl From<FrontmatterItemV0_81_0> for PayloadItem {
    fn from(item: FrontmatterItemV0_81_0) -> Self {
        match item {
            FrontmatterItemV0_81_0::Field { key, value, fill } => PayloadItem::Field {
                key,
                value: QuillValue::from_json(value),
                fill,
            },
            FrontmatterItemV0_81_0::Comment { text, inline } => {
                PayloadItem::Comment { text, inline }
            }
        }
    }
}

impl From<NestedCommentV0_81_0> for NestedComment {
    fn from(nc: NestedCommentV0_81_0) -> Self {
        NestedComment {
            container_path: nc
                .container_path
                .into_iter()
                .map(CommentPathSegment::from)
                .collect(),
            position: nc.position,
            text: nc.text,
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_81_0> for CommentPathSegment {
    fn from(seg: CommentPathSegmentV0_81_0) -> Self {
        match seg {
            CommentPathSegmentV0_81_0::Key(k) => CommentPathSegment::Key(k),
            CommentPathSegmentV0_81_0::Index(i) => CommentPathSegment::Index(i),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        Document::from_markdown(
            "\
~~~card-yaml
#@quill: usaf_memo@0.1
#@kind: main
# a top-level comment
memo_for:
  - ORG/SYMBOL # inline comment inside a sequence
date: 2504-10-05
subject: !fill Subject of the Memorandum
~~~

The body of the memorandum.

~~~card-yaml
#@kind: indorsement
for: ORG/SYMBOL
from: ORG/SYMBOL
~~~

This body and the metadata above are an indorsement card.
",
        )
        .unwrap()
    }

    #[test]
    fn round_trips_through_serde_json() {
        let doc = sample();
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, restored);
        assert_eq!(doc.to_markdown(), restored.to_markdown());
    }

    /// Every parsed root carries `meta.kind == Some("main")` per the spec,
    /// and the DTO round-trip preserves that — the wire `Main` variant
    /// implies it, so the restored model populates it unconditionally.
    #[test]
    fn root_kind_is_main_through_round_trip() {
        let doc = Document::from_markdown(
            "~~~card-yaml\n#@quill: usaf_memo@0.1\n#@kind: main\ntitle: \"Hi\"\n~~~\n",
        )
        .unwrap();
        assert_eq!(doc.main().meta().kind.as_deref(), Some("main"));
        let restored: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, restored);
        assert_eq!(restored.main().meta().kind.as_deref(), Some("main"));
    }

    /// A composable card tagged `"main"` on the wire must be rejected on
    /// reconstruction — `main` is reserved for the document root.
    #[test]
    fn rejects_composable_card_tagged_main() {
        let json = r#"{"schema":"quillmark/document@0.81.0",
            "main":{"sentinel":{"kind":"main","quill":"usaf_memo@0.1"},
                    "frontmatter":{},"body":""},
            "cards":[{"sentinel":{"kind":"card","tag":"main"},
                      "frontmatter":{},"body":""}]}"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("reserved for the document root"));
    }

    #[test]
    fn serialization_is_byte_deterministic() {
        // The wasm `toJson` docstring promises byte-equal output for
        // equal documents within a schema version; consumers content-hash
        // the result for divergence detection. Three guarantees are
        // checked: re-serialization stability, round-trip stability, and
        // path-independence — a parsed document and a DTO-restored copy of
        // it serialize to the same bytes.
        let doc = sample();
        let first = serde_json::to_string(&doc).unwrap();
        let second = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            first, second,
            "to_string must be deterministic (byte-equal on repeated calls)"
        );
        let restored: Document = serde_json::from_str(&first).unwrap();
        let third = serde_json::to_string(&restored).unwrap();
        assert_eq!(
            first, third,
            "byte-equality must survive a round-trip through fromJson/toJson"
        );

        // Path independence: documents arrived at by different routes but
        // value-equal must serialize identically.
        let from_markdown = sample();
        let from_dto: Document =
            serde_json::from_str(&serde_json::to_string(&from_markdown).unwrap()).unwrap();
        assert_eq!(from_markdown, from_dto);
        assert_eq!(
            serde_json::to_string(&from_markdown).unwrap(),
            serde_json::to_string(&from_dto).unwrap(),
            "value-equal documents must serialize to byte-equal strings regardless of construction path"
        );
    }

    #[test]
    fn serialized_form_carries_schema_version() {
        let doc = sample();
        let value: serde_json::Value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["schema"], SCHEMA_V0_81_0);
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let json = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }

    #[test]
    fn peek_schema_version_reads_field_without_full_parse() {
        let doc = sample();
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(peek_schema_version(&json).as_deref(), Some(SCHEMA_V0_81_0));

        // Unknown future version: peek still succeeds, even though full
        // deserialization would reject it.
        let future = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        assert_eq!(
            peek_schema_version(future).as_deref(),
            Some("quillmark/document@0.99.0")
        );

        // Not JSON, no schema field, wrong type — all None.
        assert_eq!(peek_schema_version("not json"), None);
        assert_eq!(peek_schema_version(r#"{"foo":"bar"}"#), None);
        assert_eq!(peek_schema_version(r#"{"schema":42}"#), None);
        assert_eq!(peek_schema_version("[1,2,3]"), None);
    }

    #[test]
    fn rejects_invalid_quill_reference() {
        let stored = StoredDocument::V0_81_0(DocumentV0_81_0 {
            main: CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "not a valid ref!!".to_string(),
                    id: None,
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            },
            cards: Vec::new(),
        });
        let err = Document::try_from(stored).unwrap_err();
        assert!(matches!(err, StorageError::InvalidQuillReference { .. }));
    }

    #[test]
    fn rejects_main_card_with_card_sentinel() {
        // A crafted DTO whose `main` carries a card tag instead of a QUILL
        // reference must be rejected — not built into a Document that later
        // panics in `to_markdown()` / the render path.
        let json = r#"{"schema":"quillmark/document@0.81.0",
            "main":{"sentinel":{"kind":"card","tag":"x"},"frontmatter":{},"body":""}}"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err
            .to_string()
            .contains("main card must carry a QUILL sentinel"));
    }

    #[test]
    fn rejects_composable_card_with_main_sentinel() {
        let stored = StoredDocument::V0_81_0(DocumentV0_81_0 {
            main: CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "usaf_memo@0.1".to_string(),
                    id: None,
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            },
            cards: vec![CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "usaf_memo@0.1".to_string(),
                    id: None,
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            }],
        });
        let err = Document::try_from(stored).unwrap_err();
        assert_eq!(err, StorageError::ComposableCardIsMain);
    }

    /// A minimal valid main card wrapping `fm`, with no composable cards.
    fn doc_with_main_frontmatter(fm: FrontmatterV0_81_0) -> StoredDocument {
        StoredDocument::V0_81_0(DocumentV0_81_0 {
            main: CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "usaf_memo@0.1".to_string(),
                    id: None,
                },
                frontmatter: fm,
                body: String::new(),
            },
            cards: Vec::new(),
        })
    }

    fn dto_field(key: &str) -> FrontmatterItemV0_81_0 {
        FrontmatterItemV0_81_0::Field {
            key: key.to_string(),
            value: serde_json::json!("x"),
            fill: false,
        }
    }

    #[test]
    fn rejects_invalid_card_tag() {
        let stored = StoredDocument::V0_81_0(DocumentV0_81_0 {
            main: CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "usaf_memo@0.1".to_string(),
                    id: None,
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            },
            cards: vec![CardV0_81_0 {
                sentinel: SentinelV0_81_0::Card {
                    tag: "Bad Tag".to_string(),
                    id: None,
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            }],
        });
        let err = Document::try_from(stored).unwrap_err();
        assert!(matches!(err, StorageError::InvalidCardTag { .. }));
    }

    #[test]
    fn rejects_reserved_field_name() {
        let fm = FrontmatterV0_81_0 {
            items: vec![dto_field("BODY")],
            nested_comments: Vec::new(),
        };
        let err = Document::try_from(doc_with_main_frontmatter(fm)).unwrap_err();
        assert!(matches!(err, StorageError::ReservedFieldName { .. }));
    }

    #[test]
    fn rejects_duplicate_field_key() {
        let fm = FrontmatterV0_81_0 {
            items: vec![dto_field("a"), dto_field("a")],
            nested_comments: Vec::new(),
        };
        let err = Document::try_from(doc_with_main_frontmatter(fm)).unwrap_err();
        assert!(matches!(err, StorageError::DuplicateFieldKey { .. }));
    }

    #[test]
    fn rejects_too_many_fields() {
        let items = (0..=crate::error::MAX_FIELD_COUNT)
            .map(|i| dto_field(&format!("f{i}")))
            .collect();
        let fm = FrontmatterV0_81_0 {
            items,
            nested_comments: Vec::new(),
        };
        let err = Document::try_from(doc_with_main_frontmatter(fm)).unwrap_err();
        assert!(matches!(err, StorageError::TooManyFields { .. }));
    }

    #[test]
    fn accepts_non_identifier_field_keys() {
        // The markdown parser produces keys like `memo-for`; the DTO must
        // round-trip them rather than reject them on charset grounds.
        let fm = FrontmatterV0_81_0 {
            items: vec![dto_field("memo-for")],
            nested_comments: Vec::new(),
        };
        assert!(Document::try_from(doc_with_main_frontmatter(fm)).is_ok());
    }

    #[test]
    fn explicit_dto_conversion_round_trips() {
        let doc = sample();
        let dto = DocumentV0_81_0::from(&doc);
        let restored = Document::try_from(dto).unwrap();
        assert_eq!(doc, restored);
    }
}
