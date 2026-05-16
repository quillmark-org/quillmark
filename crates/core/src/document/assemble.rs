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
use super::sentinel::{extract_sentinels, is_valid_tag_name};
use super::{Document, Card, Sentinel};

/// Strip exactly one F2 structural separator from the tail of a body slice.
///
/// The F2 rule (`MARKDOWN.md §3`) requires a blank line immediately above
/// every metadata fence. When a body is followed by another fence, the raw
/// slice ends with that blank line's terminator — exactly one `\n` or
/// `\r\n`. This helper strips that single line ending so stored bodies
/// contain only authored content. The emitter re-adds the separator on
/// output via `ensure_f2_before_fence`.
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

/// Parse a legacy `---/CARD: …/---` card body.
///
/// Used by the legacy `---/CARD: …/---` parser path (MARKDOWN.md §4.4). The previous
/// card syntax carried the kind as a `CARD:` first body key; the canonical
/// syntax carries it in the fence info string (`MARKDOWN.md §3.2`). To produce
/// the same `Card` from a legacy block, the kind must be lifted *out* of the
/// body — leaving `CARD:` (or `KIND:`) in place would now be a reserved-key
/// hard error.
///
/// Returns `(kind, body_without_card_line)`: the trimmed value of the first
/// `CARD:` line, and the body content with that line removed. Returns `None`
/// if the first non-blank, non-comment line is not a `CARD:` key — the caller
/// guarantees this never happens, but we stay defensive rather than corrupt
/// the slice.
fn extract_legacy_card(content: &str) -> Option<(String, String)> {
    let mut kind: Option<String> = None;
    let mut out = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        if kind.is_some() {
            out.push_str(line);
            continue;
        }
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let body = line[leading..].trim_end_matches(['\n', '\r']);
        if body.trim().is_empty() || body.starts_with('#') {
            out.push_str(line);
            continue;
        }
        let rest = body.strip_prefix("CARD:")?;
        // Drop a trailing inline `# …` comment, then trim surrounding space.
        let value = match rest.find(" #") {
            Some(idx) => &rest[..idx],
            None => rest,
        };
        kind = Some(value.trim().to_string());
        // The `CARD:` line itself is dropped — not pushed to `out`.
    }
    kind.map(|k| (k, out))
}

/// Which sentinel a metadata block carries. `Main` is the top frontmatter's
/// `QUILL:` reference (raw string, parsed to `QuillReference` later); `Card`
/// is a card fence's kind, from its `card <kind>` info string.
#[derive(Debug)]
pub(super) enum BlockSentinel {
    Main(String),
    Inline(String),
}

/// How `find_metadata_blocks` classified a fence before handing it to
/// `build_block`. Replaces the former `card: Option<String>` +
/// `legacy_card: bool` pair, which together encoded these three cases but
/// also admitted the impossible `Some(kind)` + legacy combination.
#[derive(Debug)]
pub(super) enum BlockSource {
    /// Document frontmatter (`---/---` with a `QUILL:` first key).
    Frontmatter,
    /// Canonical card fence; the kind comes from the `card <kind>` info string.
    Inline(String),
    /// Legacy `---/CARD: …/---` card — the kind is lifted from the `CARD:`
    /// first body key (`MARKDOWN.md §4.4`).
    LegacyCard,
}

/// An intermediate representation of one parsed metadata fence (frontmatter
/// or card).
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize,                          // Position of opening fence
    pub(super) end: usize,                            // Position after closing fence
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML (None when fence body is empty)
    pub(super) sentinel: BlockSentinel,
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
///
/// `source` is how `find_metadata_blocks` classified the fence:
/// `Frontmatter` for the document frontmatter, `Inline(kind)` for a canonical
/// card fence (kind from the `card <kind>` info string), or `LegacyCard` for
/// a legacy `---/CARD: …/---` card (`MARKDOWN.md §4.4`). For `LegacyCard` the
/// kind is lifted out of the `CARD:` first body key and that line is dropped
/// before YAML parsing, so the resulting `Card` matches the canonical
/// ` ```card <kind> ` form; the caller verifies the first content key is
/// `CARD:` and emits the deprecation warning.
pub(super) fn build_block(
    markdown: &str,
    abs_pos: usize,
    content_start: usize,
    content_end: usize,
    block_end: usize,
    block_index: usize,
    source: BlockSource,
) -> Result<MetadataBlock, ParseError> {
    let raw_slice = &markdown[content_start..content_end];
    let legacy_owned: String;
    let (raw_content, card): (&str, Option<String>) = match source {
        BlockSource::LegacyCard => {
            let line = markdown[..abs_pos].lines().count() + 1;
            let (kind, stripped) = extract_legacy_card(raw_slice).ok_or_else(|| {
                ParseError::InvalidStructure(format!(
                    "Legacy card at line {} is missing its `CARD:` first body key.",
                    line
                ))
            })?;
            if !is_valid_tag_name(&kind) {
                return Err(ParseError::InvalidStructure(format!(
                    "Legacy card at line {} has an invalid `CARD:` value `{}` — the kind must match pattern [a-z_][a-z0-9_]*.",
                    line, kind
                )));
            }
            legacy_owned = stripped;
            (legacy_owned.as_str(), Some(kind))
        }
        BlockSource::Inline(kind) => (raw_slice, Some(kind)),
        BlockSource::Frontmatter => (raw_slice, None),
    };

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
    let is_frontmatter = card.is_none();
    let (quill_ref, yaml_value) = if content.is_empty() {
        (None, None)
    } else {
        match serde_saphyr::from_str_with_options::<serde_json::Value>(
            &content,
            yaml_parse_options(),
        ) {
            Ok(parsed) => extract_sentinels(parsed, is_frontmatter)?,
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

    // `find_metadata_blocks` classifies every block lexically before calling
    // build_block — frontmatter carries `card = None` and a `QUILL:`
    // first key; a card carries `card = Some(kind)` from its info string.
    let sentinel = match (card, quill_ref) {
        (Some(k), _) => BlockSentinel::Inline(k),
        (None, Some(r)) => BlockSentinel::Main(r),
        (None, None) => unreachable!(
            "find_metadata_blocks classifies every block before calling build_block"
        ),
    };

    // Per-fence field-count check (spec §8, §6.1 of GAP analysis). Frontmatter
    // adds +1 for the stripped `QUILL` sentinel so the cap matches what the
    // user wrote; a card's kind is not a body field, so it adds nothing.
    if let Some(serde_json::Value::Object(ref map)) = yaml_value {
        let size = map.len() + usize::from(is_frontmatter);
        if size > crate::error::MAX_FIELD_COUNT {
            return Err(ParseError::InputTooLarge {
                size,
                max: crate::error::MAX_FIELD_COUNT,
            });
        }
    }

    Ok(MetadataBlock {
        start: abs_pos,
        end: block_end,
        yaml_value,
        sentinel,
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

    // Find all metadata blocks. `find_metadata_blocks` guarantees that
    // block 0 (if present) carries `BlockSentinel::Main` and every block
    // after it carries `BlockSentinel::Inline`.
    let (blocks, warnings, first_fence_issue) = find_metadata_blocks(markdown)?;

    let frontmatter_block = match blocks.first() {
        Some(b) if matches!(b.sentinel, BlockSentinel::Main(_)) => b,
        _ => {
            return Err(crate::error::ParseError::MissingQuillField(
                missing_quill_message(first_fence_issue),
            ))
        }
    };
    let BlockSentinel::Main(ref quill_tag) = frontmatter_block.sentinel else {
        unreachable!("matched above")
    };

    // Build frontmatter item list (YAML content with QUILL stripped). The
    // pre-scan defined source order; serde_saphyr produced typed values.
    let frontmatter = build_frontmatter_from_pre_and_parsed(
        &frontmatter_block.pre_items,
        &frontmatter_block.pre_nested_comments,
        &frontmatter_block.yaml_value,
        true,
    )?;
    // Surface pre-scan warnings (nested-comment drops, unsupported tags).
    let mut warnings = warnings;
    for w in &frontmatter_block.pre_warnings {
        warnings.push(w.clone());
    }

    // Global body: between end of frontmatter (block 0) and start of the
    // first card (or EOF). When a fence follows, the raw slice ends with
    // the F2 blank-line terminator — strip it so stored bodies hold only
    // authored content. The emitter re-derives the separator on output.
    let body_start = frontmatter_block.end;
    let (body_end, body_is_followed_by_fence) = match blocks.get(1) {
        Some(b) => (b.start, true),
        None => (markdown.len(), false),
    };
    let global_body_raw = &markdown[body_start..body_end];
    let global_body = if body_is_followed_by_fence {
        strip_f2_separator(global_body_raw).to_string()
    } else {
        global_body_raw.to_string()
    };

    // Parse card blocks into typed Cards.
    let mut cards: Vec<Card> = Vec::new();
    for (idx, block) in blocks.iter().enumerate().skip(1) {
        let BlockSentinel::Inline(ref tag_name) = block.sentinel else {
            unreachable!("blocks[1..] are cards by construction")
        };
        let card_frontmatter = build_frontmatter_from_pre_and_parsed(
            &block.pre_items,
            &block.pre_nested_comments,
            &block.yaml_value,
            false,
        )
        .map_err(|e| match e {
            ParseError::InvalidStructure(msg) => ParseError::InvalidStructure(format!(
                "Invalid YAML in card block '{}': {}",
                tag_name, msg
            )),
            other => other,
        })?;
        for w in &block.pre_warnings {
            warnings.push(w.clone());
        }

        // Card body: from this block's end to the next block's start (or EOF).
        // If another fence follows, the body slice ends with the F2 blank-line
        // terminator — strip it so stored bodies hold only authored content.
        let card_body = match blocks.get(idx + 1) {
            Some(next) => strip_f2_separator(&markdown[block.end..next.start]).to_string(),
            None => markdown[block.end..].to_string(),
        };

        cards.push(Card::new_with_sentinel(
            Sentinel::Inline(tag_name.clone()),
            card_frontmatter,
            card_body,
        ));
    }

    let quill_ref = QuillReference::from_str(quill_tag).map_err(|e| {
        ParseError::InvalidStructure(format!("Invalid QUILL tag '{}': {}", quill_tag, e))
    })?;

    let main = Card::new_with_sentinel(Sentinel::Main(quill_ref), frontmatter, global_body);
    let doc = Document::from_main_and_cards(main, cards, warnings.clone());

    Ok((doc, warnings))
}

/// Build a [`Frontmatter`] from the pre-scan items and the parsed YAML
/// mapping.
///
/// The pre-scan defined source order for fields and comments; the parsed
/// YAML defined the typed value for each key. We walk pre-scan order,
/// pulling each field's value from `parsed`. Any field that the pre-scan
/// didn't catch (e.g. it used a YAML key form the pre-scan doesn't
/// recognise — exotic identifier, flow-mapping syntax, etc.) is appended at
/// the end of the item list in parsed-map order so we never drop values.
///
/// `is_frontmatter` selects which sentinel key `extract_sentinels` stripped
/// from `yaml_value`: `QUILL` for the document frontmatter, nothing for a
/// card (its kind lives in the info string). The stripped key is skipped
/// from the item list; in a card, `QUILL` is an ordinary field and is kept.
fn build_frontmatter_from_pre_and_parsed(
    pre_items: &[PreItem],
    pre_nested_comments: &[NestedComment],
    yaml_value: &Option<serde_json::Value>,
    is_frontmatter: bool,
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
    // When the QUILL sentinel field is skipped, its trailing inline comment
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
                // In frontmatter the `QUILL` sentinel key is stripped from the
                // parsed map by `extract_sentinels`; skip it in the item list.
                // In a card, `QUILL` is an ordinary field (the kind lives in
                // the info string) and must be kept.
                if is_frontmatter && key == "QUILL" {
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
