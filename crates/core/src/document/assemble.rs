//! Assembly of card-yaml blocks into a [`Document`].
//!
//! This module contains the top-level parsing glue: it calls the fence
//! scanner, parses each block's `#@` system-metadata header, and assembles a
//! typed [`Document`] from the pieces.

use crate::error::ParseError;
use crate::value::QuillValue;
use crate::Diagnostic;

use super::fences::find_metadata_blocks;
use super::meta::{parse_meta_header, validate_payload_yaml, CardMetadata};
use super::payload::{Payload, PayloadItem};
use super::prescan::{prescan_fence_content, NestedComment, PreItem};
use super::{Card, Document};

/// Strip exactly one structural separator from the tail of a body slice.
///
/// Every card-yaml block requires a blank line immediately above it. When a
/// body is followed by another block, the raw slice ends with that blank
/// line's terminator — exactly one `\n` or `\r\n`. This helper strips that
/// single line ending so stored bodies contain only authored content. The
/// emitter re-adds the separator on output via `ensure_blank_before_fence`.
fn strip_blank_separator(body: &str) -> &str {
    if let Some(rest) = body.strip_suffix("\r\n") {
        rest
    } else if let Some(rest) = body.strip_suffix('\n') {
        rest
    } else {
        body
    }
}

/// An intermediate representation of one `~~~card-yaml … ~~~` block.
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize, // Position of the opening `~~~card-yaml`
    pub(super) end: usize,   // Position after the closing `~~~`
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML payload as JSON
    pub(super) meta: CardMetadata, // The block's typed `#@` system metadata
    /// Pre-scan items (comments + fill-tagged field keys) in source order.
    pub(super) pre_items: Vec<PreItem>,
    /// Pre-scan nested comments (with structural paths).
    pub(super) pre_nested_comments: Vec<NestedComment>,
    /// Pre-scan warnings (unknown-tag strips, ...).
    pub(super) pre_warnings: Vec<Diagnostic>,
}

/// Creates serde_saphyr Options with security budgets configured.
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

/// Split a block's raw content into its `#@` system-metadata header and the
/// YAML payload that follows it.
///
/// The metadata header is the run of `#@`-prefixed lines at the top of the
/// block (blank lines interspersed are skipped). The payload is everything
/// after the last header line. A block with no `#@` line yields an empty
/// header.
fn split_meta_header(content: &str) -> (Vec<&str>, &str) {
    let mut header: Vec<&str> = Vec::new();
    let mut payload_start = 0;
    for line in content.split_inclusive('\n') {
        let line_text = line.trim_end_matches(['\n', '\r']);
        let trimmed = line_text.trim();
        if trimmed.is_empty() {
            payload_start += line.len();
            continue;
        }
        if trimmed.starts_with("#@") {
            header.push(line_text);
            payload_start += line.len();
            continue;
        }
        break;
    }
    (header, &content[payload_start..])
}

/// Process one recognised `~~~card-yaml` block and build a [`MetadataBlock`].
///
/// `block_start` / `block_end` bound the whole block (used to slice card
/// bodies). `content_start` / `content_end` bound the block content between
/// the `~~~card-yaml` opener and its `~~~` closer. `block_index` is used only
/// for YAML-error location context.
pub(super) fn build_block(
    markdown: &str,
    block_start: usize,
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

    // Separate the `#@` system-metadata header from the YAML payload.
    let (header, yaml_payload) = split_meta_header(raw_content);
    let meta = parse_meta_header(&header)?;

    // Run the pre-scan over the YAML payload to extract top-level comments,
    // `!fill` markers, and warn on unsupported tags / nested comments.
    let pre = prescan_fence_content(yaml_payload);

    if let Some(err) = pre.fill_target_errors.first() {
        return Err(ParseError::InvalidStructure(err.clone()));
    }

    let content = pre.cleaned_yaml.trim().to_string();
    let yaml_value = if content.is_empty() {
        None
    } else {
        let parsed = match serde_saphyr::from_str_with_options::<serde_json::Value>(
            &content,
            yaml_parse_options(),
        ) {
            Ok(parsed) => parsed,
            Err(e) => {
                let line = markdown[..block_start].lines().count() + 1;
                return Err(ParseError::YamlErrorWithLocation {
                    message: e.to_string(),
                    line,
                    block_index,
                });
            }
        };
        validate_payload_yaml(parsed)?
    };

    // Per-block field-count check (spec §8)
    if let Some(serde_json::Value::Object(ref map)) = yaml_value {
        if map.len() > crate::error::MAX_FIELD_COUNT {
            return Err(ParseError::InputTooLarge {
                size: map.len(),
                max: crate::error::MAX_FIELD_COUNT,
            });
        }
    }

    Ok(MetadataBlock {
        start: block_start,
        end: block_end,
        yaml_value,
        meta,
        pre_items: pre.items,
        pre_nested_comments: pre.nested_comments,
        pre_warnings: pre.warnings,
    })
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
    // Strip a leading UTF-8 BOM if present.
    let markdown = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);

    if markdown.trim().is_empty() {
        return Err(crate::error::ParseError::EmptyInput(
            "Empty markdown input cannot be parsed as a Quillmark Document. \
             Provide at least a root card-yaml block declaring `#@quill: <name>`."
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

    // Find all card-yaml blocks. The first is the document root; the rest are
    // composable cards.
    let (blocks, warnings) = find_metadata_blocks(markdown)?;

    if blocks.is_empty() {
        return Err(crate::error::ParseError::MissingQuill(
            "Missing required root card-yaml block. The document must open with a \
             `~~~card-yaml` block declaring `#@quill: <name>`."
                .to_string(),
        ));
    }

    // The root block must declare a `#@quill` reference. (Its value is
    // validated into a typed `QuillReference` during `#@` header parsing.)
    let root_block = &blocks[0];
    if root_block.meta.quill.is_none() {
        return Err(ParseError::MissingQuill(
            "The document's root card-yaml block must declare `#@quill: <name>`.".to_string(),
        ));
    }

    // Build the root block's payload item list.
    let main_payload = build_payload_from_pre_and_parsed(
        &root_block.pre_items,
        &root_block.pre_nested_comments,
        &root_block.yaml_value,
    )?;
    // Surface pre-scan warnings (nested-comment drops, unsupported tags).
    let mut warnings = warnings;
    for w in &root_block.pre_warnings {
        warnings.push(w.clone());
    }

    // Global body: between the end of the root block and the start of the
    // first composable card block (or EOF). When a block follows, the slice
    // ends with the blank-line separator — strip it so stored bodies contain
    // only authored content.
    let body_start = blocks[0].end;
    let (body_end, body_is_followed_by_fence) = match blocks.get(1) {
        Some(b) => (b.start, true),
        None => (markdown.len(), false),
    };
    let global_body_raw = &markdown[body_start..body_end];
    let global_body = if body_is_followed_by_fence {
        strip_blank_separator(global_body_raw).to_string()
    } else {
        global_body_raw.to_string()
    };

    let main = Card::from_parts(
        true,
        blocks[0].meta.clone(),
        main_payload,
        global_body,
    );

    // Parse composable card blocks (every block after the root) into Cards.
    let mut cards: Vec<Card> = Vec::new();
    for idx in 1..blocks.len() {
        let block = &blocks[idx];

        // Only the root block binds the document to a quill. A composable
        // card declaring `#@quill` is a structural error — `#@quill` is
        // captured by the per-block header parser, but rejected here, where
        // root-vs-composable position is known.
        if block.meta.quill.is_some() {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not declare `#@quill` — only \
                 the document's root block binds the document to a quill."
                    .to_string(),
            ));
        }

        let card_payload = build_payload_from_pre_and_parsed(
            &block.pre_items,
            &block.pre_nested_comments,
            &block.yaml_value,
        )
        .map_err(|e| match e {
            ParseError::InvalidStructure(msg) => {
                ParseError::InvalidStructure(format!("Invalid YAML in card block: {}", msg))
            }
            other => other,
        })?;
        for w in &block.pre_warnings {
            warnings.push(w.clone());
        }

        // Card body: between this block's end and the next block's start (or EOF).
        let card_body_start = block.end;
        let has_next_block = idx + 1 < blocks.len();
        let card_body_end = if has_next_block {
            blocks[idx + 1].start
        } else {
            markdown.len()
        };
        let card_body_raw = &markdown[card_body_start..card_body_end];
        let card_body = if has_next_block {
            strip_blank_separator(card_body_raw).to_string()
        } else {
            card_body_raw.to_string()
        };

        cards.push(Card::from_parts(
            false,
            block.meta.clone(),
            card_payload,
            card_body,
        ));
    }

    let doc = Document::from_main_and_cards(main, cards, warnings.clone());

    Ok((doc, warnings))
}

/// Build a [`Payload`] from the pre-scan items and the parsed YAML mapping.
///
/// The pre-scan defined source order for fields and comments; the parsed
/// YAML defined the typed value for each key. We walk pre-scan order, pulling
/// each field's value from `parsed`. Any field the pre-scan didn't catch is
/// appended at the end in parsed-map order so we never drop values.
fn build_payload_from_pre_and_parsed(
    pre_items: &[PreItem],
    pre_nested_comments: &[NestedComment],
    yaml_value: &Option<serde_json::Value>,
) -> Result<Payload, ParseError> {
    let mapping = match yaml_value {
        Some(serde_json::Value::Object(map)) => map.clone(),
        Some(serde_json::Value::Null) | None => serde_json::Map::new(),
        Some(_) => {
            return Err(ParseError::InvalidStructure(
                "expected a mapping".to_string(),
            ));
        }
    };

    let mut items: Vec<PayloadItem> = Vec::new();
    let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for pre in pre_items {
        match pre {
            PreItem::Comment { text, inline } => {
                items.push(PayloadItem::Comment {
                    text: text.clone(),
                    inline: *inline,
                });
            }
            PreItem::Field { key, fill } => {
                if let Some(value) = mapping.get(key).cloned() {
                    // `!fill` applies to scalars and sequences. Mappings are
                    // rejected because top-level `type: object` is unsupported.
                    if *fill && value.is_object() {
                        return Err(ParseError::InvalidStructure(format!(
                            "`!fill` on key `{}` targets a mapping; `!fill` is supported on scalars and sequences only",
                            key
                        )));
                    }
                    items.push(PayloadItem::Field {
                        key: key.clone(),
                        value: QuillValue::from_json(value),
                        fill: *fill,
                    });
                    consumed.insert(key.clone());
                }
            }
        }
    }

    // Append any parsed-map keys that the pre-scan didn't capture.
    for (key, value) in &mapping {
        if consumed.contains(key) {
            continue;
        }
        items.push(PayloadItem::Field {
            key: key.clone(),
            value: QuillValue::from_json(value.clone()),
            fill: false,
        });
    }

    Ok(Payload::from_items_with_nested(
        items,
        pre_nested_comments.to_vec(),
    ))
}
