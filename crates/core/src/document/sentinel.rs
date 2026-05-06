//! QUILL / CARD sentinel extraction and reserved-name validation.
//!
//! Implements the sentinel rules from MARKDOWN.md §4.2 and the reserved-name
//! checks from spec §3.

use crate::error::ParseError;
use crate::version::QuillReference;

/// Validate tag name follows pattern [a-z_][a-z0-9_]*
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

/// Extract `QUILL` / `CARD` sentinels and remaining fields from a parsed-YAML
/// mapping. Returns `(tag, quill_ref, yaml_without_sentinel)`.
#[allow(clippy::type_complexity)]
pub(super) fn extract_sentinels(
    parsed: serde_json::Value,
    _markdown: &str,
    _abs_pos: usize,
    _block_index: usize,
) -> Result<(Option<String>, Option<String>, Option<serde_json::Value>), ParseError> {
    let Some(mapping) = parsed.as_object() else {
        // Non-mapping (scalar/sequence); keep as-is — upstream will reject if
        // it's a frontmatter/card mapping was expected.
        return Ok((None, None, Some(parsed)));
    };

    let has_quill = mapping.contains_key("QUILL");
    let has_card = mapping.contains_key("CARD");

    if has_quill && has_card {
        return Err(ParseError::InvalidStructure(
            "Cannot specify both QUILL and CARD in the same block".to_string(),
        ));
    }

    // Reserved keys (BODY, CARDS) — spec §3
    for reserved in ["BODY", "CARDS"] {
        if mapping.contains_key(reserved) {
            return Err(ParseError::InvalidStructure(format!(
                "Reserved field name '{}' cannot be used in YAML frontmatter",
                reserved
            )));
        }
    }

    if has_quill {
        let quill_str = mapping
            .get("QUILL")
            .unwrap()
            .as_str()
            .ok_or("QUILL value must be a string")?;
        quill_str.parse::<QuillReference>().map_err(|e| {
            ParseError::InvalidStructure(format!("Invalid QUILL reference '{}': {}", quill_str, e))
        })?;
        let mut new_map = mapping.clone();
        // Use `shift_remove` (order-preserving, O(n)) rather than the
        // default `remove` which is `swap_remove` (O(1), disrupts order).
        // serde_json::Map with `preserve_order` uses indexmap internally;
        // its `.remove()` calls `swap_remove`, not `shift_remove`, so we
        // call `shift_remove` explicitly to maintain insertion order.
        new_map.shift_remove("QUILL");
        let new_val = if new_map.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(new_map))
        };
        Ok((None, Some(quill_str.to_string()), new_val))
    } else if has_card {
        let field_name = mapping
            .get("CARD")
            .unwrap()
            .as_str()
            .ok_or("CARD value must be a string")?;
        if !is_valid_tag_name(field_name) {
            return Err(ParseError::InvalidStructure(format!(
                "Invalid card field name '{}': must match pattern [a-z_][a-z0-9_]*",
                field_name
            )));
        }
        let mut new_map = mapping.clone();
        new_map.shift_remove("CARD");
        let new_val = if new_map.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(new_map))
        };
        Ok((Some(field_name.to_string()), None, new_val))
    } else {
        Ok((None, None, Some(parsed)))
    }
}
