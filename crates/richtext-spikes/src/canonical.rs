//! Byte-deterministic JSON for [`RichText`]. The seam (Spike C) requires the
//! same value to serialize to identical bytes every time and across processes,
//! so content hashing (DOCUMENT_STORAGE.md § determinism) stays stable.
//!
//! Three nondeterminism sources and their fixes:
//! 1. mark discovery order (parser walk) → [`RichText::canonicalize_marks`]
//!    sorts before serializing;
//! 2. object key order in island `props` → recursively sorted here;
//! 3. float formatting → not exercised (props in the spike are strings/ints).

use crate::model::RichText;
use serde_json::Value;

/// Recursively sort every object's keys, so a `serde_json::Value` serializes
/// identically regardless of insertion order (the `preserve_order` feature is
/// on workspace-wide, so insertion order would otherwise leak into the bytes).
fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, val)| (k.clone(), sort_keys(val)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sort_keys).collect()),
        other => other.clone(),
    }
}

/// The canonical byte form of a `RichText`. Clones so the argument is not
/// mutated; sorts marks/islands; sorts every nested object key; emits compact
/// JSON. Equal `RichText` values (data-equal, ignoring mark/island order)
/// produce identical bytes.
pub fn canonical_json(rt: &RichText) -> String {
    let mut rt = rt.clone();
    rt.canonicalize_marks();
    let value = serde_json::to_value(&rt).expect("RichText serializes");
    serde_json::to_string(&sort_keys(&value)).expect("Value serializes")
}

/// A stable content hash of the canonical bytes. FNV-1a — not cryptographic,
/// just a fast deterministic digest to demonstrate byte-identity across two
/// independent serializations.
pub fn content_hash(rt: &RichText) -> u64 {
    let bytes = canonical_json(rt);
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
