//! Unified payload representation.
//!
//! A [`Payload`] is the typed representation of a card-yaml block's full
//! YAML content. It carries — in source order, as variants of a single
//! [`PayloadItem`] enum:
//!
//! - **System metadata** — typed `$quill` / `$kind` / `$id` / `$ext`
//!   entries.
//! - **User fields** — `key: value` pairs with an optional `!must_fill` flag.
//! - **Comments** — own-line or trailing inline, attached to whichever
//!   item they immediately follow at emit time.
//!
//! The unified item list is the canonical storage of the block; treating
//! `$` entries as just another variant means a comment adjacent to a `$`
//! line round-trips through the same mechanism as a comment adjacent to a
//! user field. No "metadata region" vs "payload region" routing decision is
//! ever made — there is only the source-ordered list.
//!
//! ## Comments at every level
//!
//! Top-level YAML comments (own-line and trailing inline) live as
//! `PayloadItem::Comment` entries interleaved with fields and `$` items.
//! Comments **inside** a structured value (mapping or sequence) live on
//! the [`PayloadItem::Field`] / [`PayloadItem::Meta`] that owns that
//! value, as a `nested_comments` slice with paths relative to the
//! field's value tree. One storage surface, scoped to the item that
//! "owns" each comment — no sidecar Vec hanging off `Payload`.
//!
//! ## Two faces
//!
//! [`Payload`] exposes both ordered iteration (over the raw items vec) and
//! map-keyed access (`get`, `iter`, `insert`, `remove`). The map-style
//! accessors filter to [`PayloadItem::Field`] only — they intentionally
//! don't expose `$` entries because typed `$` access has dedicated methods
//! (`quill`, `kind`, `id`, `ext`, `seed`, `set_quill`, `set_kind`, `set_id`,
//! `set_ext`, `set_seed`).
//!
//! The map-style accessors present the payload as a key/value map of user
//! data, while comment preservation and `$` access ride on the same
//! underlying storage.

use indexmap::IndexMap;
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::prescan::{CommentPathSegment, NestedComment};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Which out-of-band system-metadata map a [`PayloadItem::Meta`] carries.
///
/// `$ext` and `$seed` are the same shape — an opaque `Map<String, Value>` that
/// never reaches the plate JSON and round-trips through Markdown and the storage
/// DTO — so the live model represents them as one variant discriminated by this
/// key. They differ only in their canonical sort rank, whether they are
/// root-only, and (downstream of storage) whether the seeding layer interprets
/// them: `$ext` is opaque; `$seed` is read by [`crate::SeedOverlay::from_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaKey {
    /// `$ext` — opaque out-of-band consumer state (editor renames, agent
    /// annotations). Allowed on any card.
    Ext,
    /// `$seed` — per-card-kind seed overlays. **Root-only** (like `$quill`).
    Seed,
}

impl MetaKey {
    /// The literal source key (`"$ext"` / `"$seed"`).
    pub fn as_str(self) -> &'static str {
        match self {
            MetaKey::Ext => "$ext",
            MetaKey::Seed => "$seed",
        }
    }

    /// Parse the source key (`"$ext"` / `"$seed"`), or `None` for any other key.
    pub fn from_key_str(key: &str) -> Option<Self> {
        match key {
            "$ext" => Some(MetaKey::Ext),
            "$seed" => Some(MetaKey::Seed),
            _ => None,
        }
    }

    /// Canonical sort rank among typed `$` entries (after `$id`).
    fn rank(self) -> u8 {
        match self {
            MetaKey::Ext => 3,
            MetaKey::Seed => 4,
        }
    }

    /// `true` when the key may appear on the root card only (rejected on
    /// composable cards), like `$quill`.
    pub fn is_root_only(self) -> bool {
        matches!(self, MetaKey::Seed)
    }
}

/// One entry in a [`Payload`]: a typed `$` system metadata entry, a user
/// field, or a comment line.
///
/// `PayloadItem` is the live in-memory model; it is intentionally **not**
/// `Serialize`/`Deserialize`. Storage uses the versioned DTOs in
/// `document::dto`, and bindings translate to their own wire types.
#[derive(Debug, Clone, PartialEq)]
pub enum PayloadItem {
    /// `$quill` system metadata, holding the parsed quill reference.
    Quill { reference: QuillReference },
    /// `$kind` system metadata — the card's kind name.
    Kind { value: String },
    /// `$id` system metadata — the durable card handle: opaque,
    /// caller-supplied, unique per document across composable cards
    /// (`DOCUMENT_STORAGE.md` §Card-id identity).
    Id { value: String },
    /// `$ext` / `$seed` system metadata — an opaque mapping (discriminated by
    /// [`MetaKey`]) reserved for out-of-band data. Never emitted into the plate
    /// JSON, always round-trips through Markdown and the storage DTO.
    /// `nested_comments` carries YAML comments inside the mapping; paths are
    /// **relative** to the value tree (the `$ext` / `$seed` key itself is not
    /// part of the path). `$seed` is additionally interpreted by the seeding
    /// layer — see [`crate::SeedOverlay::from_json`] and [`crate::Quill::seed_card`].
    Meta {
        key: MetaKey,
        value: JsonMap<String, JsonValue>,
        nested_comments: Vec<NestedComment>,
    },
    /// A user-defined YAML field, optionally tagged `!must_fill`.
    ///
    /// `nested_comments` carries YAML comments inside the field's value
    /// (only meaningful when the value is a mapping or sequence); paths
    /// are **relative** to the field's value tree (the field's key is
    /// not part of the path).
    Field {
        key: String,
        value: QuillValue,
        /// `true` when the field was written as `key: !must_fill <value>` or
        /// `key: !must_fill` in source.
        fill: bool,
        nested_comments: Vec<NestedComment>,
    },
    /// A YAML comment. Text excludes the leading `#` and one optional space.
    ///
    /// `inline` distinguishes own-line comments (`# text` on a line by
    /// itself) from trailing inline comments (`field: value # text`). An
    /// inline comment attaches to the item that immediately precedes it
    /// in the items vector; if no such item exists at emit time (orphan)
    /// it degrades to an own-line comment.
    Comment { text: String, inline: bool },
}

impl PayloadItem {
    /// Build a plain (non-fill) field entry with no nested comments.
    pub fn field(key: impl Into<String>, value: QuillValue) -> Self {
        PayloadItem::Field {
            key: key.into(),
            value,
            fill: false,
            nested_comments: Vec::new(),
        }
    }

    /// Borrow the field/meta nested-comments slice. Returns `&[]` for
    /// variants that don't carry nested comments.
    pub fn nested_comments(&self) -> &[NestedComment] {
        match self {
            PayloadItem::Field {
                nested_comments, ..
            }
            | PayloadItem::Meta {
                nested_comments, ..
            } => nested_comments,
            _ => &[],
        }
    }

    pub fn comment(text: impl Into<String>) -> Self {
        PayloadItem::Comment {
            text: text.into(),
            inline: false,
        }
    }

    pub fn comment_inline(text: impl Into<String>) -> Self {
        PayloadItem::Comment {
            text: text.into(),
            inline: true,
        }
    }

    /// Canonical sort rank for typed `$` entries: `$quill` < `$kind` <
    /// `$id` < `$ext` < `$seed`. Returns `None` for user fields and comments,
    /// which are positioned by source order and never reshuffled.
    fn meta_rank(&self) -> Option<u8> {
        match self {
            PayloadItem::Quill { .. } => Some(0),
            PayloadItem::Kind { .. } => Some(1),
            PayloadItem::Id { .. } => Some(2),
            PayloadItem::Meta { key, .. } => Some(key.rank()),
            _ => None,
        }
    }
}

/// Ordered, comment-preserving payload of a card-yaml block.
///
/// Contains the block's `$` entries, user fields, and comments interleaved
/// in source order. See the module docs for the full design.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Payload {
    items: Vec<PayloadItem>,
}

impl Payload {
    /// Create an empty `Payload`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from an `IndexMap` of user fields. No `$` entries, no
    /// comments, no fill markers.
    pub fn from_index_map(map: IndexMap<String, QuillValue>) -> Self {
        let items = map
            .into_iter()
            .map(|(key, value)| PayloadItem::Field {
                key,
                value,
                fill: false,
                nested_comments: Vec::new(),
            })
            .collect();
        Self { items }
    }

    /// Build from a pre-computed item list (parser and DTO entry point).
    pub fn from_items(items: Vec<PayloadItem>) -> Self {
        Self { items }
    }

    /// Build from a pre-computed item list plus a flat absolute-path
    /// `nested_comments` Vec, partitioning the latter onto the matching
    /// [`PayloadItem::Field`] / [`PayloadItem::Meta`] items.
    ///
    /// The first segment of each comment's `container_path` must be a
    /// `Key(field)` matching a Field or Meta (`$ext` / `$seed`) entry in `items`;
    /// that first segment is stripped and the remainder attached to the
    /// owning item. Comments whose first segment matches nothing in
    /// `items` are dropped silently — this can only arise from a
    /// hand-crafted storage DTO that references a non-existent field.
    pub(crate) fn from_items_with_flat_nested(
        mut items: Vec<PayloadItem>,
        nested_comments: Vec<NestedComment>,
    ) -> Self {
        for nc in nested_comments {
            let Some((first, rest)) = nc.container_path.split_first() else {
                // Empty path can't address any user field; drop.
                continue;
            };
            let target_key = match first {
                CommentPathSegment::Key(k) => k.clone(),
                CommentPathSegment::Index(_) => continue,
            };

            let relative = NestedComment {
                container_path: rest.to_vec(),
                position: nc.position,
                text: nc.text,
                inline: nc.inline,
            };

            // `$ext` / `$seed` are encoded with their literal key at the head
            // of the path; everything else is a user field.
            let slot = if let Some(meta_key) = MetaKey::from_key_str(&target_key) {
                items.iter_mut().find_map(|i| match i {
                    PayloadItem::Meta {
                        key,
                        nested_comments,
                        ..
                    } if *key == meta_key => Some(nested_comments),
                    _ => None,
                })
            } else {
                items.iter_mut().find_map(|i| match i {
                    PayloadItem::Field {
                        key,
                        nested_comments,
                        ..
                    } if key == &target_key => Some(nested_comments),
                    _ => None,
                })
            };
            if let Some(slot) = slot {
                slot.push(relative);
            }
        }
        Self { items }
    }

    /// Walk every Field/Meta item and yield each nested comment with its
    /// path re-prefixed by the owning item's key (`$ext` / `$seed` for Meta,
    /// the field key for Field). Used by the storage DTO conversion to
    /// flatten the per-item storage back to the wire format's
    /// payload-level sidecar.
    pub(crate) fn flat_nested_comments(&self) -> Vec<NestedComment> {
        let mut out = Vec::new();
        for item in &self.items {
            let (prefix, comments) = match item {
                PayloadItem::Field {
                    key,
                    nested_comments,
                    ..
                } => (key.clone(), nested_comments),
                PayloadItem::Meta {
                    key,
                    nested_comments,
                    ..
                } => (key.as_str().to_string(), nested_comments),
                _ => continue,
            };
            for nc in comments {
                let mut path = Vec::with_capacity(nc.container_path.len() + 1);
                path.push(CommentPathSegment::Key(prefix.clone()));
                path.extend(nc.container_path.iter().cloned());
                out.push(NestedComment {
                    container_path: path,
                    position: nc.position,
                    text: nc.text.clone(),
                    inline: nc.inline,
                });
            }
        }
        out
    }

    // ── Item-level access ───────────────────────────────────────────────────

    /// Ordered iterator over raw items (`$` entries, fields, comments).
    pub fn items(&self) -> &[PayloadItem] {
        &self.items
    }

    /// Mutable access to the raw item list. Callers must preserve the
    /// invariants (at most one `Quill`/`Kind`/`Id`/`Ext`, no duplicate
    /// field keys, every field name matches `[A-Za-z_][A-Za-z0-9_]*`) — use
    /// the typed mutators when in doubt.
    pub fn items_mut(&mut self) -> &mut [PayloadItem] {
        &mut self.items
    }

    /// Remove the first item matching `pred` and return it. The typed
    /// removers (`take_id`, `take_meta`, `remove`) wrap this and destructure
    /// the returned variant, which `pred` guarantees.
    fn take_item(&mut self, pred: impl Fn(&PayloadItem) -> bool) -> Option<PayloadItem> {
        let pos = self.items.iter().position(pred)?;
        Some(self.items.remove(pos))
    }

    // ── Typed `$` access ────────────────────────────────────────────────────

    /// The `$quill` reference, if declared.
    pub fn quill(&self) -> Option<&QuillReference> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Quill { reference } => Some(reference),
            _ => None,
        })
    }

    /// The `$kind` value, if declared.
    pub fn kind(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Kind { value } => Some(value.as_str()),
            _ => None,
        })
    }

    /// The `$id` value, if declared.
    pub fn id(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Id { value } => Some(value.as_str()),
            _ => None,
        })
    }

    /// The map for the given out-of-band meta key, if declared.
    fn meta(&self, want: MetaKey) -> Option<&JsonMap<String, JsonValue>> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Meta { key, value, .. } if *key == want => Some(value),
            _ => None,
        })
    }

    /// The `$ext` map, if declared. The map is opaque — Quillmark does not
    /// interpret its contents and never emits them into the plate JSON.
    pub fn ext(&self) -> Option<&JsonMap<String, JsonValue>> {
        self.meta(MetaKey::Ext)
    }

    /// The raw `$seed` map (keyed by card-kind), if declared. The seeding
    /// layer interprets it; it never reaches the plate JSON. For a parsed,
    /// per-kind overlay, index this map by kind and pass the entry to
    /// [`crate::SeedOverlay::from_json`].
    pub fn seed(&self) -> Option<&JsonMap<String, JsonValue>> {
        self.meta(MetaKey::Seed)
    }

    /// Set or replace the `$quill` entry. Inserts at canonical position
    /// (before any `$kind` / `$id` / `$ext`) when adding. Comments are
    /// untouched.
    pub fn set_quill(&mut self, reference: QuillReference) {
        self.upsert_meta(PayloadItem::Quill { reference });
    }

    /// Set or replace the `$kind` entry. Same insertion rules as
    /// [`set_quill`](Self::set_quill).
    pub fn set_kind(&mut self, kind: impl Into<String>) {
        self.upsert_meta(PayloadItem::Kind { value: kind.into() });
    }

    /// Set or replace the `$id` entry. Same insertion rules as
    /// [`set_quill`](Self::set_quill).
    ///
    /// This is the stamping door for a card **not yet placed** in a document
    /// (mint → stamp → insert); uniqueness is checked at insertion. For a
    /// placed card, write through the guarded
    /// [`Document::set_card_id`](crate::Document::set_card_id) so the
    /// per-document uniqueness of `$id` holds.
    pub fn set_id(&mut self, id: impl Into<String>) {
        self.upsert_meta(PayloadItem::Id { value: id.into() });
    }

    /// Remove the `$id` entry, returning the previous value if any. Removal
    /// cannot collide, so no document-level guard exists or is needed.
    pub fn take_id(&mut self) -> Option<String> {
        match self.take_item(|i| matches!(i, PayloadItem::Id { .. }))? {
            PayloadItem::Id { value } => Some(value),
            _ => unreachable!(),
        }
    }

    /// Set or replace an out-of-band meta entry at its canonical position.
    /// Nested comments on a replaced entry are dropped (the new value tree
    /// may not contain matching positions).
    fn set_meta(&mut self, key: MetaKey, value: JsonMap<String, JsonValue>) {
        self.upsert_meta(PayloadItem::Meta {
            key,
            value,
            nested_comments: Vec::new(),
        });
    }

    /// Set or replace the `$ext` entry. Same insertion rules as
    /// [`set_quill`](Self::set_quill); the canonical position is after
    /// `$quill` / `$kind` / `$id` and before any user field.
    ///
    /// Any nested comments previously attached to a replaced `$ext`
    /// entry are dropped (the new value tree may not contain matching
    /// positions).
    pub fn set_ext(&mut self, value: JsonMap<String, JsonValue>) {
        self.set_meta(MetaKey::Ext, value);
    }

    /// Set or replace the `$seed` entry. Inserted at the canonical position
    /// (after `$quill` / `$kind` / `$id` / `$ext`, before any user field).
    /// Nested comments on a replaced `$seed` are dropped, like
    /// [`set_ext`](Self::set_ext).
    pub fn set_seed(&mut self, value: JsonMap<String, JsonValue>) {
        self.set_meta(MetaKey::Seed, value);
    }

    /// Remove an out-of-band meta entry, returning the previous map if any.
    /// Any nested comments attached to the entry are dropped.
    fn take_meta(&mut self, want: MetaKey) -> Option<JsonMap<String, JsonValue>> {
        match self.take_item(|i| matches!(i, PayloadItem::Meta { key, .. } if *key == want))? {
            PayloadItem::Meta { value, .. } => Some(value),
            _ => unreachable!(),
        }
    }

    /// Remove the `$ext` entry, returning the previous map if any. Any
    /// nested comments attached to the entry are dropped.
    pub fn take_ext(&mut self) -> Option<JsonMap<String, JsonValue>> {
        self.take_meta(MetaKey::Ext)
    }

    /// Remove the `$seed` entry, returning the previous map if any. Any
    /// nested comments attached to the entry are dropped.
    pub fn take_seed(&mut self) -> Option<JsonMap<String, JsonValue>> {
        self.take_meta(MetaKey::Seed)
    }

    fn upsert_meta(&mut self, new: PayloadItem) {
        let new_rank = new
            .meta_rank()
            .expect("upsert_meta only accepts $-typed items");
        for slot in self.items.iter_mut() {
            if slot.meta_rank() == Some(new_rank) {
                *slot = new;
                return;
            }
        }
        let insert_at = self
            .items
            .iter()
            .position(|i| matches!(i.meta_rank(), Some(r) if r > new_rank))
            .unwrap_or_else(|| {
                // No higher-ranked `$` item; insert after the last lower
                // (or equal-rank-impossible) `$` item, before any non-`$`
                // entry. This keeps the `$quill < $kind < $id` ordering
                // while not displacing user fields.
                self.items
                    .iter()
                    .rposition(|i| matches!(i.meta_rank(), Some(r) if r < new_rank))
                    .map(|p| p + 1)
                    .unwrap_or(0)
            });
        self.items.insert(insert_at, new);
    }

    // ── User-field access (map-style, `$` entries filtered out) ─────────────

    /// Iterator over user `(key, &value)` pairs. Excludes `$` entries and
    /// comments; preserves source order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &QuillValue)> + '_ {
        self.items.iter().filter_map(|item| match item {
            PayloadItem::Field { key, value, .. } => Some((key, value)),
            _ => None,
        })
    }

    /// Iterator over user field keys.
    pub fn keys(&self) -> impl Iterator<Item = &String> + '_ {
        self.items.iter().filter_map(|item| match item {
            PayloadItem::Field { key, .. } => Some(key),
            _ => None,
        })
    }

    /// Number of *user-field* items (`$` entries and comments excluded).
    pub fn len(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, PayloadItem::Field { .. }))
            .count()
    }

    /// `true` when there are no user-field items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Look up a user-field value by key. `$` entries are not visible via
    /// this accessor — use [`quill`](Self::quill) / [`kind`](Self::kind) /
    /// [`id`](Self::id).
    pub fn get(&self, key: &str) -> Option<&QuillValue> {
        self.items.iter().find_map(|item| match item {
            PayloadItem::Field { key: k, value, .. } if k == key => Some(value),
            _ => None,
        })
    }

    /// `true` if a user field with this key is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `true` if a user field with this key is marked `!must_fill`.
    pub fn is_fill(&self, key: &str) -> bool {
        self.items.iter().any(|item| match item {
            PayloadItem::Field { key: k, fill, .. } => k == key && *fill,
            _ => false,
        })
    }

    /// Insert or update a user field, clearing any `!must_fill` marker.
    /// Preserves position for an existing key; appends a new one. `$` entries
    /// and comments are untouched; replacing a field discards its
    /// `nested_comments` (the new value tree may not carry matching positions).
    ///
    /// Validates the field name and value depth
    /// ([`validate_field`](super::edit::validate_field)) at this boundary, so
    /// the "a constructed document cannot be invalid" invariant holds even for
    /// the direct `Payload` path reachable through
    /// [`Card::payload_mut`](super::Card::payload_mut). Pre-validated callers
    /// (typed commit, all-or-nothing batches) use `insert_unchecked` to skip the
    /// redundant check.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: QuillValue,
    ) -> Result<Option<QuillValue>, super::edit::FieldViolation> {
        let key = key.into();
        super::edit::validate_field(&key, value.as_json())?;
        Ok(self.insert_item(key, value, false))
    }

    /// Insert or update a user field and mark it a `!must_fill` placeholder;
    /// same rules and boundary validation as [`insert`](Self::insert).
    pub fn insert_fill(
        &mut self,
        key: impl Into<String>,
        value: QuillValue,
    ) -> Result<Option<QuillValue>, super::edit::FieldViolation> {
        let key = key.into();
        super::edit::validate_field(&key, value.as_json())?;
        Ok(self.insert_item(key, value, true))
    }

    /// [`insert`](Self::insert) without the field-invariant check. `pub(crate)`
    /// for callers that have already validated the exact stored `(name, value)`
    /// — `resolve_field_write` and the batch setters that validate the whole
    /// batch before applying any of it.
    pub(crate) fn insert_unchecked(
        &mut self,
        key: impl Into<String>,
        value: QuillValue,
    ) -> Option<QuillValue> {
        self.insert_item(key.into(), value, false)
    }

    /// Insert or replace field `key` with `value`, setting its fill marker.
    /// Position-preserving for an existing key, append otherwise.
    fn insert_item(&mut self, key: String, value: QuillValue, fill: bool) -> Option<QuillValue> {
        for item in self.items.iter_mut() {
            if let PayloadItem::Field {
                key: k,
                value: v,
                fill: item_fill,
                nested_comments,
            } = item
            {
                if k == &key {
                    let old = std::mem::replace(v, value);
                    *item_fill = fill;
                    nested_comments.clear();
                    return Some(old);
                }
            }
        }
        self.items.push(PayloadItem::Field {
            key,
            value,
            fill,
            nested_comments: Vec::new(),
        });
        None
    }

    /// Remove a user field by key, returning its value. Comments and `$`
    /// entries are untouched.
    pub fn remove(&mut self, key: &str) -> Option<QuillValue> {
        match self.take_item(|item| matches!(item, PayloadItem::Field { key: k, .. } if k == key))? {
            PayloadItem::Field { value, .. } => Some(value),
            _ => unreachable!(),
        }
    }

    /// Project the user-field portion into an `IndexMap<String, QuillValue>`.
    /// Comments, fill markers, and `$` entries are dropped. Preserves order.
    pub fn to_index_map(&self) -> IndexMap<String, QuillValue> {
        let mut map = IndexMap::new();
        for item in &self.items {
            if let PayloadItem::Field { key, value, .. } = item {
                map.insert(key.clone(), value.clone());
            }
        }
        map
    }
}

impl<'a> IntoIterator for &'a Payload {
    type Item = (&'a String, &'a QuillValue);
    type IntoIter = std::iter::FilterMap<
        std::slice::Iter<'a, PayloadItem>,
        fn(&'a PayloadItem) -> Option<(&'a String, &'a QuillValue)>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        fn filter(item: &PayloadItem) -> Option<(&String, &QuillValue)> {
            match item {
                PayloadItem::Field { key, value, .. } => Some((key, value)),
                _ => None,
            }
        }
        self.items.iter().filter_map(filter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qv(s: &str) -> QuillValue {
        QuillValue::from_json(serde_json::json!(s))
    }

    #[test]
    fn insert_new_appends_after_meta() {
        let mut fm = Payload::new();
        fm.set_quill("foo@0.1".parse().unwrap());
        fm.set_kind("main");
        fm.insert("title", qv("Hello")).unwrap();
        let last = fm.items().last().unwrap();
        assert!(matches!(last, PayloadItem::Field { key, .. } if key == "title"));
    }

    #[test]
    fn insert_existing_preserves_position() {
        let mut fm = Payload::new();
        fm.insert("a", qv("1")).unwrap();
        fm.insert("b", qv("2")).unwrap();
        fm.insert("a", qv("updated")).unwrap();
        let keys: Vec<&String> = fm.keys().collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(fm.get("a").unwrap().as_str(), Some("updated"));
    }

    #[test]
    fn insert_clears_fill() {
        let mut fm = Payload::new();
        fm.insert_fill("k", qv("placeholder")).unwrap();
        assert!(fm.is_fill("k"));
        fm.insert("k", qv("user value")).unwrap();
        assert!(!fm.is_fill("k"));
    }

    #[test]
    fn insert_enforces_the_field_invariant() {
        use super::super::edit::FieldViolation;

        // A malformed name is refused: `payload_mut().insert(...)` cannot seat
        // an invalid field in a "constructed" document.
        let mut fm = Payload::new();
        assert_eq!(fm.insert("bad name", qv("v")), Err(FieldViolation::InvalidName));
        assert_eq!(fm.insert("$id", qv("v")), Err(FieldViolation::InvalidName));
        assert_eq!(
            fm.insert_fill("bad name", qv("v")),
            Err(FieldViolation::InvalidName)
        );

        // Over-deep value.
        let mut deep = serde_json::json!(0);
        for _ in 0..(crate::document::limits::MAX_YAML_DEPTH + 5) {
            deep = serde_json::json!([deep]);
        }
        assert_eq!(
            fm.insert("field", QuillValue::from_json(deep)),
            Err(FieldViolation::TooDeep)
        );

        // Nothing was applied on any rejection.
        assert!(fm.items().is_empty());

        // The unchecked path is the deliberate escape hatch — no validation.
        fm.insert_unchecked("bad name", qv("v"));
        assert_eq!(fm.items().len(), 1);
    }

    #[test]
    fn map_style_iter_skips_meta_and_comments() {
        let mut fm = Payload::new();
        fm.set_quill("foo@0.1".parse().unwrap());
        fm.set_kind("main");
        let _ = fm.insert("title", qv("Hello"));
        let items = std::mem::take(&mut fm).items().to_vec();
        // Reconstruct with an interleaved comment.
        let mut items_with_comment = items;
        items_with_comment.insert(2, PayloadItem::comment("c"));
        let fm = Payload::from_items(items_with_comment);
        let pairs: Vec<(String, String)> = fm
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect();
        assert_eq!(pairs, vec![("title".to_string(), "Hello".to_string())]);
        // But the typed access still works:
        assert_eq!(fm.kind(), Some("main"));
    }

    #[test]
    fn set_quill_inserts_at_position_zero() {
        let mut fm = Payload::new();
        fm.set_kind("main");
        fm.set_quill("foo@0.1".parse().unwrap());
        assert!(matches!(fm.items()[0], PayloadItem::Quill { .. }));
        assert!(matches!(fm.items()[1], PayloadItem::Kind { .. }));
    }

    #[test]
    fn set_id_inserts_after_quill_and_kind() {
        let mut fm = Payload::new();
        fm.set_quill("foo@0.1".parse().unwrap());
        fm.set_kind("main");
        fm.set_id("rev-1");
        assert_eq!(fm.items().len(), 3);
        assert!(matches!(fm.items()[2], PayloadItem::Id { .. }));
    }

    #[test]
    fn set_replaces_in_place_preserving_comments() {
        let mut fm = Payload::from_items(vec![
            PayloadItem::Quill {
                reference: "foo@0.1".parse().unwrap(),
            },
            PayloadItem::comment_inline("trailing"),
            PayloadItem::Kind {
                value: "main".into(),
            },
        ]);
        fm.set_quill("bar@0.2".parse().unwrap());
        assert_eq!(fm.quill().unwrap().to_string(), "bar@0.2");
        assert_eq!(fm.items().len(), 3);
        assert!(matches!(fm.items()[1], PayloadItem::Comment { .. }));
    }

    #[test]
    fn remove_leaves_comments_and_meta_alone() {
        let mut fm = Payload::from_items(vec![
            PayloadItem::Quill {
                reference: "q".parse().unwrap(),
            },
            PayloadItem::Kind {
                value: "main".into(),
            },
            PayloadItem::comment("header"),
            PayloadItem::field("a", qv("1")),
            PayloadItem::comment("mid"),
            PayloadItem::field("b", qv("2")),
        ]);
        let removed = fm.remove("a").unwrap();
        assert_eq!(removed.as_str(), Some("1"));
        assert!(matches!(fm.items()[0], PayloadItem::Quill { .. }));
        assert!(matches!(fm.items()[1], PayloadItem::Kind { .. }));
        let comments: Vec<&str> = fm
            .items()
            .iter()
            .filter_map(|item| match item {
                PayloadItem::Comment { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(comments, vec!["header", "mid"]);
    }
}
