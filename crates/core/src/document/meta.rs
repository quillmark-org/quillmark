//! Card-yaml block metadata: the `#@`-prefixed system-metadata header.
//!
//! Every `~~~card-yaml` block may carry a leading run of `#@key: value` lines.
//! These are **system metadata** drawn from a closed set of three reserved
//! keys — `#@quill`, `#@kind`, `#@id`. Any other `#@key` is a parse error.
//! The `#@` prefix keeps them invisible to a plain YAML parser (a `#` line is
//! a comment).
//!
//! `#@quill` on the root block binds the document to a quill; `#@kind` is
//! name-validated against `[a-z_][a-z0-9_]*` at parse time. `#@id` is opaque
//! metadata, carried through round-trip unchanged.

use std::str::FromStr;

use crate::error::ParseError;
use crate::version::QuillReference;

/// Typed `#@`-metadata of a single card-yaml block.
///
/// The `#@` header is a **closed set** of three optional keys; an unknown
/// `#@key` is rejected at parse time. `#@quill` is parsed into a typed
/// [`QuillReference`] as the block is read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CardMetadata {
    /// The `#@quill` reference. Required on the document's root block and
    /// rejected on composable cards (see `assemble`); `None` on every card
    /// in a successfully parsed [`crate::Document`].
    pub quill: Option<QuillReference>,
    /// The `#@kind` card kind, if the block declares one. Validated against
    /// `[a-z_][a-z0-9_]*` at parse time.
    pub kind: Option<String>,
    /// The `#@id` opaque identifier, if the block declares one.
    pub id: Option<String>,
}

/// Parse a block's `#@` header lines into a typed [`CardMetadata`].
///
/// Header lines may appear in any order. The accepted keys are the closed set
/// `{quill, kind, id}`. A malformed `#@` line, an unknown `#@key`, a duplicate
/// key, an invalid `#@quill` reference, or a `#@kind` that does not match
/// `[a-z_][a-z0-9_]*` is a parse error.
pub(super) fn parse_meta_header(header: &[&str]) -> Result<CardMetadata, ParseError> {
    let mut meta = CardMetadata::default();
    for line in header {
        let (key, value) = parse_meta_line(line).ok_or_else(|| {
            ParseError::InvalidStructure(format!(
                "Malformed `#@` metadata line `{}` — expected `#@<key>: <value>`",
                line.trim()
            ))
        })?;
        match key.as_str() {
            "quill" => {
                if meta.quill.is_some() {
                    return Err(duplicate_meta_error("quill"));
                }
                let reference = QuillReference::from_str(&value).map_err(|e| {
                    ParseError::InvalidStructure(format!(
                        "Invalid #@quill reference '{}': {}",
                        value, e
                    ))
                })?;
                meta.quill = Some(reference);
            }
            "kind" => {
                if meta.kind.is_some() {
                    return Err(duplicate_meta_error("kind"));
                }
                if !is_valid_kind_name(&value) {
                    return Err(ParseError::InvalidStructure(format!(
                        "Invalid `#@kind` value '{}' — a card kind must match \
                         `[a-z_][a-z0-9_]*`",
                        value
                    )));
                }
                meta.kind = Some(value);
            }
            "id" => {
                if meta.id.is_some() {
                    return Err(duplicate_meta_error("id"));
                }
                meta.id = Some(value);
            }
            other => {
                return Err(ParseError::InvalidStructure(format!(
                    "Unknown `#@{}` system-metadata key — the card-yaml header \
                     accepts only `#@quill`, `#@kind`, and `#@id`",
                    other
                )));
            }
        }
    }
    Ok(meta)
}

fn duplicate_meta_error(key: &str) -> ParseError {
    ParseError::InvalidStructure(format!(
        "Duplicate `#@{}` system-metadata entry in one card-yaml block",
        key
    ))
}

/// Parse a `#@`-prefixed metadata line into its `(key, value)` pair.
///
/// `#@quill: example@0.1.0` parses to `("quill", "example@0.1.0")`. A trailing
/// inline comment (` # …` to end of line) is dropped from the value, matching
/// YAML's own comment rule:
///
/// ```text
/// #@quill: example@0.1.0  # bound at build time
/// →                ↳ key="quill", value="example@0.1.0"
/// ```
///
/// A `#` is only treated as a comment marker when preceded by whitespace
/// (or at the start of the value), so `#@id: abc#def` keeps `abc#def` intact.
///
/// Returns `None` when `line` is not a `#@` metadata line (no `#@` prefix,
/// or no `:` separator).
fn parse_meta_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("#@")?;
    let colon = rest.find(':')?;
    let key = rest[..colon].trim().to_string();
    let value_raw = &rest[colon + 1..];
    let value = strip_trailing_comment(value_raw).trim().to_string();
    Some((key, value))
}

/// Strip a YAML-style trailing comment from a `#@` value.
///
/// A `#` starts a comment when it is the first non-whitespace character of
/// the value or is preceded by whitespace. Other `#`s are kept verbatim.
fn strip_trailing_comment(s: &str) -> &str {
    let mut prev_was_ws = true;
    for (i, c) in s.char_indices() {
        if c == '#' && prev_was_ws {
            return &s[..i];
        }
        prev_was_ws = c == ' ' || c == '\t';
    }
    s
}

/// Validate a card-yaml block's YAML payload.
///
/// Rejects the reserved wire-format keys (`QUILL`, `CARD`, `BODY`, `CARDS`)
/// appearing as user-defined fields — they would collide with
/// [`crate::Document::to_plate_json`]'s output. The parsed value is returned
/// unchanged.
pub(super) fn validate_payload_yaml(
    parsed: serde_json::Value,
) -> Result<serde_json::Value, ParseError> {
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
    Ok(parsed)
}

/// Validate a card kind name follows the pattern `[a-z_][a-z0-9_]*`.
pub(super) fn is_valid_kind_name(name: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_comment_stripped_from_quill_value() {
        let (k, v) = parse_meta_line("#@quill: foo@0.1  # bound at build").unwrap();
        assert_eq!(k, "quill");
        assert_eq!(v, "foo@0.1");
    }

    #[test]
    fn trailing_comment_stripped_from_kind_value() {
        let (k, v) = parse_meta_line("#@kind: main # the root").unwrap();
        assert_eq!(k, "kind");
        assert_eq!(v, "main");
    }

    #[test]
    fn hash_inside_value_is_kept_when_not_preceded_by_whitespace() {
        // `abc#def` is a single token — `#` has no whitespace before it.
        let (_, v) = parse_meta_line("#@id: abc#def").unwrap();
        assert_eq!(v, "abc#def");
    }

    #[test]
    fn hash_at_start_of_value_is_a_comment() {
        // After the colon-space, `#` is the first non-whitespace character —
        // matches YAML's rule for "comment starts at column 0 of value".
        let (k, v) = parse_meta_line("#@id: # gone").unwrap();
        assert_eq!(k, "id");
        assert_eq!(v, "");
    }

    #[test]
    fn no_trailing_comment_leaves_value_unchanged() {
        let (_, v) = parse_meta_line("#@quill: foo@0.1").unwrap();
        assert_eq!(v, "foo@0.1");
    }
}
