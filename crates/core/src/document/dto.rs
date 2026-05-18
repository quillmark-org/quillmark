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
//! ## Evolving the format
//!
//! When the internal model changes, the `From`/`TryFrom` conversions in this
//! module are updated — the `V1` types stay frozen. A breaking change adds a
//! new `StoredDocument` variant (`V2(DocumentV2)`) and a migration; rows
//! written under older versions keep deserializing because their variant is
//! still present.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::frontmatter::{Frontmatter, FrontmatterItem};
use super::prescan::{CommentPathSegment, NestedComment};
use super::{Card, Document, Sentinel};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Versioned envelope for a persisted [`Document`].
///
/// The `schema` field selects the payload version. Deserialization
/// dispatches on it; unknown values are rejected. New schema versions are
/// added as new variants, leaving existing ones byte-stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema")]
pub enum StoredDocument {
    /// Version 1 payload.
    #[serde(rename = "quillmark/document@v1")]
    V1(DocumentV1),
}

/// Frozen v1 representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV1 {
    /// The document entry card.
    pub main: CardV1,
    /// Composable cards, in order.
    #[serde(default)]
    pub cards: Vec<CardV1>,
}

/// Frozen v1 representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV1 {
    /// Card discriminator.
    pub sentinel: SentinelV1,
    /// Ordered frontmatter.
    #[serde(default)]
    pub frontmatter: FrontmatterV1,
    /// Markdown body following the card fence.
    #[serde(default)]
    pub body: String,
}

/// Frozen v1 representation of a [`Sentinel`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SentinelV1 {
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

/// Frozen v1 representation of a [`Frontmatter`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrontmatterV1 {
    /// Ordered fields and top-level comments.
    #[serde(default)]
    pub items: Vec<FrontmatterItemV1>,
    /// Comments captured inside nested mappings/sequences.
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV1>,
}

/// Frozen v1 representation of a [`FrontmatterItem`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FrontmatterItemV1 {
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

/// Frozen v1 representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV1 {
    /// Path to the immediate parent container.
    pub container_path: Vec<CommentPathSegmentV1>,
    /// Slot or host index (see [`NestedComment`]).
    pub position: usize,
    /// Comment text.
    pub text: String,
    /// `true` for trailing inline comments.
    pub inline: bool,
}

/// Frozen v1 representation of a [`CommentPathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV1 {
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
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::InvalidQuillReference { value, reason } => {
                write!(f, "invalid quill reference {value:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for StorageError {}

// ── Document → StoredDocument (infallible) ────────────────────────────────────

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V1(DocumentV1::from(&doc))
    }
}

impl From<&Document> for DocumentV1 {
    fn from(doc: &Document) -> Self {
        DocumentV1 {
            main: CardV1::from(doc.main()),
            cards: doc.cards().iter().map(CardV1::from).collect(),
        }
    }
}

impl From<&Card> for CardV1 {
    fn from(card: &Card) -> Self {
        CardV1 {
            sentinel: SentinelV1::from(card.sentinel()),
            frontmatter: FrontmatterV1::from(card.frontmatter()),
            body: card.body().to_string(),
        }
    }
}

impl From<&Sentinel> for SentinelV1 {
    fn from(sentinel: &Sentinel) -> Self {
        match sentinel {
            Sentinel::Main(reference) => SentinelV1::Main {
                quill: reference.to_string(),
            },
            Sentinel::Card(tag) => SentinelV1::Card { tag: tag.clone() },
        }
    }
}

impl From<&Frontmatter> for FrontmatterV1 {
    fn from(fm: &Frontmatter) -> Self {
        FrontmatterV1 {
            items: fm.items().iter().map(FrontmatterItemV1::from).collect(),
            nested_comments: fm
                .nested_comments()
                .iter()
                .map(NestedCommentV1::from)
                .collect(),
        }
    }
}

impl From<&FrontmatterItem> for FrontmatterItemV1 {
    fn from(item: &FrontmatterItem) -> Self {
        match item {
            FrontmatterItem::Field { key, value, fill } => FrontmatterItemV1::Field {
                key: key.clone(),
                value: value.as_json().clone(),
                fill: *fill,
            },
            FrontmatterItem::Comment { text, inline } => FrontmatterItemV1::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV1 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV1 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV1::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&CommentPathSegment> for CommentPathSegmentV1 {
    fn from(seg: &CommentPathSegment) -> Self {
        match seg {
            CommentPathSegment::Key(k) => CommentPathSegmentV1::Key(k.clone()),
            CommentPathSegment::Index(i) => CommentPathSegmentV1::Index(*i),
        }
    }
}

// ── StoredDocument → Document (fallible) ──────────────────────────────────────

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        match stored {
            StoredDocument::V1(v1) => Document::try_from(v1),
        }
    }
}

impl TryFrom<DocumentV1> for Document {
    type Error = StorageError;

    fn try_from(v1: DocumentV1) -> Result<Self, Self::Error> {
        let main = Card::try_from(v1.main)?;
        let cards = v1
            .cards
            .into_iter()
            .map(Card::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Document::from_main_and_cards(main, cards, Vec::new()))
    }
}

impl TryFrom<CardV1> for Card {
    type Error = StorageError;

    fn try_from(card: CardV1) -> Result<Self, Self::Error> {
        let sentinel = Sentinel::try_from(card.sentinel)?;
        let frontmatter = Frontmatter::from(card.frontmatter);
        Ok(Card::new_with_sentinel(sentinel, frontmatter, card.body))
    }
}

impl TryFrom<SentinelV1> for Sentinel {
    type Error = StorageError;

    fn try_from(sentinel: SentinelV1) -> Result<Self, Self::Error> {
        match sentinel {
            SentinelV1::Main { quill } => {
                let reference = QuillReference::from_str(&quill).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: quill.clone(),
                        reason,
                    }
                })?;
                Ok(Sentinel::Main(reference))
            }
            SentinelV1::Card { tag } => Ok(Sentinel::Card(tag)),
        }
    }
}

impl From<FrontmatterV1> for Frontmatter {
    fn from(fm: FrontmatterV1) -> Self {
        let items = fm.items.into_iter().map(FrontmatterItem::from).collect();
        let nested = fm
            .nested_comments
            .into_iter()
            .map(NestedComment::from)
            .collect();
        Frontmatter::from_items_with_nested(items, nested)
    }
}

impl From<FrontmatterItemV1> for FrontmatterItem {
    fn from(item: FrontmatterItemV1) -> Self {
        match item {
            FrontmatterItemV1::Field { key, value, fill } => FrontmatterItem::Field {
                key,
                value: QuillValue::from_json(value),
                fill,
            },
            FrontmatterItemV1::Comment { text, inline } => {
                FrontmatterItem::Comment { text, inline }
            }
        }
    }
}

impl From<NestedCommentV1> for NestedComment {
    fn from(nc: NestedCommentV1) -> Self {
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

impl From<CommentPathSegmentV1> for CommentPathSegment {
    fn from(seg: CommentPathSegmentV1) -> Self {
        match seg {
            CommentPathSegmentV1::Key(k) => CommentPathSegment::Key(k),
            CommentPathSegmentV1::Index(i) => CommentPathSegment::Index(i),
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
    fn serialized_form_carries_schema_tag() {
        let doc = sample();
        let value: serde_json::Value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["schema"], "quillmark/document@v1");
        assert!(value.get("warnings").is_none());
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let json = r#"{"schema":"quillmark/document@v999","main":{}}"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }

    #[test]
    fn rejects_invalid_quill_reference() {
        let stored = StoredDocument::V1(DocumentV1 {
            main: CardV1 {
                sentinel: SentinelV1::Main {
                    quill: "not a valid ref!!".to_string(),
                },
                frontmatter: FrontmatterV1::default(),
                body: String::new(),
            },
            cards: Vec::new(),
        });
        let err = Document::try_from(stored).unwrap_err();
        assert!(matches!(err, StorageError::InvalidQuillReference { .. }));
    }

    #[test]
    fn explicit_dto_conversion_round_trips() {
        let doc = sample();
        let dto = DocumentV1::from(&doc);
        let restored = Document::try_from(dto).unwrap();
        assert_eq!(doc, restored);
    }
}
