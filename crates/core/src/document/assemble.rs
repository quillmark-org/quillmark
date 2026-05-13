//! Assembly of fences and sentinels into a [`Document`].
//!
//! This module contains the top-level parsing glue: it calls the fence scanner,
//! extracts sentinels, and assembles a typed [`Document`] from the pieces.

use std::str::FromStr;

use crate::error::ParseError;
use crate::value::QuillValue;
use crate::version::QuillReference;
use crate::Diagnostic;

use super::fences::find_metadata_blocks;
use super::frontmatter::{Frontmatter, FrontmatterItem};
use super::prescan::{prescan_fence_content, NestedComment, PreItem};
use super::sentinel::extract_sentinels;
use super::{Document, Leaf, Sentinel};

/// Strip exactly one F2 structural separator from the tail of a body slice.
///
/// The F2 rule (`MARKDOWN.md §3`) requires a blank line immediately above
/// every metadata fence. When a body is followed by another fence, the raw
/// slice ends with that blank line's terminator — exactly one `\n` or
/// `\r\n`. This helper strips that single line ending so stored bodies
/// contain only authored content. The emitter re-adds the separator on
/// output via `ensure_blank_line_before_fence`.
///
/// Stripping more than one line ending (as the WASM binding's former
/// `trim_body` did) would silently drop content-meaningful trailing
/// newlines — e.g. a body that ends with a fenced code block's closing
/// newline.
fn strip_f2_separator(body: &str) -> &str {
    if let Some(rest) = body.strip_suffix("\r\n") {
        rest
    } else if let Some(rest) = body.strip_suffix('\n') {
        rest
    } else {
        body
    }
}

/// An intermediate representation of one `---…---` metadata block.
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize,                          // Position of opening "---"
    pub(super) end: usize,                            // Position after closing "---\n"
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML as JSON (None if empty or parse failed)
    pub(super) tag: Option<String>,                   // Field name from KIND key
    pub(super) quill_ref: Option<String>,             // Quill reference from QUILL key
    /// Pre-scan items (comments + fill-tagged field keys) in source order.
    pub(super) pre_items: Vec<PreItem>,
    /// Pre-scan nested comments (with structural paths).
    pub(super) pre_nested_comments: Vec<NestedComment>,
    /// Pre-scan warnings (unknown-tag strips, ...).
    pub(super) pre_warnings: Vec<Diagnostic>,
}

/// Creates serde_saphyr Options with security budgets configured.
///
/// Uses MAX_YAML_DEPTH from limits.rs to limit nesting depth at the parser level,
/// which is more robust than heuristic-based pre-parse checks.
fn yaml_parse_options() -> serde_saphyr::Options {
    let budget = serde_saphyr::Budget {
        max_depth: super::limits::MAX_YAML_DEPTH,
        ..Default::default()
    };
    serde_saphyr::Options {
        budget: Some(budget),
        ..Default::default()
    }
}

/// Process YAML content for a recognized metadata fence and build a
/// `MetadataBlock`. `content_start` is the byte position immediately after
/// the opening fence line; `content_end` is the byte position at the start
/// of the closing fence line. Returns errors per spec §9.
pub(super) fn build_block(
    markdown: &str,
    abs_pos: usize,
    content_start: usize,
    content_end: usize,
    block_end: usize,
    block_index: usize,
) -> Result<MetadataBlock, ParseError> {
    let raw_content = &markdown[content_start..content_end];

    // Check YAML size limit (spec §8)
    if raw_content.len() > crate::error::MAX_YAML_SIZE {
        return Err(ParseError::InputTooLarge {
            size: raw_content.len(),
            max: crate::error::MAX_YAML_SIZE,
        });
    }

    // Run the pre-scan to extract top-level comments, `!fill` markers,
    // and warn on unsupported tags / nested comments.
    let pre = prescan_fence_content(raw_content);

    if let Some(err) = pre.fill_target_errors.first() {
        return Err(ParseError::InvalidStructure(err.clone()));
    }

    let content = pre.cleaned_yaml.trim().to_string();
    let (tag, quill_ref, yaml_value) = if content.is_empty() {
        (None, None, None)
    } else {
        match serde_saphyr::from_str_with_options::<serde_json::Value>(
            &content,
            yaml_parse_options(),
        ) {
            Ok(parsed) => extract_sentinels(parsed, markdown, abs_pos, block_index)?,
            Err(e) => {
                let line = markdown[..abs_pos].lines().count() + 1;
                return Err(ParseError::YamlErrorWithLocation {
                    message: e.to_string(),
                    line,
                    block_index,
                });
            }
        }
    };

    // Per-fence field-count check (spec §8, §6.1 of GAP analysis)
    if let Some(serde_json::Value::Object(ref map)) = yaml_value {
        // Add +1 for QUILL (stripped) or KIND (stripped) so the cap matches
        // what the user wrote, not what's left after sentinel extraction.
        let sentinel_extra = if quill_ref.is_some() || tag.is_some() {
            1
        } else {
            0
        };
        if map.len() + sentinel_extra > crate::error::MAX_FIELD_COUNT {
            return Err(ParseError::InputTooLarge {
                size: map.len() + sentinel_extra,
                max: crate::error::MAX_FIELD_COUNT,
            });
        }
    }

    Ok(MetadataBlock {
        start: abs_pos,
        end: block_end,
        yaml_value,
        tag,
        quill_ref,
        pre_items: pre.items,
        pre_nested_comments: pre.nested_comments,
        pre_warnings: pre.warnings,
    })
}

/// Construct the top-level "missing QUILL" error message. If we saw a
/// first-fence F1 failure, tailor the message to the actual key found:
/// a case-insensitive match to `QUILL` is a typo, anything else is a
/// key-ordering problem.
fn missing_quill_message(first_fence_issue: Option<(String, usize)>) -> String {
    match first_fence_issue {
        Some((actual, line)) if actual.eq_ignore_ascii_case("QUILL") => format!(
            "Missing required QUILL field. Found `{}:` at line {} — expected `QUILL:` (uppercase). Change the key to `QUILL` to register this fence as the document frontmatter.",
            actual, line
        ),
        Some((actual, line)) => format!(
            "Missing required QUILL field. The first YAML key in the frontmatter must be `QUILL:` (found `{}:` at line {}). Reorder the frontmatter so `QUILL: <name>` is the first key.",
            actual, line
        ),
        None => "Missing required QUILL field. Add `QUILL: <name>` to the frontmatter.".to_string(),
    }
}

/// Decompose markdown, discarding warnings. Test- and `from_markdown`-facing.
pub(super) fn decompose(markdown: &str) -> Result<Document, crate::error::ParseError> {
    decompose_with_warnings(markdown).map(|(doc, _)| doc)
}

/// Decompose markdown into a typed [`Document`], returning any non-fatal warnings
/// collected during fence scanning.
pub(super) fn decompose_with_warnings(
    markdown: &str,
) -> Result<(Document, Vec<Diagnostic>), crate::error::ParseError> {
    // Strip a leading UTF-8 BOM if present. Editors on Windows (Notepad, some
    // Word exports) prepend `\u{FEFF}` which otherwise defeats F2 because the
    // first line no longer matches `---`.
    let markdown = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);

    // Empty / whitespace-only input gets a tailored message. The default
    // missing-QUILL error reads as if the user supplied a partial document
    // missing only QUILL, which is misleading when there's no document at all.
    if markdown.trim().is_empty() {
        return Err(crate::error::ParseError::EmptyInput(
            "Empty markdown input cannot be parsed as a Quillmark Document. \
             Provide at least a QUILL frontmatter field: `QUILL: <name>`."
                .to_string(),
        ));
    }

    // Check input size limit
    if markdown.len() > crate::error::MAX_INPUT_SIZE {
        return Err(crate::error::ParseError::InputTooLarge {
            size: markdown.len(),
            max: crate::error::MAX_INPUT_SIZE,
        });
    }

    // Find all metadata blocks. F1/F2 already guarantee that block 0 carries
    // QUILL and that every subsequent block carries KIND.
    let (blocks, warnings, first_fence_issue) = find_metadata_blocks(markdown)?;

    if blocks.is_empty() {
        return Err(crate::error::ParseError::MissingQuillField(
            missing_quill_message(first_fence_issue),
        ));
    }

    // Block 0 is always the QUILL frontmatter (F1 guarantee).
    let frontmatter_block = &blocks[0];
    let quill_tag = frontmatter_block.quill_ref.clone().ok_or_else(|| {
        ParseError::MissingQuillField(
            "Missing required QUILL field. Add `QUILL: <name>` to the frontmatter.".to_string(),
        )
    })?;

    // Build frontmatter item list (YAML content with QUILL stripped).
    //
    // The pre-scan captured top-level comments and `!fill` markers in source
    // order; serde_saphyr produced the parsed values. We iterate the pre-scan
    // order and pull each field's value from the parsed map.
    let frontmatter = build_frontmatter_from_pre_and_parsed(
        &frontmatter_block.pre_items,
        &frontmatter_block.pre_nested_comments,
        &frontmatter_block.yaml_value,
    )?;
    // Surface pre-scan warnings (nested-comment drops, unsupported tags).
    let mut warnings = warnings;
    for w in &frontmatter_block.pre_warnings {
        warnings.push(w.clone());
    }

    // Global body: between end of frontmatter (block 0) and start of the
    // first KIND block (or EOF).
    //
    // When a fence follows, the body slice ends with the F2 blank-line
    // terminator — strip it so stored bodies contain only authored content.
    // The emitter re-derives the separator on output (see `emit.rs`'s
    // `ensure_blank_line_before_fence`).
    let body_start = blocks[0].end;
    let first_card_block = blocks.iter().skip(1).find(|b| b.tag.is_some());
    let (body_end, body_is_followed_by_fence) = match first_card_block {
        Some(b) => (b.start, true),
        None => (markdown.len(), false),
    };
    let global_body_raw = &markdown[body_start..body_end];
    let global_body = if body_is_followed_by_fence {
        strip_f2_separator(global_body_raw).to_string()
    } else {
        global_body_raw.to_string()
    };

    // Parse tagged blocks (KIND blocks) into typed Leaves.
    let mut leaves: Vec<Leaf> = Vec::new();
    for (idx, block) in blocks.iter().enumerate() {
        if let Some(ref tag_name) = block.tag {
            // Build the leaf's typed frontmatter from pre-scan + parsed YAML.
            let leaf_frontmatter = build_frontmatter_from_pre_and_parsed(
                &block.pre_items,
                &block.pre_nested_comments,
                &block.yaml_value,
            )
            .map_err(|e| match e {
                ParseError::InvalidStructure(msg) => ParseError::InvalidStructure(format!(
                    "Invalid YAML in leaf block '{}': {}",
                    tag_name, msg
                )),
                other => other,
            })?;
            for w in &block.pre_warnings {
                warnings.push(w.clone());
            }

            // Leaf body: between this block's end and the next block's start (or EOF).
            let leaf_body_start = block.end;
            let has_next_block = idx + 1 < blocks.len();
            let leaf_body_end = if has_next_block {
                blocks[idx + 1].start
            } else {
                markdown.len()
            };
            let leaf_body_raw = &markdown[leaf_body_start..leaf_body_end];
            let leaf_body = if has_next_block {
                strip_f2_separator(leaf_body_raw).to_string()
            } else {
                leaf_body_raw.to_string()
            };

            leaves.push(Leaf::new_with_sentinel(
                Sentinel::Leaf(tag_name.clone()),
                leaf_frontmatter,
                leaf_body,
            ));
        }
    }

    let quill_ref = QuillReference::from_str(&quill_tag).map_err(|e| {
        ParseError::InvalidStructure(format!("Invalid QUILL tag '{}': {}", quill_tag, e))
    })?;

    let main = Leaf::new_with_sentinel(Sentinel::Main(quill_ref), frontmatter, global_body);
    let doc = Document::from_main_and_leaves(main, leaves, warnings.clone());

    Ok((doc, warnings))
}

/// Build a [`Frontmatter`] from the pre-scan items and the parsed YAML
/// mapping (with sentinel keys already stripped).
///
/// The pre-scan defined source order for fields and comments; the parsed
/// YAML defined the typed value for each key. We walk pre-scan order,
/// pulling each field's value from `parsed`. Any field that the pre-scan
/// didn't catch (e.g. it used a YAML key form the pre-scan doesn't
/// recognise — exotic identifier, flow-mapping syntax, etc.) is appended at
/// the end of the item list in parsed-map order so we never drop values.
fn build_frontmatter_from_pre_and_parsed(
    pre_items: &[PreItem],
    pre_nested_comments: &[NestedComment],
    yaml_value: &Option<serde_json::Value>,
) -> Result<Frontmatter, ParseError> {
    let mapping = match yaml_value {
        Some(serde_json::Value::Object(map)) => map.clone(),
        Some(serde_json::Value::Null) | None => serde_json::Map::new(),
        Some(_) => {
            return Err(ParseError::InvalidStructure(
                "expected a mapping".to_string(),
            ));
        }
    };

    let mut items: Vec<FrontmatterItem> = Vec::new();
    let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();
    // When a QUILL/KIND sentinel field is skipped, its trailing inline comment
    // loses its host field. The emitter can only round-trip an inline comment
    // on the sentinel line when it sits at items[0] (sentinel-preview path).
    // If other items already precede it, it will never reach items[0] and will
    // become an orphan that degrades differently on each emit → broken
    // idempotency. Demote it to own-line at parse time in that case.
    let mut after_stripped_sentinel = false;

    for pre in pre_items {
        match pre {
            PreItem::Comment { text, inline } => {
                let demote = after_stripped_sentinel && *inline && !items.is_empty();
                after_stripped_sentinel = false;
                items.push(FrontmatterItem::Comment {
                    text: text.clone(),
                    inline: *inline && !demote,
                });
            }
            PreItem::Field { key, fill } => {
                // QUILL / KIND sentinel keys are stripped from the parsed
                // map by `extract_sentinels`; skip them in the item list.
                if key == "QUILL" || key == "KIND" {
                    after_stripped_sentinel = true;
                    continue;
                }
                after_stripped_sentinel = false;
                if let Some(value) = mapping.get(key).cloned() {
                    // `!fill` applies to scalars and sequences. Mappings
                    // are rejected because top-level `type: object` is
                    // unsupported by Quillmark's schema.
                    if *fill && value.is_object() {
                        return Err(ParseError::InvalidStructure(format!(
                            "`!fill` on key `{}` targets a mapping; `!fill` is supported on scalars and sequences only",
                            key
                        )));
                    }
                    items.push(FrontmatterItem::Field {
                        key: key.clone(),
                        value: QuillValue::from_json(value),
                        fill: *fill,
                    });
                    consumed.insert(key.clone());
                }
                // If the key isn't in the parsed map, it was dropped by
                // YAML parsing (shouldn't happen for well-formed input);
                // silently skip.
            }
        }
    }

    // Append any parsed-map keys that the pre-scan didn't capture.
    for (key, value) in &mapping {
        if consumed.contains(key) {
            continue;
        }
        items.push(FrontmatterItem::Field {
            key: key.clone(),
            value: QuillValue::from_json(value.clone()),
            fill: false,
        });
    }

    Ok(Frontmatter::from_items_with_nested(
        items,
        pre_nested_comments.to_vec(),
    ))
}
