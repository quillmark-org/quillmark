//! Unified payload representation.
//!
//! A [`Payload`] is the typed representation of a card-yaml block's full
//! YAML content. It carries — in source order, as variants of a single
//! [`PayloadItem`] enum:
//!
//! - **System metadata** — typed `$quill` / `$kind` / `$id` entries.
//! - **User fields** — `key: value` pairs with an optional `!fill` flag.
//! - **Comments** — own-line or trailing inline, attached to whichever
//!   item they immediately follow at emit time.
//!
//! The unified item list is the canonical storage of the block; treating
//! `$` entries as just another variant means a comment adjacent to a `$`
//! line round-trips through the same mechanism as a comment adjacent to a
//! user field. No "metadata region" vs "payload region" routing decision is
//! ever made — there is only the source-ordered list.
//!
//! ## Two faces
//!
//! [`Payload`] exposes both ordered iteration (over the raw items vec) and
//! map-keyed access (`get`, `iter`, `insert`, `remove`). The map-style
//! accessors filter to [`PayloadItem::Field`] only — they intentionally
//! don't expose `$` entries because typed `$` access has dedicated methods
//! (`quill`, `kind`, `id`, `set_quill`, `set_kind`, `set_id`).
//!
//! This preserves the historical "payload as a key/value map of user data"
//! API while letting comment preservation and `$` access ride on the same
//! underlying storage.

use indexmap::IndexMap;

use super::prescan::NestedComment;
use crate::value::QuillValue;
use crate::version::QuillReference;

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
    /// `$id` system metadata — opaque identifier.
    Id { value: String },
    /// A user-defined YAML field, optionally tagged `!fill`.
    Field {
        key: String,
        value: QuillValue,
        /// `true` when the field was written as `key: !fill <value>` or
        /// `key: !fill` in source.
        fill: bool,
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
    /// Build a plain (non-fill) field entry.
    pub fn field(key: impl Into<String>, value: QuillValue) -> Self {
        PayloadItem::Field {
            key: key.into(),
            value,
            fill: false,
        }
    }

    /// Build an own-line comment item.
    pub fn comment(text: impl Into<String>) -> Self {
        PayloadItem::Comment {
            text: text.into(),
            inline: false,
        }
    }

    /// Build an inline (trailing) comment item.
    pub fn comment_inline(text: impl Into<String>) -> Self {
        PayloadItem::Comment {
            text: text.into(),
            inline: true,
        }
    }

    /// Canonical sort rank for typed `$` entries: `$quill` < `$kind` <
    /// `$id`. Returns `None` for user fields and comments, which are
    /// positioned by source order and never reshuffled.
    fn meta_rank(&self) -> Option<u8> {
        match self {
            PayloadItem::Quill { .. } => Some(0),
            PayloadItem::Kind { .. } => Some(1),
            PayloadItem::Id { .. } => Some(2),
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
    nested_comments: Vec<NestedComment>,
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
            })
            .collect();
        Self {
            items,
            nested_comments: Vec::new(),
        }
    }

    /// Build from a pre-computed item list (parser and DTO entry point).
    pub fn from_items(items: Vec<PayloadItem>) -> Self {
        Self {
            items,
            nested_comments: Vec::new(),
        }
    }

    /// Build from a pre-computed item list and nested comments.
    pub fn from_items_with_nested(
        items: Vec<PayloadItem>,
        nested_comments: Vec<NestedComment>,
    ) -> Self {
        Self {
            items,
            nested_comments,
        }
    }

    // ── Item-level access ───────────────────────────────────────────────────

    /// Ordered iterator over raw items (`$` entries, fields, comments).
    pub fn items(&self) -> &[PayloadItem] {
        &self.items
    }

    /// Mutable access to the raw item list. Callers must preserve the
    /// invariants (at most one `Quill`/`Kind`/`Id`, no duplicate field
    /// keys, no reserved field names) — use the typed mutators when in
    /// doubt.
    pub fn items_mut(&mut self) -> &mut [PayloadItem] {
        &mut self.items
    }

    /// Comments captured inside nested mappings/sequences. The emitter
    /// re-injects these at the matching position when serialising the
    /// value tree.
    pub fn nested_comments(&self) -> &[NestedComment] {
        &self.nested_comments
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

    /// Set or replace the `$quill` entry. Inserts at canonical position
    /// (before any `$kind` / `$id`) when adding. Comments are untouched.
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
    pub fn set_id(&mut self, id: impl Into<String>) {
        self.upsert_meta(PayloadItem::Id { value: id.into() });
    }

    /// Remove the `$quill` entry, returning the previous value if any.
    pub fn take_quill(&mut self) -> Option<QuillReference> {
        let pos = self
            .items
            .iter()
            .position(|i| matches!(i, PayloadItem::Quill { .. }))?;
        match self.items.remove(pos) {
            PayloadItem::Quill { reference } => Some(reference),
            _ => unreachable!(),
        }
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

    /// `true` if a user field with this key is marked `!fill`.
    pub fn is_fill(&self, key: &str) -> bool {
        self.items.iter().any(|item| match item {
            PayloadItem::Field { key: k, fill, .. } => k == key && *fill,
            _ => false,
        })
    }

    /// Insert or update a user field. Always clears the `fill` marker
    /// (field is no longer a placeholder). Preserves position for existing
    /// keys; appends new keys at the end. `$` entries and comments are
    /// untouched.
    pub fn insert(&mut self, key: impl Into<String>, value: QuillValue) -> Option<QuillValue> {
        let key = key.into();
        for item in self.items.iter_mut() {
            if let PayloadItem::Field {
                key: k,
                value: v,
                fill,
            } = item
            {
                if k == &key {
                    let old = std::mem::replace(v, value);
                    *fill = false;
                    return Some(old);
                }
            }
        }
        self.items.push(PayloadItem::Field {
            key,
            value,
            fill: false,
        });
        None
    }

    /// Insert or update a user field and mark it as a `!fill` placeholder.
    /// Preserves position for existing keys; appends new keys at the end.
    pub fn insert_fill(&mut self, key: impl Into<String>, value: QuillValue) -> Option<QuillValue> {
        let key = key.into();
        for item in self.items.iter_mut() {
            if let PayloadItem::Field {
                key: k,
                value: v,
                fill,
            } = item
            {
                if k == &key {
                    let old = std::mem::replace(v, value);
                    *fill = true;
                    return Some(old);
                }
            }
        }
        self.items.push(PayloadItem::Field {
            key,
            value,
            fill: true,
        });
        None
    }

    /// Remove a user field by key, returning its value. Comments and `$`
    /// entries are untouched.
    pub fn remove(&mut self, key: &str) -> Option<QuillValue> {
        let pos = self
            .items
            .iter()
            .position(|item| matches!(item, PayloadItem::Field { key: k, .. } if k == key))?;
        match self.items.remove(pos) {
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
        fm.insert("title", qv("Hello"));
        let last = fm.items().last().unwrap();
        assert!(matches!(last, PayloadItem::Field { key, .. } if key == "title"));
    }

    #[test]
    fn insert_existing_preserves_position() {
        let mut fm = Payload::new();
        fm.insert("a", qv("1"));
        fm.insert("b", qv("2"));
        fm.insert("a", qv("updated"));
        let keys: Vec<&String> = fm.keys().collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(fm.get("a").unwrap().as_str(), Some("updated"));
    }

    #[test]
    fn insert_clears_fill() {
        let mut fm = Payload::new();
        fm.insert_fill("k", qv("placeholder"));
        assert!(fm.is_fill("k"));
        fm.insert("k", qv("user value"));
        assert!(!fm.is_fill("k"));
    }

    #[test]
    fn map_style_iter_skips_meta_and_comments() {
        let mut fm = Payload::new();
        fm.set_quill("foo@0.1".parse().unwrap());
        fm.set_kind("main");
        fm.insert("title", qv("Hello"));
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
