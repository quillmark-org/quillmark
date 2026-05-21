//! Versioned, storage-stable serialization for [`Document`].
//!
//! [`Document`] and its component types (`Card`, `Payload`, …) track the
//! evolving Quillmark model; their in-memory layout is an internal detail
//! and is deliberately *not* serialized directly. To persist a document —
//! e.g. in a database — it is converted to a [`StoredDocument`]: a versioned
//! envelope whose wire format is frozen per schema version.
//!
//! `Document` itself serializes through this envelope via
//! `#[serde(into / try_from)]`, so the ordinary serde entry points produce
//! and consume the versioned form transparently.
//!
//! ## Schema versions
//!
//! - **`quillmark/document@0.82.0`** — current. Encodes the unified
//!   [`Payload`] item list (typed `$` entries, user fields, and comments
//!   interleaved in source order). This is the format newly serialized
//!   documents use.
//! - **`quillmark/document@0.81.0`** — legacy. Encodes the pre-unification
//!   shape with a separate `sentinel` (the typed `$quill` / `$kind`) and a
//!   `frontmatter` item list (user fields + comments only). Kept read-only
//!   so documents written by `0.81.x` consumers still load; on
//!   reconstruction it is migrated to the V0_82_0 in-memory shape.
//!
//! The canonical design — including the step-by-step procedure for adding
//! a schema version — is `prose/canon/DOCUMENT_STORAGE.md`.

// Storage DTO types are named after the crate version that fixed their shape
// (e.g. `DocumentV0_81_0`); the underscores are intentional.
#![allow(non_camel_case_types)]

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::meta::validate_composable_kind;
use super::payload::{Payload, PayloadItem};
use super::prescan::{CommentPathSegment, NestedComment};
use super::{Card, Document};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Schema version for the V0_81_0 wire format. Documents written by
/// `quillmark-core` `0.81.x` carry this tag and are migrated forward on
/// read.
pub const SCHEMA_V0_81_0: &str = "quillmark/document@0.81.0";

/// Schema version for the V0_82_0 wire format. Newly serialized documents
/// carry this tag.
pub const SCHEMA_V0_82_0: &str = "quillmark/document@0.82.0";

/// Read the `schema` field from a raw storage DTO payload without
/// performing full deserialization.
///
/// Returns `None` if `json` is not valid JSON, is not an object, or has no
/// `schema` field. The returned string is **not** validated against the
/// set of supported schema versions — callers use this to distinguish
/// "unknown future version" from "corrupt payload" when [`Document`]
/// deserialization fails.
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
    /// Current (V0_82_0) document model — unified payload items.
    #[serde(rename = "quillmark/document@0.82.0")]
    V0_82_0(DocumentV0_82_0),
    /// Legacy (V0_81_0) document model — separate sentinel + frontmatter.
    /// Read-only; migrated on reconstruction.
    #[serde(rename = "quillmark/document@0.81.0")]
    V0_81_0(DocumentV0_81_0),
}

/// Failure while reconstructing a [`Document`] from a [`StoredDocument`].
///
/// The taxonomy is intentionally minimal: only [`Self::InvalidQuillReference`]
/// is typed, because that is the one error a non-malicious caller hits at
/// the document/quill boundary. Every other defect — wrong-role card,
/// invalid kind, reserved field name, duplicate key, too many fields —
/// can only arise from a hand-crafted storage DTO (the markdown parser
/// already rejects them) and is reported through [`Self::Malformed`] with
/// a descriptive message.
#[derive(Debug, Clone, PartialEq)]
pub enum StorageError {
    /// A stored quill reference string could not be parsed.
    InvalidQuillReference {
        /// The offending string.
        value: String,
        /// Parser explanation.
        reason: String,
    },
    /// The stored document is structurally malformed in a way the markdown
    /// parser would reject. The message describes the specific defect.
    Malformed(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::InvalidQuillReference { value, reason } => {
                write!(f, "invalid quill reference {value:?}: {reason}")
            }
            StorageError::Malformed(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for StorageError {}

// ─── V0_82_0 wire format (current) ────────────────────────────────────────────

/// Frozen `0.82.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_82_0 {
    pub main: CardV0_82_0,
    #[serde(default)]
    pub cards: Vec<CardV0_82_0>,
}

/// Frozen `0.82.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_82_0 {
    pub payload: PayloadV0_82_0,
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.82.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PayloadV0_82_0 {
    #[serde(default)]
    pub items: Vec<PayloadItemV0_82_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_82_0>,
}

/// Frozen `0.82.0` representation of a unified payload item.
///
/// Discriminator field is `type` to keep it unambiguous next to the `$kind`
/// metadata semantic (a `kind` discriminator would yield `{"kind":"kind"}`
/// for `$kind` entries).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemV0_82_0 {
    /// `$quill` system metadata — the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
    /// `$id` system metadata.
    Id { value: String },
    /// A user-defined field.
    Field {
        key: String,
        value: serde_json::Value,
        #[serde(default)]
        fill: bool,
    },
    /// A YAML comment.
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.82.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_82_0 {
    pub container_path: Vec<CommentPathSegmentV0_82_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.82.0` representation of a [`CommentPathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_82_0 {
    Key(String),
    Index(usize),
}

// ─── Document ↔ V0_82_0 (live conversion) ─────────────────────────────────────

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V0_82_0(DocumentV0_82_0::from(&doc))
    }
}

impl From<&Document> for DocumentV0_82_0 {
    fn from(doc: &Document) -> Self {
        DocumentV0_82_0 {
            main: CardV0_82_0::from(doc.main()),
            cards: doc.cards().iter().map(CardV0_82_0::from).collect(),
        }
    }
}

impl From<&Card> for CardV0_82_0 {
    fn from(card: &Card) -> Self {
        CardV0_82_0 {
            payload: PayloadV0_82_0::from(card.payload()),
            body: card.body().to_string(),
        }
    }
}

impl From<&Payload> for PayloadV0_82_0 {
    fn from(payload: &Payload) -> Self {
        PayloadV0_82_0 {
            items: payload
                .items()
                .iter()
                .map(PayloadItemV0_82_0::from)
                .collect(),
            nested_comments: payload
                .nested_comments()
                .iter()
                .map(NestedCommentV0_82_0::from)
                .collect(),
        }
    }
}

impl From<&PayloadItem> for PayloadItemV0_82_0 {
    fn from(item: &PayloadItem) -> Self {
        match item {
            PayloadItem::Quill { reference } => PayloadItemV0_82_0::Quill {
                value: reference.to_string(),
            },
            PayloadItem::Kind { value } => PayloadItemV0_82_0::Kind {
                value: value.clone(),
            },
            PayloadItem::Id { value } => PayloadItemV0_82_0::Id {
                value: value.clone(),
            },
            PayloadItem::Field { key, value, fill } => PayloadItemV0_82_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
                fill: *fill,
            },
            PayloadItem::Comment { text, inline } => PayloadItemV0_82_0::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV0_82_0 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV0_82_0 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV0_82_0::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&CommentPathSegment> for CommentPathSegmentV0_82_0 {
    fn from(seg: &CommentPathSegment) -> Self {
        match seg {
            CommentPathSegment::Key(k) => CommentPathSegmentV0_82_0::Key(k.clone()),
            CommentPathSegment::Index(i) => CommentPathSegmentV0_82_0::Index(*i),
        }
    }
}

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        match stored {
            StoredDocument::V0_82_0(payload) => Document::try_from(payload),
            StoredDocument::V0_81_0(payload) => Document::try_from(DocumentV0_82_0::from(payload)),
        }
    }
}

impl TryFrom<DocumentV0_82_0> for Document {
    type Error = StorageError;

    fn try_from(payload: DocumentV0_82_0) -> Result<Self, Self::Error> {
        let main = Card::try_from(payload.main)?;
        if main.quill().is_none() {
            return Err(StorageError::Malformed(
                "main card must carry a $quill entry".into(),
            ));
        }
        let cards = payload
            .cards
            .into_iter()
            .map(Card::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        for card in &cards {
            if card.quill().is_some() {
                return Err(StorageError::Malformed(
                    "composable cards must not carry a $quill entry".into(),
                ));
            }
            if let Some(kind) = card.kind() {
                match validate_composable_kind(kind) {
                    Ok(()) => {}
                    Err(super::meta::CardKindError::InvalidName) => {
                        return Err(StorageError::Malformed(format!(
                            "invalid composable card kind {kind:?}: must match \
                             [a-z_][a-z0-9_]*"
                        )));
                    }
                    Err(super::meta::CardKindError::Reserved) => {
                        return Err(StorageError::Malformed(format!(
                            "composable card kind {kind:?} is reserved (root only)"
                        )));
                    }
                }
            }
        }
        Ok(Document::from_main_and_cards(main, cards, Vec::new()))
    }
}

impl TryFrom<CardV0_82_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_82_0) -> Result<Self, Self::Error> {
        let payload = Payload::try_from(card.payload)?;
        validate_dto_payload(&payload)?;
        Ok(Card::from_parts(payload, card.body))
    }
}

impl TryFrom<PayloadV0_82_0> for Payload {
    type Error = StorageError;

    fn try_from(p: PayloadV0_82_0) -> Result<Self, Self::Error> {
        let mut items = Vec::with_capacity(p.items.len());
        for item in p.items {
            items.push(PayloadItem::try_from(item)?);
        }
        let nested = p
            .nested_comments
            .into_iter()
            .map(NestedComment::from)
            .collect();
        Ok(Payload::from_items_with_nested(items, nested))
    }
}

impl TryFrom<PayloadItemV0_82_0> for PayloadItem {
    type Error = StorageError;

    fn try_from(item: PayloadItemV0_82_0) -> Result<Self, Self::Error> {
        Ok(match item {
            PayloadItemV0_82_0::Quill { value } => {
                let reference = QuillReference::from_str(&value).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: value.clone(),
                        reason,
                    }
                })?;
                PayloadItem::Quill { reference }
            }
            PayloadItemV0_82_0::Kind { value } => PayloadItem::Kind { value },
            PayloadItemV0_82_0::Id { value } => PayloadItem::Id { value },
            PayloadItemV0_82_0::Field { key, value, fill } => PayloadItem::Field {
                key,
                value: QuillValue::from_json(value),
                fill,
            },
            PayloadItemV0_82_0::Comment { text, inline } => PayloadItem::Comment { text, inline },
        })
    }
}

impl From<NestedCommentV0_82_0> for NestedComment {
    fn from(nc: NestedCommentV0_82_0) -> Self {
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

impl From<CommentPathSegmentV0_82_0> for CommentPathSegment {
    fn from(seg: CommentPathSegmentV0_82_0) -> Self {
        match seg {
            CommentPathSegmentV0_82_0::Key(k) => CommentPathSegment::Key(k),
            CommentPathSegmentV0_82_0::Index(i) => CommentPathSegment::Index(i),
        }
    }
}

/// Reject a payload no markdown-parsed `Document` could produce: too many
/// fields, a reserved sentinel key, or a duplicate user-field key. The
/// markdown parser already rejects all three; this only guards hand-crafted
/// storage DTOs.
fn validate_dto_payload(payload: &Payload) -> Result<(), StorageError> {
    if payload.len() > crate::error::MAX_FIELD_COUNT {
        return Err(StorageError::Malformed(format!(
            "card has {} user fields, exceeding the maximum of {}",
            payload.len(),
            crate::error::MAX_FIELD_COUNT
        )));
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for key in payload.keys() {
        if super::edit::is_reserved_name(key) {
            return Err(StorageError::Malformed(format!(
                "reserved name {key:?} cannot be used as a field name"
            )));
        }
        if !seen.insert(key.as_str()) {
            return Err(StorageError::Malformed(format!(
                "duplicate user-field key {key:?}"
            )));
        }
    }
    Ok(())
}

// ─── V0_81_0 wire format (legacy, read-only) ──────────────────────────────────

/// Frozen `0.81.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_81_0 {
    pub main: CardV0_81_0,
    #[serde(default)]
    pub cards: Vec<CardV0_81_0>,
}

/// Frozen `0.81.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_81_0 {
    pub sentinel: SentinelV0_81_0,
    #[serde(default)]
    pub frontmatter: FrontmatterV0_81_0,
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.81.0` representation of a card discriminator (sentinel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SentinelV0_81_0 {
    Main { quill: String },
    Card { tag: String },
}

/// Frozen `0.81.0` representation of a card payload (user fields only).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrontmatterV0_81_0 {
    #[serde(default)]
    pub items: Vec<FrontmatterItemV0_81_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_81_0>,
}

/// Frozen `0.81.0` representation of a payload item (no `$` entries).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FrontmatterItemV0_81_0 {
    Field {
        key: String,
        value: serde_json::Value,
        #[serde(default)]
        fill: bool,
    },
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.81.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_81_0 {
    pub container_path: Vec<CommentPathSegmentV0_81_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.81.0` representation of a [`CommentPathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_81_0 {
    Key(String),
    Index(usize),
}

// ─── V0_81_0 → V0_82_0 migration ──────────────────────────────────────────────
//
// The migration is purely structural — it converts the old separate
// `sentinel + frontmatter` shape into a unified items list, then defers to
// the V0_82_0 → Document path for typed validation. Quill-reference
// validity is checked once, on the V0_82_0 side.

impl From<DocumentV0_81_0> for DocumentV0_82_0 {
    fn from(d: DocumentV0_81_0) -> Self {
        DocumentV0_82_0 {
            main: CardV0_82_0::from(d.main),
            cards: d.cards.into_iter().map(CardV0_82_0::from).collect(),
        }
    }
}

impl From<CardV0_81_0> for CardV0_82_0 {
    fn from(c: CardV0_81_0) -> Self {
        let mut items: Vec<PayloadItemV0_82_0> = Vec::new();

        // The sentinel migrates to a prelude of typed `$` entries. The
        // `Main` variant implies `$kind: main` (spec §3.3); the
        // reconstructed model carries the canonical kind so the markdown
        // emit produces a parseable document.
        match c.sentinel {
            SentinelV0_81_0::Main { quill } => {
                items.push(PayloadItemV0_82_0::Quill { value: quill });
                items.push(PayloadItemV0_82_0::Kind {
                    value: "main".into(),
                });
            }
            SentinelV0_81_0::Card { tag } => {
                items.push(PayloadItemV0_82_0::Kind { value: tag });
            }
        }

        // Append user fields and comments in their original order. V0_81_0
        // didn't track `$`-line comments separately, so the comment
        // positions migrate as-is (after the `$` prelude).
        for item in c.frontmatter.items {
            items.push(match item {
                FrontmatterItemV0_81_0::Field { key, value, fill } => {
                    PayloadItemV0_82_0::Field { key, value, fill }
                }
                FrontmatterItemV0_81_0::Comment { text, inline } => {
                    PayloadItemV0_82_0::Comment { text, inline }
                }
            });
        }

        let nested_comments = c
            .frontmatter
            .nested_comments
            .into_iter()
            .map(NestedCommentV0_82_0::from)
            .collect();

        CardV0_82_0 {
            payload: PayloadV0_82_0 {
                items,
                nested_comments,
            },
            body: c.body,
        }
    }
}

impl From<NestedCommentV0_81_0> for NestedCommentV0_82_0 {
    fn from(nc: NestedCommentV0_81_0) -> Self {
        NestedCommentV0_82_0 {
            container_path: nc
                .container_path
                .into_iter()
                .map(CommentPathSegmentV0_82_0::from)
                .collect(),
            position: nc.position,
            text: nc.text,
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_81_0> for CommentPathSegmentV0_82_0 {
    fn from(seg: CommentPathSegmentV0_81_0) -> Self {
        match seg {
            CommentPathSegmentV0_81_0::Key(k) => CommentPathSegmentV0_82_0::Key(k),
            CommentPathSegmentV0_81_0::Index(i) => CommentPathSegmentV0_82_0::Index(i),
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
$quill: usaf_memo@0.1
$kind: main
# a top-level comment
memo_for:
  - ORG/SYMBOL # inline comment inside a sequence
date: 2504-10-05
subject: !fill Subject of the Memorandum
~~~

The body of the memorandum.

~~~card-yaml
$kind: indorsement
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

    #[test]
    fn serialization_uses_v0_82_0_schema() {
        let doc = sample();
        let value: serde_json::Value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["schema"], SCHEMA_V0_82_0);
    }

    #[test]
    fn root_kind_is_main_through_round_trip() {
        let doc = Document::from_markdown(
            "~~~card-yaml\n$quill: usaf_memo@0.1\n$kind: main\ntitle: \"Hi\"\n~~~\n",
        )
        .unwrap();
        assert_eq!(doc.main().kind(), Some("main"));
        let restored: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, restored);
        assert_eq!(restored.main().kind(), Some("main"));
    }

    #[test]
    fn serialization_is_byte_deterministic() {
        // Re-serialization stability, round-trip stability, and
        // path-independence — checked together because consumers
        // content-hash the result.
        let doc = sample();
        let first = serde_json::to_string(&doc).unwrap();
        let second = serde_json::to_string(&doc).unwrap();
        assert_eq!(first, second, "to_string must be deterministic");
        let restored: Document = serde_json::from_str(&first).unwrap();
        let third = serde_json::to_string(&restored).unwrap();
        assert_eq!(first, third, "byte-equality must survive a round-trip");
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
        assert_eq!(peek_schema_version(&json).as_deref(), Some(SCHEMA_V0_82_0));

        // Unknown future version: peek still succeeds.
        let future = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        assert_eq!(
            peek_schema_version(future).as_deref(),
            Some("quillmark/document@0.99.0")
        );
        assert_eq!(peek_schema_version("not json"), None);
        assert_eq!(peek_schema_version(r#"{"foo":"bar"}"#), None);
    }

    #[test]
    fn comment_on_dollar_line_round_trips() {
        // The headline case the unification enables: a `$kind` line with an
        // inline trailing comment survives a JSON round-trip.
        let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main # required for root
title: Hi
~~~
";
        let doc = Document::from_markdown(src).unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, restored);
        // And the emitted markdown carries the comment back on the `$kind` line.
        assert!(restored
            .to_markdown()
            .contains("$kind: main # required for root"));
    }

    #[test]
    fn v0_81_0_payload_loads_via_migration() {
        let json = r#"{
            "schema": "quillmark/document@0.81.0",
            "main": {
                "sentinel": {"kind": "main", "quill": "usaf_memo@0.1"},
                "frontmatter": {
                    "items": [{"kind": "field", "key": "title", "value": "Hello"}]
                },
                "body": "Body."
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.main().kind(), Some("main"));
        assert_eq!(doc.quill_reference().to_string(), "usaf_memo@0.1");
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str(),
            Some("Hello")
        );
    }

    #[test]
    fn v0_81_0_with_composable_card_migrates() {
        let json = r#"{
            "schema": "quillmark/document@0.81.0",
            "main": {
                "sentinel": {"kind": "main", "quill": "q@1.0"},
                "frontmatter": {"items": []},
                "body": ""
            },
            "cards": [
                {
                    "sentinel": {"kind": "card", "tag": "indorsement"},
                    "frontmatter": {"items": [{"kind": "field", "key": "for", "value": "X"}]},
                    "body": "C body"
                }
            ]
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.cards().len(), 1);
        assert_eq!(doc.cards()[0].kind(), Some("indorsement"));
        assert_eq!(
            doc.cards()[0].payload().get("for").unwrap().as_str(),
            Some("X")
        );
    }

    #[test]
    fn rejects_main_card_without_quill() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": ""},
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("$quill"));
    }

    #[test]
    fn rejects_composable_card_tagged_main() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": ""
            },
            "cards": [
                {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": ""}
            ]
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("reserved (root only)"));
    }

    #[test]
    fn rejects_invalid_quill_reference() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "not a valid ref!!"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("invalid quill reference"));
    }

    #[test]
    fn rejects_reserved_field_name() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "field", "key": "BODY", "value": "x"}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("reserved name"));
    }
}
