//! Card-yaml block metadata: the `#@`-prefixed system-metadata header.
//!
//! Every `~~~card-yaml` block may carry a leading run of `#@key: value` lines.
//! These are **system metadata** — reserved keys (`#@quill`, `#@kind`, `#@id`,
//! …) kept out of the YAML payload's user field set. The `#@` prefix also
//! keeps them invisible to a plain YAML parser (a `#` line is a comment).
//!
//! System metadata carries no parser semantics beyond `#@quill` on the root
//! block, which binds the document to a quill. Every other entry — including
//! `#@kind` and `#@id` — is opaque metadata, carried through round-trip
//! unchanged.

use indexmap::IndexMap;

use crate::error::ParseError;

/// Ordered `#@`-metadata of a single card-yaml block.
///
/// Keys are stored without the `#@` prefix (`quill`, `kind`, `id`, …) and
/// values are the raw line text after the `:`. Insertion order is preserved
/// so emission round-trips byte-stably.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemMeta {
    entries: IndexMap<String, String>,
}

impl SystemMeta {
    /// Create an empty metadata set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a metadata value by key (key without the `#@` prefix).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Returns `true` if a metadata entry with this key is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Insert or update a metadata entry. Existing keys keep their position.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        self.entries.insert(key.into(), value.into())
    }

    /// Remove a metadata entry, returning its value if present. Preserves the
    /// order of the remaining entries.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.shift_remove(key)
    }

    /// Ordered iterator over `(key, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> + '_ {
        self.entries.iter()
    }

    /// Returns `true` if there are no metadata entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of metadata entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The `#@quill` quill reference, if present.
    pub fn quill(&self) -> Option<&str> {
        self.get("quill")
    }

    /// The `#@kind` card kind, if present.
    pub fn kind(&self) -> Option<&str> {
        self.get("kind")
    }

    /// The `#@id` opaque identifier, if present.
    pub fn id(&self) -> Option<&str> {
        self.get("id")
    }
}

/// Parse a `#@`-prefixed metadata line into its `(key, value)` pair.
///
/// `#@quill: example@0.1.0` parses to `("quill", "example@0.1.0")`. Returns
/// `None` when `line` is not a `#@` metadata line (no `#@` prefix, or no `:`
/// separator).
pub(super) fn parse_meta_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("#@")?;
    let colon = rest.find(':')?;
    let key = rest[..colon].trim().to_string();
    let value = rest[colon + 1..].trim().to_string();
    Some((key, value))
}

/// Validate a card-yaml block's YAML payload.
///
/// Rejects the reserved wire-format keys (`QUILL`, `CARD`, `BODY`, `CARDS`)
/// appearing as user-defined fields — they would collide with
/// [`crate::Document::to_plate_json`]'s output. The parsed value is returned
/// unchanged.
pub(super) fn validate_payload_yaml(
    parsed: serde_json::Value,
) -> Result<Option<serde_json::Value>, ParseError> {
    if let Some(mapping) = parsed.as_object() {
        for reserved in ["QUILL", "CARD", "BODY", "CARDS"] {
            if mapping.contains_key(reserved) {
                return Err(ParseError::InvalidStructure(format!(
                    "Reserved field name '{}' cannot be used in a card-yaml block",
                    reserved
                )));
            }
        }
    }
    Ok(Some(parsed))
}

/// Validate a card kind / tag name follows the pattern `[a-z_][a-z0-9_]*`.
pub(super) fn is_valid_tag_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();
    let first = chars.next().unwrap();

    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }

    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }

    true
}
