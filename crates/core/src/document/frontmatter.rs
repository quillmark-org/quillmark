//! Ordered frontmatter representation.
//!
//! A [`Frontmatter`] is the typed representation of a YAML fence body with the
//! sentinel key already stripped. Unlike a plain `IndexMap`, it preserves
//! YAML comments as first-class ordered items and carries a `fill: bool`
//! marker on each field (for `!fill` tags).
//!
//! It provides both ordered iteration (over [`FrontmatterItem`]s) and
//! map-keyed access (`get`, `contains_key`, `insert`, `remove`) so existing
//! callers that treat the frontmatter as a map keep working. The map-keyed
//! accessors walk the item vec; field count is small enough that a linear
//! scan is fine.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::prescan::NestedComment;
use crate::value::QuillValue;

/// One entry in a [`Frontmatter`]: a field or a comment line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FrontmatterItem {
    /// A YAML field (key-value pair), optionally tagged `!fill`.
    Field {
        key: String,
        value: QuillValue,
        /// `true` when the field was written as `key: !fill <value>` or
        /// `key: !fill` in source.
        #[serde(default)]
        fill: bool,
    },
    /// A YAML comment. Text excludes the leading `#` and one optional space.
    ///
    /// `inline` distinguishes own-line comments (`# text` on a line by
    /// itself) from trailing inline comments (`field: value # text`). An
    /// inline comment attaches to the field that immediately precedes it
    /// in the items vector; if no such field exists at emit time (orphan)
    /// it degrades to an own-line comment. A `Comment { inline: true }` at
    /// `items[0]` instead attaches to the sentinel line (`QUILL: …` /
    /// `CARD: …`).
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

impl FrontmatterItem {
    /// Build a plain (non-fill) field entry.
    pub fn field(key: impl Into<String>, value: QuillValue) -> Self {
        FrontmatterItem::Field {
            key: key.into(),
            value,
            fill: false,
        }
    }

    /// Build an own-line comment item.
    pub fn comment(text: impl Into<String>) -> Self {
        FrontmatterItem::Comment {
            text: text.into(),
            inline: false,
        }
    }

    /// Build an inline (trailing) comment item. Attaches to the previous
    /// field on emit; degrades to own-line if none exists.
    pub fn comment_inline(text: impl Into<String>) -> Self {
        FrontmatterItem::Comment {
            text: text.into(),
            inline: true,
        }
    }
}

/// Ordered list of frontmatter items with map-keyed convenience accessors.
///
/// Top-level YAML comments live in `items` as [`FrontmatterItem::Comment`].
/// Comments inside nested mappings/sequences live in `nested_comments`,
/// keyed by structural path; the emitter re-injects them at the matching
/// position when serialising the value tree.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frontmatter {
    items: Vec<FrontmatterItem>,
    nested_comments: Vec<NestedComment>,
}

impl Frontmatter {
    /// Create an empty `Frontmatter`.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            nested_comments: Vec::new(),
        }
    }

    /// Build from an `IndexMap` of fields (no comments, no fill markers).
    pub fn from_index_map(map: IndexMap<String, QuillValue>) -> Self {
        let items = map
            .into_iter()
            .map(|(key, value)| FrontmatterItem::Field {
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

    /// Build from a pre-computed item list.
    pub fn from_items(items: Vec<FrontmatterItem>) -> Self {
        Self {
            items,
            nested_comments: Vec::new(),
        }
    }

    /// Build from a pre-computed item list and a set of nested comments.
    pub fn from_items_with_nested(
        items: Vec<FrontmatterItem>,
        nested_comments: Vec<NestedComment>,
    ) -> Self {
        Self {
            items,
            nested_comments,
        }
    }

    /// Comments captured inside nested mappings/sequences. The emitter
    /// re-injects these at the matching position when serialising the
    /// value tree.
    pub fn nested_comments(&self) -> &[NestedComment] {
        &self.nested_comments
    }

    /// Ordered iterator over raw items (including comments).
    pub fn items(&self) -> &[FrontmatterItem] {
        &self.items
    }

    /// Iterator over `(key, value)` pairs, skipping comments. Preserves order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &QuillValue)> + '_ {
        self.items.iter().filter_map(|item| match item {
            FrontmatterItem::Field { key, value, .. } => Some((key, value)),
            FrontmatterItem::Comment { .. } => None,
        })
    }

    /// Iterator over field keys, skipping comments. Preserves order.
    pub fn keys(&self) -> impl Iterator<Item = &String> + '_ {
        self.items.iter().filter_map(|item| match item {
            FrontmatterItem::Field { key, .. } => Some(key),
            FrontmatterItem::Comment { .. } => None,
        })
    }

    /// Number of *field* items (comments excluded).
    pub fn len(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, FrontmatterItem::Field { .. }))
            .count()
    }

    /// Returns `true` if there are no field items (comments are ignored).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Look up a field value by key.
    pub fn get(&self, key: &str) -> Option<&QuillValue> {
        self.items.iter().find_map(|item| match item {
            FrontmatterItem::Field { key: k, value, .. } if k == key => Some(value),
            _ => None,
        })
    }

    /// Returns `true` if a field with this key is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Insert or update a field. Always clears the `fill` marker (field is no
    /// longer a placeholder). Preserves position for existing keys; appends
    /// new keys at the end. Adjacent comments are untouched.
    pub fn insert(&mut self, key: impl Into<String>, value: QuillValue) -> Option<QuillValue> {
        let key = key.into();
        for item in self.items.iter_mut() {
            if let FrontmatterItem::Field {
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
        self.items.push(FrontmatterItem::Field {
            key,
            value,
            fill: false,
        });
        None
    }

    /// Insert or update a field and mark it as a `!fill` placeholder. Preserves
    /// position for existing keys; appends new keys at the end.
    pub fn insert_fill(&mut self, key: impl Into<String>, value: QuillValue) -> Option<QuillValue> {
        let key = key.into();
        for item in self.items.iter_mut() {
            if let FrontmatterItem::Field {
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
        self.items.push(FrontmatterItem::Field {
            key,
            value,
            fill: true,
        });
        None
    }

    /// Remove a field by key and return its value. Adjacent comments stay
    /// where they are.
    pub fn remove(&mut self, key: &str) -> Option<QuillValue> {
        let pos = self
            .items
            .iter()
            .position(|item| matches!(item, FrontmatterItem::Field { key: k, .. } if k == key))?;
        match self.items.remove(pos) {
            FrontmatterItem::Field { value, .. } => Some(value),
            FrontmatterItem::Comment { .. } => unreachable!(),
        }
    }

    /// Returns `true` if a field with this key is marked `!fill`.
    pub fn is_fill(&self, key: &str) -> bool {
        self.items.iter().any(|item| match item {
            FrontmatterItem::Field { key: k, fill, .. } => k == key && *fill,
            _ => false,
        })
    }

    /// Project the field portion into an `IndexMap<String, QuillValue>`.
    /// Comments are dropped; fill markers are lost. Preserves order.
    pub fn to_index_map(&self) -> IndexMap<String, QuillValue> {
        let mut map = IndexMap::new();
        for item in &self.items {
            if let FrontmatterItem::Field { key, value, .. } = item {
                map.insert(key.clone(), value.clone());
            }
        }
        map
    }
}

impl<'a> IntoIterator for &'a Frontmatter {
    type Item = (&'a String, &'a QuillValue);
    type IntoIter = std::iter::FilterMap<
        std::slice::Iter<'a, FrontmatterItem>,
        fn(&'a FrontmatterItem) -> Option<(&'a String, &'a QuillValue)>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        fn filter<'a>(item: &'a FrontmatterItem) -> Option<(&'a String, &'a QuillValue)> {
            match item {
                FrontmatterItem::Field { key, value, .. } => Some((key, value)),
                FrontmatterItem::Comment { .. } => None,
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
    fn insert_new_appends() {
        let mut fm = Frontmatter::new();
        fm.insert("title", qv("Hello"));
        fm.insert("author", qv("Alice"));
        assert_eq!(fm.len(), 2);
        let keys: Vec<&String> = fm.keys().collect();
        assert_eq!(keys, vec!["title", "author"]);
    }

    #[test]
    fn insert_existing_preserves_position() {
        let mut fm = Frontmatter::new();
        fm.insert("a", qv("1"));
        fm.insert("b", qv("2"));
        fm.insert("a", qv("updated"));
        let keys: Vec<&String> = fm.keys().collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(fm.get("a").unwrap().as_str(), Some("updated"));
    }

    #[test]
    fn insert_clears_fill() {
        let mut fm = Frontmatter::new();
        fm.insert_fill("k", qv("placeholder"));
        assert!(fm.is_fill("k"));
        fm.insert("k", qv("user value"));
        assert!(!fm.is_fill("k"));
    }

    #[test]
    fn insert_fill_preserves_position_and_sets_flag() {
        let mut fm = Frontmatter::new();
        fm.insert("k", qv("v"));
        fm.insert_fill("k", qv("placeholder"));
        assert!(fm.is_fill("k"));
        assert_eq!(fm.get("k").unwrap().as_str(), Some("placeholder"));
    }

    #[test]
    fn remove_leaves_comments_alone() {
        let items = vec![
            FrontmatterItem::comment("header"),
            FrontmatterItem::field("a", qv("1")),
            FrontmatterItem::comment("mid"),
            FrontmatterItem::field("b", qv("2")),
        ];
        let mut fm = Frontmatter::from_items(items);
        let removed = fm.remove("a").unwrap();
        assert_eq!(removed.as_str(), Some("1"));
        let comments: Vec<&str> = fm
            .items()
            .iter()
            .filter_map(|item| match item {
                FrontmatterItem::Comment { text, .. } => Some(text.as_str()),
                FrontmatterItem::Field { .. } => None,
            })
            .collect();
        assert_eq!(comments, vec!["header", "mid"]);
    }

    #[test]
    fn map_style_iter_skips_comments() {
        let items = vec![
            FrontmatterItem::comment("c"),
            FrontmatterItem::field("a", qv("1")),
            FrontmatterItem::field("b", qv("2")),
        ];
        let fm = Frontmatter::from_items(items);
        let pairs: Vec<(String, String)> = fm
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ]
        );
    }
}
