//! Versioned, storage-stable serialization for [`Document`].
//!
//! [`Document`] and its component types (`Card`, `Sentinel`, `Frontmatter`,
//! …) track the evolving Quillmark model; their in-memory layout is an
//! internal detail and is deliberately *not* serialized directly. To persist
//! a document — e.g. in a database — it is converted to a [`StoredDocument`]:
//! a versioned envelope whose wire format is frozen per schema version.
//!
//! `Document` itself serializes through this envelope via
//! `#[serde(into / try_from)]`, so the ordinary serde entry points produce
//! and consume the versioned form transparently:
//!
//! ```
//! use quillmark_core::Document;
//!
//! let doc = Document::from_markdown(
//!     "---\nQUILL: my_quill\ntitle: Hi\n---\n\nBody.\n"
//! ).unwrap();
//!
//! let json = serde_json::to_string(&doc).unwrap();
//! assert!(json.contains("\"schema\""));
//!
//! let restored: Document = serde_json::from_str(&json).unwrap();
//! assert_eq!(doc, restored);
//! ```
//!
//! ## Schema versioning
//!
//! The schema tag tracks the crate version at which the `Document` model was
//! last changed — currently `0.81.0` (see [`SCHEMA_V0_81_0`]). A model change
//! adds a new [`StoredDocument`] variant with its own frozen type tree and a
//! migration; older variants stay frozen so previously stored rows keep
//! deserializing.
//!
//! The canonical design — including the step-by-step procedure for adding a
//! schema version — is `prose/canon/DOCUMENT_STORAGE.md`.

// Storage DTO types are deliberately named after the crate version that
// fixed their shape (e.g. `DocumentV0_81_0`); the underscores are intentional.
#![allow(non_camel_case_types)]

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::frontmatter::{Frontmatter, FrontmatterItem};
use super::prescan::{CommentPathSegment, NestedComment};
use super::{Card, Document, Sentinel};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Schema tag for the Document model as established in crate version
/// `0.81.0`. Bumped only when the model itself changes.
pub const SCHEMA_V0_81_0: &str = "quillmark/document@0.81.0";

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

/// Frozen `0.81.0` representation of a [`Sentinel`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SentinelV0_81_0 {
    /// Document entry card. `quill` is the rendered quill reference string
    /// (e.g. `usaf_memo@0.1`), parsed back via [`QuillReference::from_str`].
    Main {
        /// Quill reference string.
        quill: String,
    },
    /// Composable card with a kind tag.
    Card {
        /// Card kind tag.
        tag: String,
    },
}

/// Frozen `0.81.0` representation of a [`Frontmatter`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrontmatterV0_81_0 {
    /// Ordered fields and top-level comments.
    #[serde(default)]
    pub items: Vec<FrontmatterItemV0_81_0>,
    /// Comments captured inside nested mappings/sequences.
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_81_0>,
}

/// Frozen `0.81.0` representation of a [`FrontmatterItem`].
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
    /// A composable card's tag was not a valid `[a-z_][a-z0-9_]*` identifier.
    InvalidCardTag {
        /// The offending tag.
        tag: String,
    },
    /// A card carried more frontmatter fields than
    /// [`MAX_FIELD_COUNT`](crate::error::MAX_FIELD_COUNT).
    TooManyFields {
        /// The field count found.
        count: usize,
    },
    /// A frontmatter field used a reserved sentinel key
    /// (`BODY`, `CARDS`, `QUILL`, `CARD`).
    ReservedFieldName {
        /// The offending key.
        key: String,
    },
    /// Two frontmatter fields in the same card shared a key.
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
                write!(f, "invalid card tag {tag:?}: must match [a-z_][a-z0-9_]*")
            }
            StorageError::TooManyFields { count } => write!(
                f,
                "card has {count} frontmatter fields, exceeding the maximum of {}",
                crate::error::MAX_FIELD_COUNT
            ),
            StorageError::ReservedFieldName { key } => {
                write!(f, "reserved name {key:?} cannot be used as a field name")
            }
            StorageError::DuplicateFieldKey { key } => {
                write!(f, "duplicate frontmatter field key {key:?}")
            }
        }
    }
}

/// Reject a frontmatter no markdown-parsed `Document` could produce: too many
/// fields, a reserved sentinel key, or a duplicate key. The markdown parser
/// already rejects all three, so this only guards hand-crafted storage DTOs.
fn validate_dto_frontmatter(fm: &Frontmatter) -> Result<(), StorageError> {
    if fm.len() > crate::error::MAX_FIELD_COUNT {
        return Err(StorageError::TooManyFields { count: fm.len() });
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for key in fm.keys() {
        if super::edit::is_reserved_name(key) {
            return Err(StorageError::ReservedFieldName { key: key.clone() });
        }
        if !seen.insert(key.as_str()) {
            return Err(StorageError::DuplicateFieldKey { key: key.clone() });
        }
    }
    Ok(())
}

impl std::error::Error for StorageError {}

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
            sentinel: SentinelV0_81_0::from(card.sentinel()),
            frontmatter: FrontmatterV0_81_0::from(card.frontmatter()),
            body: card.body().to_string(),
        }
    }
}

impl From<&Sentinel> for SentinelV0_81_0 {
    fn from(sentinel: &Sentinel) -> Self {
        match sentinel {
            Sentinel::Main(reference) => SentinelV0_81_0::Main {
                quill: reference.to_string(),
            },
            Sentinel::Card(tag) => SentinelV0_81_0::Card { tag: tag.clone() },
        }
    }
}

impl From<&Frontmatter> for FrontmatterV0_81_0 {
    fn from(fm: &Frontmatter) -> Self {
        FrontmatterV0_81_0 {
            items: fm
                .items()
                .iter()
                .map(FrontmatterItemV0_81_0::from)
                .collect(),
            nested_comments: fm
                .nested_comments()
                .iter()
                .map(NestedCommentV0_81_0::from)
                .collect(),
        }
    }
}

impl From<&FrontmatterItem> for FrontmatterItemV0_81_0 {
    fn from(item: &FrontmatterItem) -> Self {
        match item {
            FrontmatterItem::Field { key, value, fill } => FrontmatterItemV0_81_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
                fill: *fill,
            },
            FrontmatterItem::Comment { text, inline } => FrontmatterItemV0_81_0::Comment {
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
        if !main.sentinel().is_main() {
            return Err(StorageError::MainCardNotMain);
        }
        let cards = payload
            .cards
            .into_iter()
            .map(Card::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        if cards.iter().any(|c| c.sentinel().is_main()) {
            return Err(StorageError::ComposableCardIsMain);
        }
        Ok(Document::from_main_and_cards(main, cards))
    }
}

impl TryFrom<CardV0_81_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_81_0) -> Result<Self, Self::Error> {
        let sentinel = Sentinel::try_from(card.sentinel)?;
        let frontmatter = Frontmatter::from(card.frontmatter);
        validate_dto_frontmatter(&frontmatter)?;
        Ok(Card::new_with_sentinel(sentinel, frontmatter, card.body))
    }
}

impl TryFrom<SentinelV0_81_0> for Sentinel {
    type Error = StorageError;

    fn try_from(sentinel: SentinelV0_81_0) -> Result<Self, Self::Error> {
        match sentinel {
            SentinelV0_81_0::Main { quill } => {
                let reference = QuillReference::from_str(&quill).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: quill.clone(),
                        reason,
                    }
                })?;
                Ok(Sentinel::Main(reference))
            }
            SentinelV0_81_0::Card { tag } => {
                if !super::sentinel::is_valid_tag_name(&tag) {
                    return Err(StorageError::InvalidCardTag { tag });
                }
                Ok(Sentinel::Card(tag))
            }
        }
    }
}

impl From<FrontmatterV0_81_0> for Frontmatter {
    fn from(fm: FrontmatterV0_81_0) -> Self {
        let items = fm.items.into_iter().map(FrontmatterItem::from).collect();
        let nested = fm
            .nested_comments
            .into_iter()
            .map(NestedComment::from)
            .collect();
        Frontmatter::from_items_with_nested(items, nested)
    }
}

impl From<FrontmatterItemV0_81_0> for FrontmatterItem {
    fn from(item: FrontmatterItemV0_81_0) -> Self {
        match item {
            FrontmatterItemV0_81_0::Field { key, value, fill } => FrontmatterItem::Field {
                key,
                value: QuillValue::from_json(value),
                fill,
            },
            FrontmatterItemV0_81_0::Comment { text, inline } => {
                FrontmatterItem::Comment { text, inline }
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
---
QUILL: usaf_memo@0.1
# a top-level comment
memo_for:
  - ORG/SYMBOL # inline comment inside a sequence
date: 2504-10-05
subject: !fill Subject of the Memorandum
---

The body of the memorandum.

```card indorsement
for: ORG/SYMBOL
from: ORG/SYMBOL
```

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

    #[test]
    fn serialized_form_carries_versioned_schema_tag() {
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
    fn rejects_invalid_quill_reference() {
        let stored = StoredDocument::V0_81_0(DocumentV0_81_0 {
            main: CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "not a valid ref!!".to_string(),
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
        assert!(err.to_string().contains("main card must carry a QUILL sentinel"));
    }

    #[test]
    fn rejects_composable_card_with_main_sentinel() {
        let stored = StoredDocument::V0_81_0(DocumentV0_81_0 {
            main: CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "usaf_memo@0.1".to_string(),
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            },
            cards: vec![CardV0_81_0 {
                sentinel: SentinelV0_81_0::Main {
                    quill: "usaf_memo@0.1".to_string(),
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
                },
                frontmatter: FrontmatterV0_81_0::default(),
                body: String::new(),
            },
            cards: vec![CardV0_81_0 {
                sentinel: SentinelV0_81_0::Card {
                    tag: "Bad Tag".to_string(),
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
