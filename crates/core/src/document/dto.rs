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
//! - **`quillmark/document@0.93.0`** — current. The V0_92_0 payload model with
//!   the card `body` stored as the **canonical richtext corpus** embedded
//!   structurally (a nested object byte-identical to `to_canonical_json`), not a
//!   markdown string. The envelope carries two byte disciplines: the outer
//!   structure stays compact `serde_json` in frozen struct + payload-insertion
//!   order (`preserve_order`), while every `body` subtree is the recursively
//!   key-sorted canonical form. This is the format newly serialized documents
//!   use.
//! - **`quillmark/document@0.92.0`** — legacy. The V0_82_0 model plus a
//!   per-field `nested_fills` list (so `!must_fill` markers nested inside a
//!   field value survive a storage round-trip) and the `$seed` payload-item
//!   variant (per-card-kind seed overlays), with the body as a markdown string.
//!   Kept read-only; the body cold-imports to a corpus and it migrates forward
//!   to V0_93_0 on read.
//! - **`quillmark/document@0.82.0`** — legacy. Encodes the unified
//!   [`Payload`] item list (typed `$` entries, user fields, and comments
//!   interleaved in source order) but carries top-level fill only and no
//!   `$seed`. Kept read-only; migrated forward to V0_93_0 on read.
//! - **`quillmark/document@0.81.0`** — legacy. Encodes the pre-unification
//!   shape with a separate `sentinel` (the typed `$quill` / `$kind`) and a
//!   `frontmatter` item list (user fields + comments only). Kept read-only
//!   so documents written by `0.81.x` consumers still load; on
//!   reconstruction it is migrated forward (V0_81_0 → V0_82_0 → V0_92_0 →
//!   V0_93_0).
//!
//! The canonical design — including the step-by-step procedure for adding
//! a schema version — is `prose/canon/DOCUMENT_STORAGE.md`.

// Storage DTO types are named after the crate version that fixed their shape
// (e.g. `DocumentV0_81_0`); the underscores are intentional.
#![allow(non_camel_case_types)]

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use quillmark_richtext::RichText;

use super::meta::validate_composable_kind;
use super::payload::{MetaKey, Payload, PayloadItem};
use super::prescan::{CommentPathSegment, NestedComment};
use super::{Card, Document};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Schema version for the V0_93_0 wire format. Newly serialized documents carry
/// this tag. Stores the card `body` as the canonical richtext corpus embedded
/// structurally (byte-identical to `to_canonical_json`) rather than a markdown string;
/// the payload shape is unchanged from V0_92_0.
pub const SCHEMA_V0_93_0: &str = "quillmark/document@0.93.0";

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
    /// Current (V0_93_0) document model — the V0_92_0 payload with the card
    /// `body` embedded as the canonical richtext corpus (a nested object).
    #[serde(rename = "quillmark/document@0.93.0")]
    V0_93_0(DocumentV0_93_0),
    /// Legacy (V0_92_0) document model — unified payload items with per-field
    /// nested fill paths and `$seed`, body as a markdown string. Read-only;
    /// migrated forward to V0_93_0 on reconstruction.
    #[serde(rename = "quillmark/document@0.92.0")]
    V0_92_0(DocumentV0_92_0),
    /// Legacy (V0_82_0) document model — unified payload items, top-level
    /// fill only and no `$seed`. Read-only; migrated on reconstruction.
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
/// invalid kind, duplicate key, too many fields — can only arise from a
/// hand-crafted storage DTO (the markdown parser already rejects them)
/// and is reported through [`Self::Malformed`] with a descriptive message.
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

// ─── V0_93_0 wire format (current) ────────────────────────────────────────────

/// Frozen `0.93.0` representation of a [`Document`]. Mirrors `DocumentV0_92_0`;
/// the only structural change is `Card.body` (see [`CardV0_93_0`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_93_0 {
    pub main: CardV0_93_0,
    #[serde(default)]
    pub cards: Vec<CardV0_93_0>,
}

/// Frozen `0.93.0` representation of a [`Card`]. The `body` is the canonical
/// richtext corpus embedded structurally (see [`CanonicalRichText`]); the
/// payload is not part of this freeze and reuses the V0_92_0 shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_93_0 {
    pub payload: PayloadV0_93_0,
    pub body: CanonicalRichText,
}

/// The V0_93_0 payload shape — identical to V0_92_0. Aliased rather than copied
/// because payload is outside this freeze; a future payload change forks it.
pub type PayloadV0_93_0 = PayloadV0_92_0;

/// A card body embedded as the **canonical richtext corpus**. Its serde *is* the
/// frozen canonical serializer (`quillmark_richtext::serial`), delegated to — not
/// a hand-mirrored DTO tree that could drift from the frozen wire format:
///
/// - `Serialize` emits the recursively key-sorted structure byte-identical to
///   `self.0.to_canonical_json()` as a **nested JSON object**, never an escaped
///   string. Embedded in the compact envelope, the `body` subtree bytes equal
///   that canonical JSON, independent of `preserve_order`.
/// - `Deserialize` parses that structure, normalizes, and validates, so an
///   invalid corpus is rejected at load (a serde error) rather than silently
///   round-tripped.
///
/// Byte-equality with `to_canonical_json` holds because every `RichText` in a live
/// [`Document`] is normalized at construction; the serializer normalizes a copy
/// regardless, so a hand-built value cannot leak non-canonical bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalRichText(pub RichText);

impl Serialize for CanonicalRichText {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        quillmark_richtext::serial::to_canonical_value(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CanonicalRichText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let rt = quillmark_richtext::serial::from_canonical_value(&value)
            .map_err(serde::de::Error::custom)?;
        Ok(CanonicalRichText(rt))
    }
}

// ─── V0_82_0 wire format (legacy; read + migrate forward only) ────────────────

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
    /// `$ext` system metadata — an opaque mapping carrying out-of-band
    /// extension data (UI editor state, agent annotations, …). Never
    /// emitted into the plate JSON; round-trips through the DTO unchanged.
    Ext {
        value: serde_json::Map<String, serde_json::Value>,
    },
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

// ─── V0_92_0 wire format (current) ────────────────────────────────────────────

/// Frozen `0.92.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_92_0 {
    pub main: CardV0_92_0,
    #[serde(default)]
    pub cards: Vec<CardV0_92_0>,
}

/// Frozen `0.92.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_92_0 {
    pub payload: PayloadV0_92_0,
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.92.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PayloadV0_92_0 {
    #[serde(default)]
    pub items: Vec<PayloadItemV0_92_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_92_0>,
}

/// Frozen `0.92.0` representation of a unified payload item. Extends
/// `V0_82_0` with the `Seed` variant and a per-`Field` `nested_fills` list:
/// the paths of `!must_fill` markers nested inside the field value (the JSON
/// `value` is fill-free).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemV0_92_0 {
    /// `$quill` system metadata — the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
    /// `$id` system metadata.
    Id { value: String },
    /// `$ext` system metadata — an opaque mapping carrying out-of-band
    /// extension data. Never emitted into the plate JSON.
    Ext {
        value: serde_json::Map<String, serde_json::Value>,
    },
    /// `$seed` system metadata — a mapping keyed by card-kind carrying the
    /// per-kind seed overlays. Never emitted into the plate JSON.
    Seed {
        value: serde_json::Map<String, serde_json::Value>,
    },
    /// A user-defined field.
    Field {
        key: String,
        value: serde_json::Value,
        #[serde(default)]
        fill: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nested_fills: Vec<Vec<CommentPathSegmentV0_92_0>>,
    },
    /// A YAML comment.
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.92.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_92_0 {
    pub container_path: Vec<CommentPathSegmentV0_92_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.92.0` representation of a [`CommentPathSegment`]. Also used for
/// `nested_fills` path segments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_92_0 {
    Key(String),
    Index(usize),
}

// ─── Document → V0_93_0 (write) ───────────────────────────────────────────────
//
// The write path targets the newest version only. Payload conversion still
// runs through the V0_92_0 `PayloadItem` DTOs (`PayloadV0_93_0` aliases them);
// the body is embedded as the canonical corpus.

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V0_93_0(DocumentV0_93_0::from(&doc))
    }
}

impl From<&Document> for DocumentV0_93_0 {
    fn from(doc: &Document) -> Self {
        DocumentV0_93_0 {
            main: CardV0_93_0::from(doc.main()),
            cards: doc.cards().iter().map(CardV0_93_0::from).collect(),
        }
    }
}

impl From<&Card> for CardV0_93_0 {
    fn from(card: &Card) -> Self {
        // The body is already a normalized corpus on the live model; embed it
        // directly. `CanonicalRichText`'s serializer emits the canonical form.
        CardV0_93_0 {
            payload: PayloadV0_92_0::from(card.payload()),
            body: CanonicalRichText(card.body().clone()),
        }
    }
}

impl From<&Payload> for PayloadV0_92_0 {
    fn from(payload: &Payload) -> Self {
        // The wire format keeps `nested_comments` as a flat sidecar at
        // the payload level. The in-memory model carries them per-item
        // with relative paths, so we re-prefix and flatten here.
        let nested_comments = payload
            .flat_nested_comments()
            .iter()
            .map(NestedCommentV0_92_0::from)
            .collect();
        PayloadV0_92_0 {
            items: payload
                .items()
                .iter()
                .map(PayloadItemV0_92_0::from)
                .collect(),
            nested_comments,
        }
    }
}

impl From<&PayloadItem> for PayloadItemV0_92_0 {
    fn from(item: &PayloadItem) -> Self {
        match item {
            PayloadItem::Quill { reference } => PayloadItemV0_92_0::Quill {
                value: reference.to_string(),
            },
            PayloadItem::Kind { value } => PayloadItemV0_92_0::Kind {
                value: value.clone(),
            },
            PayloadItem::Id { value } => PayloadItemV0_92_0::Id {
                value: value.clone(),
            },
            // The storage DTO keeps `$ext` / `$seed` as explicit, self-describing
            // variants; the live model's unified `Meta` is split back out by key.
            // Neither wire variant carries a `nested_comments` field — their
            // comments live in the payload-level sidecar after
            // `flat_nested_comments` re-prefixes them with `$ext` / `$seed`.
            PayloadItem::Meta {
                key: MetaKey::Ext,
                value,
                ..
            } => PayloadItemV0_92_0::Ext {
                value: value.clone(),
            },
            PayloadItem::Meta {
                key: MetaKey::Seed,
                value,
                ..
            } => PayloadItemV0_92_0::Seed {
                value: value.clone(),
            },
            // The JSON `value` projection is fill-free; nested `!must_fill`
            // markers ride alongside as `nested_fills` (root path omitted —
            // a top-level marker is the `fill` flag).
            PayloadItem::Field {
                key, value, fill, ..
            } => PayloadItemV0_92_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
                fill: *fill,
                nested_fills: value
                    .nonroot_fill_paths()
                    .map(|p| p.iter().map(CommentPathSegmentV0_92_0::from).collect())
                    .collect(),
            },
            PayloadItem::Comment { text, inline } => PayloadItemV0_92_0::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV0_92_0 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV0_92_0 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV0_92_0::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&CommentPathSegment> for CommentPathSegmentV0_92_0 {
    fn from(seg: &CommentPathSegment) -> Self {
        match seg {
            CommentPathSegment::Key(k) => CommentPathSegmentV0_92_0::Key(k.clone()),
            CommentPathSegment::Index(i) => CommentPathSegmentV0_92_0::Index(*i),
        }
    }
}

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        // Migrations chain: only the newest DTO converts to the live model;
        // older versions migrate forward (V0_81 → V0_82 → V0_92 → V0_93). The
        // V0_92 → V0_93 hop cold-imports the markdown body, so every arm below
        // the newest is fallible (`?`).
        match stored {
            StoredDocument::V0_93_0(payload) => Document::try_from(payload),
            StoredDocument::V0_92_0(payload) => {
                Document::try_from(DocumentV0_93_0::try_from(payload)?)
            }
            StoredDocument::V0_82_0(payload) => {
                Document::try_from(DocumentV0_93_0::try_from(DocumentV0_92_0::from(payload))?)
            }
            StoredDocument::V0_81_0(payload) => Document::try_from(DocumentV0_93_0::try_from(
                DocumentV0_92_0::from(DocumentV0_82_0::from(payload)),
            )?),
        }
    }
}

impl TryFrom<DocumentV0_93_0> for Document {
    type Error = StorageError;

    fn try_from(payload: DocumentV0_93_0) -> Result<Self, Self::Error> {
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
            if card.seed().is_some() {
                return Err(StorageError::Malformed(
                    "composable cards must not carry a $seed entry".into(),
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

impl TryFrom<CardV0_93_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_93_0) -> Result<Self, Self::Error> {
        let payload = Payload::try_from(card.payload)?;
        validate_dto_payload(&payload)?;
        // `body` is already a normalized, validated corpus — `CanonicalRichText`
        // enforced that on deserialize (and the V0_92 → V0_93 migration produced
        // it via cold import). Take it directly.
        Ok(Card::from_parts(payload, card.body.0))
    }
}

// ─── V0_92_0 → V0_93_0 migration (fallible cold import) ───────────────────────
//
// The one hop that can reject: the stored markdown body cold-imports to the
// corpus (`import_body`, pure/deterministic). An over-nested body
// (> MAX_NESTING_DEPTH, surfaced as `ImportError::NestingTooDeep`) never
// rendered, so mapping it to `StorageError::Malformed` loses nothing
// renderable. Cross-release byte-stability of a *migrated* row is therefore
// conditional on `pulldown-cmark` (DOCUMENT_STORAGE.md § byte stability).

impl TryFrom<DocumentV0_92_0> for DocumentV0_93_0 {
    type Error = StorageError;

    fn try_from(d: DocumentV0_92_0) -> Result<Self, Self::Error> {
        Ok(DocumentV0_93_0 {
            main: CardV0_93_0::try_from(d.main)?,
            cards: d
                .cards
                .into_iter()
                .map(CardV0_93_0::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl TryFrom<CardV0_92_0> for CardV0_93_0 {
    type Error = StorageError;

    fn try_from(card: CardV0_92_0) -> Result<Self, Self::Error> {
        let body = super::import_body(&card.body)
            .map_err(|e| StorageError::Malformed(format!("card body: {e}")))?;
        Ok(CardV0_93_0 {
            payload: card.payload,
            body: CanonicalRichText(body),
        })
    }
}

impl TryFrom<PayloadV0_92_0> for Payload {
    type Error = StorageError;

    fn try_from(p: PayloadV0_92_0) -> Result<Self, Self::Error> {
        let mut items = Vec::with_capacity(p.items.len());
        for item in p.items {
            items.push(PayloadItem::try_from(item)?);
        }
        let nested = p
            .nested_comments
            .into_iter()
            .map(NestedComment::from)
            .collect();
        // Partition the flat wire-format sidecar onto the matching
        // Field / Ext / Seed items (paths become relative to the owning value).
        Ok(Payload::from_items_with_flat_nested(items, nested))
    }
}

impl TryFrom<PayloadItemV0_92_0> for PayloadItem {
    type Error = StorageError;

    fn try_from(item: PayloadItemV0_92_0) -> Result<Self, Self::Error> {
        Ok(match item {
            PayloadItemV0_92_0::Quill { value } => {
                let reference = QuillReference::from_str(&value).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: value.clone(),
                        reason,
                    }
                })?;
                PayloadItem::Quill { reference }
            }
            PayloadItemV0_92_0::Kind { value } => PayloadItem::Kind { value },
            PayloadItemV0_92_0::Id { value } => PayloadItem::Id { value },
            PayloadItemV0_92_0::Ext { value } => PayloadItem::Meta {
                key: MetaKey::Ext,
                value: depth_check_meta_map(value, "$ext")?,
                nested_comments: Vec::new(),
            },
            PayloadItemV0_92_0::Seed { value } => PayloadItem::Meta {
                key: MetaKey::Seed,
                value: depth_check_meta_map(value, "$seed")?,
                nested_comments: Vec::new(),
            },
            PayloadItemV0_92_0::Field {
                key,
                value,
                fill,
                nested_fills,
            } => {
                use super::edit::{validate_field, FieldViolation};
                validate_field(&key, &value).map_err(|v| {
                    StorageError::Malformed(match v {
                        FieldViolation::InvalidName => {
                            format!("invalid field name {key:?}: must match [A-Za-z_][A-Za-z0-9_]*")
                        }
                        FieldViolation::TooDeep => format!(
                            "field {key:?} nests deeper than the maximum of {} levels",
                            crate::document::limits::MAX_YAML_DEPTH
                        ),
                    })
                })?;
                let mut qv = QuillValue::from_json(value);
                for path in nested_fills {
                    let segs: Vec<CommentPathSegment> =
                        path.into_iter().map(CommentPathSegment::from).collect();
                    qv.set_fill_at(&segs);
                }
                PayloadItem::Field {
                    key,
                    value: qv,
                    fill,
                    nested_comments: Vec::new(),
                }
            }
            PayloadItemV0_92_0::Comment { text, inline } => PayloadItem::Comment { text, inline },
        })
    }
}

/// Depth-bound a `$ext` / `$seed` mapping at the storage boundary; both flow
/// through the recursive emit/DTO paths and carry the §8 value-depth limit.
fn depth_check_meta_map(
    value: serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, StorageError> {
    let as_value = serde_json::Value::Object(value);
    if crate::value::json_depth_exceeds(&as_value, crate::document::limits::MAX_YAML_DEPTH) {
        return Err(StorageError::Malformed(format!(
            "{key} nests deeper than the maximum of {} levels",
            crate::document::limits::MAX_YAML_DEPTH
        )));
    }
    let serde_json::Value::Object(value) = as_value else {
        unreachable!("constructed as Object above")
    };
    Ok(value)
}

impl From<NestedCommentV0_92_0> for NestedComment {
    fn from(nc: NestedCommentV0_92_0) -> Self {
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

impl From<CommentPathSegmentV0_92_0> for CommentPathSegment {
    fn from(seg: CommentPathSegmentV0_92_0) -> Self {
        match seg {
            CommentPathSegmentV0_92_0::Key(k) => CommentPathSegment::Key(k),
            CommentPathSegmentV0_92_0::Index(i) => CommentPathSegment::Index(i),
        }
    }
}

// ─── V0_82_0 → V0_92_0 migration ──────────────────────────────────────────────
//
// Purely structural: V0_82_0 has neither `$seed` nor `Field.nested_fills`, so
// every variant maps 1:1 — the new `Seed` variant is never produced and
// `nested_fills` defaults to empty (that format never carried nested markers).

impl From<DocumentV0_82_0> for DocumentV0_92_0 {
    fn from(d: DocumentV0_82_0) -> Self {
        DocumentV0_92_0 {
            main: CardV0_92_0::from(d.main),
            cards: d.cards.into_iter().map(CardV0_92_0::from).collect(),
        }
    }
}

impl From<CardV0_82_0> for CardV0_92_0 {
    fn from(c: CardV0_82_0) -> Self {
        CardV0_92_0 {
            payload: PayloadV0_92_0::from(c.payload),
            body: c.body,
        }
    }
}

impl From<PayloadV0_82_0> for PayloadV0_92_0 {
    fn from(p: PayloadV0_82_0) -> Self {
        PayloadV0_92_0 {
            items: p.items.into_iter().map(PayloadItemV0_92_0::from).collect(),
            nested_comments: p
                .nested_comments
                .into_iter()
                .map(NestedCommentV0_92_0::from)
                .collect(),
        }
    }
}

impl From<PayloadItemV0_82_0> for PayloadItemV0_92_0 {
    fn from(item: PayloadItemV0_82_0) -> Self {
        match item {
            PayloadItemV0_82_0::Quill { value } => PayloadItemV0_92_0::Quill { value },
            PayloadItemV0_82_0::Kind { value } => PayloadItemV0_92_0::Kind { value },
            PayloadItemV0_82_0::Id { value } => PayloadItemV0_92_0::Id { value },
            PayloadItemV0_82_0::Ext { value } => PayloadItemV0_92_0::Ext { value },
            PayloadItemV0_82_0::Field { key, value, fill } => PayloadItemV0_92_0::Field {
                key,
                value,
                fill,
                nested_fills: Vec::new(),
            },
            PayloadItemV0_82_0::Comment { text, inline } => {
                PayloadItemV0_92_0::Comment { text, inline }
            }
        }
    }
}

impl From<NestedCommentV0_82_0> for NestedCommentV0_92_0 {
    fn from(nc: NestedCommentV0_82_0) -> Self {
        NestedCommentV0_92_0 {
            container_path: nc
                .container_path
                .into_iter()
                .map(CommentPathSegmentV0_92_0::from)
                .collect(),
            position: nc.position,
            text: nc.text,
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_82_0> for CommentPathSegmentV0_92_0 {
    fn from(seg: CommentPathSegmentV0_82_0) -> Self {
        match seg {
            CommentPathSegmentV0_82_0::Key(k) => CommentPathSegmentV0_92_0::Key(k),
            CommentPathSegmentV0_82_0::Index(i) => CommentPathSegmentV0_92_0::Index(i),
        }
    }
}

/// Reject a payload no markdown-parsed `Document` could produce: too many
/// fields or a duplicate user-field key. The markdown parser already
/// rejects both; this only guards hand-crafted storage DTOs.
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
subject: !must_fill Subject of the Memorandum
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
    fn corpus_field_survives_storage_round_trip_losslessly() {
        // A richtext field stored as a canonical corpus object is the case the
        // card-yaml markdown projection is lossy for; the storage DTO is the
        // lossless carrier, so identity marks (an `underline` with no markdown
        // form) survive a serde-JSON round-trip that a `.qmd` save would drop.
        use quillmark_richtext::model::{Mark, MarkKind};

        let mut doc = sample();
        let mut corpus = quillmark_richtext::import::from_markdown("underlined intro").unwrap();
        corpus.marks.push(Mark {
            start: 0,
            end: 10,
            kind: MarkKind::Underline,
        });
        corpus.normalize();
        let json = quillmark_richtext::serial::to_canonical_value(&corpus);
        doc.main_mut().set_field_richtext("intro", &json, false).unwrap();

        let stored = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&stored).unwrap();
        assert_eq!(doc, restored, "corpus field must survive storage round-trip");
        let read = restored.main().field_richtext("intro").unwrap().unwrap();
        assert!(
            read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)),
            "underline (corpus-only) must survive the DTO carrier"
        );
    }

    #[test]
    fn nested_fill_survives_storage_round_trip() {
        // A `!must_fill` marker on a nested object leaf rides the `nested_fills`
        // path list (the JSON `value` projection is fill-free).
        let doc = Document::from_markdown(
            "~~~card-yaml\n$quill: q@0.1\n$kind: main\naddr:\n  street: !must_fill\n  city: Anytown\n~~~\n",
        )
        .unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, restored, "nested fill must survive storage round-trip");
        assert!(
            restored.to_markdown().contains("street: !must_fill"),
            "Got:\n{}",
            restored.to_markdown()
        );
    }

    #[test]
    fn v0_82_0_payload_migrates_forward() {
        // A 0.82.0 row (no `nested_fills` on its field) loads via the
        // V0_82_0 → V0_92_0 migration, defaulting nested_fills to empty.
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {
                    "items": [
                        {"type": "quill", "value": "usaf_memo@0.1"},
                        {"type": "kind", "value": "main"},
                        {"type": "field", "key": "title", "value": "Hello", "fill": false}
                    ]
                },
                "body": "Body."
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.main().kind(), Some("main"));
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str(),
            Some("Hello")
        );
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
    fn rejects_unknown_schema_version() {
        let json = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }

    #[test]
    fn peek_schema_version_reads_field_without_full_parse() {
        let doc = sample();
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(peek_schema_version(&json).as_deref(), Some(SCHEMA_V0_93_0));

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
    fn v0_82_0_payload_loads_via_migration() {
        // A 0.82.0 blob (no `$seed`) migrates forward (0.82 → 0.92) to the live
        // model, then re-serializes under the current tag.
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "usaf_memo@0.1"},
                    {"type": "kind", "value": "main"},
                    {"type": "field", "key": "title", "value": "Hello"}
                ]},
                "body": "Body."
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.main().kind(), Some("main"));
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str(),
            Some("Hello")
        );
        let reser = serde_json::to_string(&doc).unwrap();
        assert_eq!(peek_schema_version(&reser).as_deref(), Some(SCHEMA_V0_93_0));
    }

    #[test]
    fn v0_82_0_blob_with_seed_item_is_rejected() {
        // Proves the schema bump was necessary: `{"type":"seed"}` is not a legal
        // V0_82_0 payload item, so a blob claiming the 0.82.0 tag must fail
        // rather than silently load.
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "seed", "value": {"indorsement": {"from": "X"}}}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }

    #[test]
    fn rejects_composable_card_with_seed() {
        // `$seed` is root-only (like `$quill`): a stored composable card
        // carrying it fails to load.
        let json = r#"{
            "schema": "quillmark/document@0.92.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": ""
            },
            "cards": [
                {"payload": {"items": [
                    {"type": "kind", "value": "indorsement"},
                    {"type": "seed", "value": {"note": {"from": "X"}}}
                ]}, "body": ""}
            ]
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err
            .to_string()
            .contains("composable cards must not carry a $seed entry"));
    }

    #[test]
    fn v0_92_0_seed_item_round_trips() {
        let json = r#"{
            "schema": "quillmark/document@0.92.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "seed", "value": {"indorsement": {"from": "49 FW/CC"}}}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        let overlay = doc
            .main()
            .seed()
            .and_then(|m| m.get("indorsement"))
            .and_then(crate::SeedOverlay::from_json)
            .expect("overlay present");
        assert_eq!(
            overlay.fields.get("from").and_then(|v| v.as_str()),
            Some("49 FW/CC")
        );
        let reser: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, reser);
    }

    // ─── V0_93_0 storage cutover ──────────────────────────────────────────────

    /// Slice the value of the first top-level `"body":` object out of a compact
    /// `serde_json` envelope — the exact bytes embedded, balanced-brace and
    /// string-aware. Used to prove the body subtree equals `to_canonical_json`.
    fn locate_body_subtree(envelope: &str) -> &str {
        const KEY: &str = "\"body\":";
        let start = envelope.find(KEY).expect("body key present") + KEY.len();
        let bytes = envelope.as_bytes();
        assert_eq!(
            bytes[start], b'{',
            "body must embed as a nested object, not an escaped string"
        );
        let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
        for (i, &b) in bytes[start..].iter().enumerate() {
            if in_str {
                match (escaped, b) {
                    (true, _) => escaped = false,
                    (false, b'\\') => escaped = true,
                    (false, b'"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &envelope[start..start + i + 1];
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced body object");
    }

    #[test]
    fn body_subtree_is_byte_identical_to_canonical_json() {
        // Two disciplines in one envelope: the outer structure is compact
        // insertion-ordered serde_json, but the `body` subtree is the canonical
        // richtext form, byte-identical to `rt.to_canonical_json()`.
        let doc = Document::from_markdown(
            "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: Hi\n~~~\n\n\
             A paragraph with **bold**, _emph_, and a [link](https://example.com).\n\n\
             Second paragraph continues the corpus.\n",
        )
        .unwrap();
        let rt = doc.main().body().clone();
        assert!(
            !rt.marks.is_empty(),
            "test needs a non-trivial corpus (marks present)"
        );
        let expected = rt.to_canonical_json();
        let envelope = serde_json::to_string(&doc).unwrap();
        let body = locate_body_subtree(&envelope);
        assert_eq!(
            body, expected,
            "the envelope body subtree must equal to_canonical_json byte-for-byte"
        );
        // A nested structure, not a double-encoded string.
        assert!(body.starts_with("{\"islands\":"));
    }

    #[test]
    fn v0_93_0_round_trips_as_fixed_point() {
        let doc = sample();
        let first = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&first).unwrap();
        assert_eq!(doc, restored);
        let second = serde_json::to_string(&restored).unwrap();
        assert_eq!(
            first, second,
            "V0_93_0 serialize→deserialize is a byte-fixed point"
        );
        assert_eq!(peek_schema_version(&first).as_deref(), Some(SCHEMA_V0_93_0));
    }

    #[test]
    fn legacy_table_body_migrates_deterministically_with_islands() {
        // A table-bearing 0.92.0 body cold-imports on the 92→93 hop to a corpus
        // whose island ids are sequential (`isl-0`, …). Import is a pure
        // function, so the same legacy row migrates to byte-identical storage.
        let blob = r#"{
            "schema": "quillmark/document@0.92.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@0.1"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": "| A | B |\n| - | - |\n| 1 | 2 |\n"
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(blob).unwrap();
        let body = doc.main().body();
        assert_eq!(body.islands.len(), 1, "table imports as one island");
        assert_eq!(body.islands[0].id, "isl-0", "sequential island id");
        assert_eq!(body.islands[0].island_type, "table");
        // Option A: each cell is inline `{text, marks}`, not a raw markdown slice.
        // The @0.93.0 table-body canonical bytes changed with this; the freeze is
        // branch-private/unreleased, so amending this golden pre-release is
        // expected. Regenerated golden below.
        let key = body.to_canonical_json();
        assert_eq!(
            key,
            "{\"islands\":[{\"id\":\"isl-0\",\"loss\":\"lossless\",\"props\":{\
             \"aligns\":[\"none\",\"none\"],\
             \"header\":[{\"marks\":[],\"text\":\"A\"},{\"marks\":[],\"text\":\"B\"}],\
             \"rows\":[[{\"marks\":[],\"text\":\"1\"},{\"marks\":[],\"text\":\"2\"}]]},\
             \"type\":\"table\"}],\
             \"lines\":[{\"containers\":[],\"kind\":\"island\"}],\
             \"marks\":[],\"text\":\"\u{FFFC}\"}",
            "regenerated @0.93.0 golden: cells are structured text+marks"
        );

        let again: Document = serde_json::from_str(blob).unwrap();
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            serde_json::to_string(&again).unwrap(),
            "same legacy input → same migrated bytes"
        );
        let reser = serde_json::to_string(&doc).unwrap();
        assert_eq!(peek_schema_version(&reser).as_deref(), Some(SCHEMA_V0_93_0));
    }

    #[test]
    fn over_nested_legacy_body_is_malformed() {
        // A legacy body whose container nesting exceeds MAX_NESTING_DEPTH never
        // rendered; the fallible 92→93 import hop maps `NestingTooDeep` to
        // `StorageError::Malformed` rather than silently dropping structure.
        let deep = ">".repeat(crate::error::MAX_NESTING_DEPTH + 5);
        let card = CardV0_92_0 {
            payload: PayloadV0_92_0::default(),
            body: format!("{deep} too deep"),
        };
        let err = CardV0_93_0::try_from(card).unwrap_err();
        assert!(matches!(err, StorageError::Malformed(_)), "got: {err:?}");
        assert!(err.to_string().contains("card body"));
    }

    #[test]
    fn deserialize_rejects_invalid_corpus_body() {
        // `CanonicalRichText`'s Deserialize validates: a structurally-embedded
        // body whose `lines` count disagrees with its text is rejected at load,
        // never silently round-tripped.
        let blob = r#"{
            "schema": "quillmark/document@0.93.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@0.1"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"text": "a\nb", "lines": [{"kind": "para", "containers": []}], "marks": [], "islands": []}
            },
            "cards": []
        }"#;
        assert!(serde_json::from_str::<Document>(blob).is_err());
    }
}
