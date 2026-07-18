//! Value type for unified representation of TOML/YAML/JSON values.
//!
//! [`QuillValue`] is an **annotated value tree**: every node carries a
//! `fill` flag (the in-memory form of the `!must_fill` YAML tag) alongside
//! its data. The tree is the authoritative representation. For the data
//! API (`as_json`, `as_array`, `as_object`, `Deref`) a plain
//! [`serde_json::Value`] projection is materialized lazily and cached; that
//! projection is **fill-free** — it is a derived view of the data, not a
//! second source of truth. Fill never reaches the JSON projection, so
//! rendering and wire layers that consume `as_json()` are unaffected by it.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::ops::Deref;
use std::sync::OnceLock;

/// Unified value type: an annotated tree of JSON-shaped data where every
/// node additionally records whether it was tagged `!must_fill`.
///
/// Construction (`from_json`, `from_yaml_str`, the scalar constructors)
/// produces nodes with `fill = false`; the `!must_fill` markers are applied
/// by the document layer. `QuillValue` exposes no data-mutating methods —
/// only the fill setter `set_fill_at`, which does not affect the JSON
/// projection — so the cached projection never goes stale.
pub struct QuillValue {
    node: Node,
    /// Lazily materialized, fill-free [`serde_json::Value`] view of `node`.
    json: OnceLock<JsonValue>,
}

/// One node of the annotated tree: a `fill` flag plus the data.
#[derive(Debug, Clone, PartialEq)]
struct Node {
    fill: bool,
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Node>),
    Object(IndexMap<String, Node>),
}

/// One step of a path into a value tree: an object key or an array index.
///
/// This is the canonical path-segment type for the whole crate; the document
/// layer aliases it as `CommentPathSegment` for nested-comment paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

fn collect_fill_paths(node: &Node, prefix: &mut Vec<PathSegment>, out: &mut Vec<Vec<PathSegment>>) {
    if node.fill {
        out.push(prefix.clone());
    }
    match &node.kind {
        Kind::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                prefix.push(PathSegment::Index(i));
                collect_fill_paths(child, prefix, out);
                prefix.pop();
            }
        }
        Kind::Object(entries) => {
            for (k, child) in entries {
                prefix.push(PathSegment::Key(k.clone()));
                collect_fill_paths(child, prefix, out);
                prefix.pop();
            }
        }
        _ => {}
    }
}

fn node_at_mut<'a>(node: &'a mut Node, path: &[PathSegment]) -> Option<&'a mut Node> {
    let mut cur = node;
    for seg in path {
        cur = match (&mut cur.kind, seg) {
            (Kind::Object(entries), PathSegment::Key(k)) => entries.get_mut(k)?,
            (Kind::Array(items), PathSegment::Index(i)) => items.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn node_is_object(node: &Node, path: &[PathSegment]) -> bool {
    fn at<'a>(node: &'a Node, path: &[PathSegment]) -> Option<&'a Node> {
        let mut cur = node;
        for seg in path {
            cur = match (&cur.kind, seg) {
                (Kind::Object(entries), PathSegment::Key(k)) => entries.get(k)?,
                (Kind::Array(items), PathSegment::Index(i)) => items.get(*i)?,
                _ => return None,
            };
        }
        Some(cur)
    }
    matches!(at(node, path).map(|n| &n.kind), Some(Kind::Object(_)))
}

impl Node {
    fn from_json(value: &JsonValue) -> Node {
        let kind = match value {
            JsonValue::Null => Kind::Null,
            JsonValue::Bool(b) => Kind::Bool(*b),
            JsonValue::Number(n) => Kind::Number(n.clone()),
            JsonValue::String(s) => Kind::String(s.clone()),
            JsonValue::Array(items) => Kind::Array(items.iter().map(Node::from_json).collect()),
            JsonValue::Object(map) => Kind::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Node::from_json(v)))
                    .collect(),
            ),
        };
        Node { fill: false, kind }
    }

    fn to_json(&self) -> JsonValue {
        match &self.kind {
            Kind::Null => JsonValue::Null,
            Kind::Bool(b) => JsonValue::Bool(*b),
            Kind::Number(n) => JsonValue::Number(n.clone()),
            Kind::String(s) => JsonValue::String(s.clone()),
            Kind::Array(items) => JsonValue::Array(items.iter().map(Node::to_json).collect()),
            Kind::Object(entries) => JsonValue::Object(
                entries
                    .iter()
                    .map(|(k, n)| (k.clone(), n.to_json()))
                    .collect(),
            ),
        }
    }
}

/// `true` when `value` nests deeper than `max_depth` container levels.
///
/// Every path that stores a value into a `Document` — markdown parse,
/// DTO/wire deserialization, the typed mutators, the binding converters —
/// bounds nesting at the spec §8 limit
/// ([`crate::document::limits::MAX_YAML_DEPTH`]), which makes the recursive
/// consumers (emit, plate-JSON serialization, DTO conversion) bounded by
/// construction. The walk is iterative (explicit stack), so the check
/// itself cannot overflow on adversarially deep input — the very condition
/// it exists to detect.
///
/// The unit is **container levels**, not nodes: only arrays/objects are
/// charged a level, and the scalar leaf at the bottom of a chain is never
/// checked. So `max_depth` nested containers are accepted whether the deepest
/// holds a scalar, is empty, or holds another container; `max_depth + 1` is
/// rejected in every case. A container occupies a level whether or not it has
/// contents — reaching an empty array/object at level `max_depth + 1` still
/// cost the recursive consumers that many frames to get there — so an
/// over-deep *empty* container is rejected exactly like a non-empty one.
///
/// The Python binding's `py_to_json_at` charges levels the same way (its guard
/// fires only on container branches, never scalar leaves), so the two paths
/// reject the identical shape; see [`crate::document::limits::MAX_YAML_DEPTH`]
/// for the canonical definition.
pub fn json_depth_exceeds(value: &serde_json::Value, max_depth: usize) -> bool {
    use serde_json::Value;
    // (value, depth) pairs; depth counts container levels entered.
    let mut stack: Vec<(&Value, usize)> = vec![(value, 0)];
    while let Some((v, depth)) = stack.pop() {
        match v {
            Value::Array(items) => {
                if depth + 1 > max_depth {
                    return true;
                }
                stack.extend(items.iter().map(|c| (c, depth + 1)));
            }
            Value::Object(map) => {
                if depth + 1 > max_depth {
                    return true;
                }
                stack.extend(map.values().map(|c| (c, depth + 1)));
            }
            _ => {}
        }
    }
    false
}

impl QuillValue {
    fn from_node(node: Node) -> Self {
        QuillValue {
            node,
            json: OnceLock::new(),
        }
    }

    /// Create a QuillValue from a YAML string
    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, serde_saphyr::Error> {
        let json_val: serde_json::Value = serde_saphyr::from_str(yaml_str)?;
        Ok(Self::from_json(json_val))
    }

    /// Get a reference to the value's JSON projection.
    ///
    /// The projection is materialized on first use and cached. It carries
    /// the data only; `!must_fill` markers are not represented in JSON.
    pub fn as_json(&self) -> &serde_json::Value {
        self.json.get_or_init(|| self.node.to_json())
    }

    /// Convert into the underlying JSON value (fill markers are dropped).
    pub fn into_json(self) -> serde_json::Value {
        match self.json.into_inner() {
            Some(json) => json,
            None => self.node.to_json(),
        }
    }

    /// Create a QuillValue from a JSON value, with every node `fill = false`.
    pub fn from_json(json_val: serde_json::Value) -> Self {
        let node = Node::from_json(&json_val);
        let json = OnceLock::new();
        // Seed the projection with the value we were handed so the common
        // render path doesn't re-lower it. This trades memory for speed:
        // until dropped, the data is held twice (the `node` tree plus the
        // cached JSON). Acceptable for the render-hot path; leaving the cache
        // empty here would halve memory at the cost of re-lowering on first
        // `as_json`.
        let _ = json.set(json_val);
        QuillValue { node, json }
    }

    /// String value.
    pub fn string(s: impl Into<String>) -> Self {
        Self::from_json(serde_json::Value::String(s.into()))
    }

    /// Integer value.
    pub fn integer(n: i64) -> Self {
        Self::from_json(serde_json::Value::Number(n.into()))
    }

    /// Boolean value.
    pub fn bool(b: bool) -> Self {
        Self::from_json(serde_json::Value::Bool(b))
    }

    /// Null value.
    pub fn null() -> Self {
        Self::from_json(serde_json::Value::Null)
    }

    /// Whether this value's root node carries the `!must_fill` marker.
    pub fn fill(&self) -> bool {
        self.node.fill
    }

    /// Paths (relative to this value's root) of every node carrying the
    /// `!must_fill` marker. The root, if filled, is reported as the empty
    /// path. The JSON projection carries no fill, so this is the only way to
    /// observe nested fill markers.
    pub fn fill_paths(&self) -> Vec<Vec<PathSegment>> {
        let mut out = Vec::new();
        let mut prefix = Vec::new();
        collect_fill_paths(&self.node, &mut prefix, &mut out);
        out
    }

    /// Fill paths *nested inside* this value — every [`fill_paths`](Self::fill_paths)
    /// entry except the empty (root) path. A root fill is carried separately as
    /// the `fill` flag on the owning field, so the wire / storage DTO record
    /// only the nested ones here.
    pub fn nonroot_fill_paths(&self) -> impl Iterator<Item = Vec<PathSegment>> {
        self.fill_paths().into_iter().filter(|p| !p.is_empty())
    }

    /// Set `fill = true` on the node at `path` (relative to the root).
    /// Returns `false` if the path does not resolve to a node.
    pub fn set_fill_at(&mut self, path: &[PathSegment]) -> bool {
        match node_at_mut(&mut self.node, path) {
            Some(n) => {
                n.fill = true;
                true
            }
            None => false,
        }
    }

    /// Whether the node at `path` (relative to the root) is a mapping.
    /// Used to reject `!must_fill` on object-valued nodes.
    pub fn is_object_at(&self, path: &[PathSegment]) -> bool {
        node_is_object(&self.node, path)
    }
}

/// Scalar conversions mirror [`serde_json::Value`]'s and produce `fill =
/// false` nodes (like [`QuillValue::from_json`]); a non-finite `f64` maps to
/// null, matching serde_json. These back the `impl Into<QuillValue>` mutator
/// parameters, so `card.store_field("qty", 3)` reads as written.
macro_rules! impl_from_scalar {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for QuillValue {
            fn from(v: $ty) -> Self {
                QuillValue::from_json(serde_json::Value::from(v))
            }
        }
    )*};
}
impl_from_scalar!(&str, String, bool, i32, i64, u32, u64, f64);

impl From<serde_json::Value> for QuillValue {
    fn from(v: serde_json::Value) -> Self {
        QuillValue::from_json(v)
    }
}

impl Deref for QuillValue {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        self.as_json()
    }
}

impl PartialEq for QuillValue {
    /// Two values are equal when their annotated trees (data **and** fill)
    /// are equal. The cached JSON projection is derived and not compared.
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Clone for QuillValue {
    fn clone(&self) -> Self {
        let json = OnceLock::new();
        if let Some(cached) = self.json.get() {
            let _ = json.set(cached.clone());
        }
        QuillValue {
            node: self.node.clone(),
            json,
        }
    }
}

impl std::fmt::Debug for QuillValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.node.fill {
            write!(f, "QuillValue(!must_fill {:?})", self.as_json())
        } else {
            write!(f, "QuillValue({:?})", self.as_json())
        }
    }
}

impl Serialize for QuillValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QuillValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = serde_json::Value::deserialize(deserializer)?;
        Ok(QuillValue::from_json(json))
    }
}

// Common delegating accessors, projected through the JSON view so existing
// consumers keep their serde_json-shaped API unchanged.
impl QuillValue {
    /// Check if the value is null
    pub fn is_null(&self) -> bool {
        self.as_json().is_null()
    }

    /// Get the value as a string reference
    pub fn as_str(&self) -> Option<&str> {
        self.as_json().as_str()
    }

    /// Get the value as a boolean
    pub fn as_bool(&self) -> Option<bool> {
        self.as_json().as_bool()
    }

    /// Get the value as an i64
    pub fn as_i64(&self) -> Option<i64> {
        self.as_json().as_i64()
    }

    /// Get the value as a u64
    pub fn as_u64(&self) -> Option<u64> {
        self.as_json().as_u64()
    }

    /// Get the value as an f64
    pub fn as_f64(&self) -> Option<f64> {
        self.as_json().as_f64()
    }

    /// Get the value as an array reference
    pub fn as_array(&self) -> Option<&Vec<serde_json::Value>> {
        self.as_json().as_array()
    }

    /// Get the value as an object reference
    pub fn as_object(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.as_json().as_object()
    }

    /// Get a field from an object by key, preserving the child's fill markers.
    pub fn get(&self, key: &str) -> Option<QuillValue> {
        match &self.node.kind {
            Kind::Object(entries) => entries.get(key).map(|n| QuillValue::from_node(n.clone())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_yaml_str() {
        let yaml_str = r#"
            title: Test Document
            author: John Doe
            count: 42
        "#;
        let quill_val = QuillValue::from_yaml_str(yaml_str).unwrap();

        assert_eq!(
            quill_val.get("title").as_ref().and_then(|v| v.as_str()),
            Some("Test Document")
        );
        assert_eq!(
            quill_val.get("author").as_ref().and_then(|v| v.as_str()),
            Some("John Doe")
        );
        assert_eq!(
            quill_val.get("count").as_ref().and_then(|v| v.as_i64()),
            Some(42)
        );
    }

    #[test]
    fn test_delegating_methods() {
        let quill_val = QuillValue::from_json(serde_json::json!({
            "name": "test",
            "count": 42,
            "active": true,
            "items": [1, 2, 3]
        }));

        assert_eq!(
            quill_val.get("name").as_ref().and_then(|v| v.as_str()),
            Some("test")
        );
        assert_eq!(
            quill_val.get("count").as_ref().and_then(|v| v.as_i64()),
            Some(42)
        );
        assert_eq!(
            quill_val.get("active").as_ref().and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(quill_val
            .get("items")
            .as_ref()
            .and_then(|v| v.as_array())
            .is_some());
    }

    #[test]
    fn test_yaml_custom_tags_ignored_at_value_level() {
        // At the raw `QuillValue::from_yaml_str` layer, custom YAML tags
        // (including `!must_fill`) pass through serde_saphyr which drops the
        // tag and returns the underlying scalar.  The tag is recovered at
        // the `Document` layer by `document::prescan`: see
        // `document::tests::lossiness_tests::custom_tags_lose_tag_but_keep_value`.
        let yaml_str = "memo_from: !must_fill 2d lt example";
        let quill_val = QuillValue::from_yaml_str(yaml_str).unwrap();

        assert_eq!(
            quill_val.get("memo_from").as_ref().and_then(|v| v.as_str()),
            Some("2d lt example")
        );
    }

    #[test]
    fn json_round_trips_through_the_tree() {
        // from_json → as_json must be identity, preserving object key order
        // (serde_json `preserve_order`) and number kinds.
        let original = serde_json::json!({
            "z": 1,
            "a": [true, "x", 3.5, null],
            "nested": { "k": 42 }
        });
        let qv = QuillValue::from_json(original.clone());
        assert_eq!(qv.as_json(), &original);

        // A value re-lowered from the tree (not the seeded cache) also matches.
        let relowered = QuillValue::from_node(qv.node.clone()).into_json();
        assert_eq!(relowered, original);
    }

    #[test]
    fn depth_check_counts_empty_containers() {
        use serde_json::json;

        // A container occupies a level even when empty. With `max_depth = 1`,
        // a single empty container is at the limit (accepted); a nested empty
        // container is one level past it (rejected) — same as if its innermost
        // slot held a non-empty container.
        assert!(!json_depth_exceeds(&json!([]), 1));
        assert!(!json_depth_exceeds(&json!({}), 1));
        assert!(json_depth_exceeds(&json!([[]]), 1));
        assert!(json_depth_exceeds(&json!({ "a": {} }), 1));

        // Regression: an empty container at the deepest level must not slip
        // past the bound. Build `[[[…[]…]]]` nested `n` levels with the
        // innermost array empty, iteratively so the test stays stack-safe.
        let deep_empty = |levels: usize| {
            let mut v = serde_json::Value::Array(Vec::new());
            for _ in 1..levels {
                v = serde_json::Value::Array(vec![v]);
            }
            v
        };
        // `levels == max_depth` is exactly at the limit (the empty array is the
        // last allowed level); `levels == max_depth + 1` is one past it.
        assert!(!json_depth_exceeds(&deep_empty(100), 100));
        assert!(json_depth_exceeds(&deep_empty(101), 100));
    }

    #[test]
    fn depth_check_counts_container_levels_not_the_scalar_leaf() {
        // The cutoff is container levels: a scalar leaf at the bottom is never
        // charged a level. `{"a":{"a":…{"a":1}}}` with exactly `max_depth`
        // objects is at the limit; one more object is past it. The Python
        // binding's `py_to_json_at` pins the same boundary (test
        // `test_depth_bound_matches_core_container_levels`), so the two paths
        // reject the identical shape.
        let scalar_terminated = |levels: usize| {
            let mut v = serde_json::json!(1);
            for _ in 0..levels {
                v = serde_json::json!({ "a": v });
            }
            v
        };
        assert!(!json_depth_exceeds(&scalar_terminated(100), 100));
        assert!(json_depth_exceeds(&scalar_terminated(101), 100));

        // A non-empty container leaf lands at the same boundary: the deepest
        // container — not its contents — is what occupies the last level.
        let container_terminated = |levels: usize| {
            let mut v = serde_json::json!([1, 2, 3]);
            for _ in 1..levels {
                v = serde_json::json!({ "a": v });
            }
            v
        };
        assert!(!json_depth_exceeds(&container_terminated(100), 100));
        assert!(json_depth_exceeds(&container_terminated(101), 100));
    }

    #[test]
    fn fill_marker_rides_on_the_node_not_the_json() {
        let filled = || {
            let mut qv = QuillValue::string("draft");
            assert!(qv.set_fill_at(&[]));
            qv
        };
        let qv = filled();
        assert!(qv.fill());
        // Projection is fill-free and equal to the plain scalar.
        assert_eq!(qv.as_json(), &serde_json::json!("draft"));
        // Equality is fill-sensitive.
        assert_ne!(qv, QuillValue::string("draft"));
        assert_eq!(qv, filled());
    }
}
