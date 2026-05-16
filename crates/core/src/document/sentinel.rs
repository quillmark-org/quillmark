//! QUILL sentinel extraction and reserved-name validation.
//!
//! Implements the sentinel rules from MARKDOWN.md §4.2 and the reserved-name
//! checks from spec §3.

use crate::error::ParseError;
use crate::version::QuillReference;

/// Validate tag name follows pattern [a-z_][a-z0-9_]*
pub(crate) fn is_valid_tag_name(name: &str) -> bool {
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

/// Extracts the first non-blank, non-comment line of `content` and, if it
/// starts with a `[A-Za-z][A-Za-z0-9_]*` identifier followed by `:`, returns
/// that identifier. Any leading spaces/tabs on that line are ignored
/// (YAML indentation-tolerant). YAML `#` comment lines are skipped so the
/// sentinel rule is indifferent to banner-style comments above the key.
pub(super) fn first_content_key(content: &str) -> Option<&str> {
    let first = content
        .lines()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))?;
    let trimmed = first.trim_start_matches([' ', '\t']);
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b':' {
        Some(&trimmed[..i])
    } else {
        None
    }
}

/// Clone `mapping`, strip `key`, return the remainder (or `None` if empty).
///
/// `serde_json::Map` with the `preserve_order` feature (enabled in this
/// workspace) is backed by `indexmap::IndexMap`; its default `remove` is
/// `swap_remove` (O(1) but order-disrupting). We use `shift_remove` so that
/// the surviving keys keep their source order.
fn strip_key(
    mapping: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<serde_json::Value> {
    let mut m = mapping.clone();
    m.shift_remove(key);
    (!m.is_empty()).then(|| serde_json::Value::Object(m))
}

/// Extract the `QUILL` sentinel (frontmatter only) and the remaining fields
/// from a parsed-YAML mapping. Returns `(quill_ref, yaml_without_sentinel)`.
///
/// A leaf's kind is carried by the fence info string (`MARKDOWN.md §3.2`), so
/// `KIND` no longer participates in sentinel extraction: it joins `BODY` and
/// `LEAVES` as an output-only reserved key, and supplying it as an input body
/// key is a hard parse error in both frontmatter and leaves.
pub(super) fn extract_sentinels(
    parsed: serde_json::Value,
    is_frontmatter: bool,
) -> Result<(Option<String>, Option<serde_json::Value>), ParseError> {
    let Some(mapping) = parsed.as_object() else {
        // Non-mapping (scalar/sequence); pass through — upstream will reject
        // if a frontmatter/leaf mapping was expected.
        return Ok((None, Some(parsed)));
    };

    // Output-only reserved keys (spec §3): the parser populates these, so an
    // author supplying any of them as an input field is a hard parse error.
    for reserved in ["BODY", "LEAVES", "KIND"] {
        if mapping.contains_key(reserved) {
            return Err(ParseError::InvalidStructure(format!(
                "Reserved field name '{}' cannot be used as an input field",
                reserved
            )));
        }
    }

    if is_frontmatter {
        if let Some(quill_val) = mapping.get("QUILL") {
            let quill_str = quill_val.as_str().ok_or("QUILL value must be a string")?;
            quill_str.parse::<QuillReference>().map_err(|e| {
                ParseError::InvalidStructure(format!(
                    "Invalid QUILL reference '{}': {}",
                    quill_str, e
                ))
            })?;
            return Ok((Some(quill_str.to_string()), strip_key(mapping, "QUILL")));
        }
    }
    Ok((None, Some(parsed)))
}
