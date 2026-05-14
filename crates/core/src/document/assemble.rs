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

/// Rewrite the first non-blank, non-comment line's `CARD:` prefix to `KIND:`.
///
/// Used by the Release-N legacy parser path (LEAF_REWORK.md §7) so that a
/// document authored against the previous syntax (`---/CARD: foo/---`)
/// produces the same `Leaf` as the canonical ` ```leaf / KIND: foo / ``` `
/// form. The substitution is byte-for-byte length-preserving (both sentinels
/// are four ASCII characters), so downstream offset bookkeeping is unaffected.
/// Caller guarantees the first content key is `CARD:` before invoking.
fn rewrite_first_card_to_kind(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rewrote = false;
    for line in content.split_inclusive('\n') {
        if rewrote {
            out.push_str(line);
            continue;
        }
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let body = &line[leading..];
        if body.trim().is_empty() || body.starts_with('#') {
            out.push_str(line);
            continue;
        }
        if let Some(rest) = body.strip_prefix("CARD:") {
            out.push_str(&line[..leading]);
            out.push_str("KIND:");
            out.push_str(rest);
            rewrote = true;
        } else {
            // First non-blank, non-comment line is not `CARD:` — caller
            // should never invoke us in this case, but stay verbatim
            // rather than corrupt the slice.
            out.push_str(line);
            rewrote = true;
        }
    }
    out
}

/// Which sentinel a metadata block carries. `Main` is the top frontmatter's
/// `QUILL:` reference (raw string, parsed to `QuillReference` later); `Leaf`
/// is a leaf fence's `KIND:` tag.
#[derive(Debug)]
pub(super) enum BlockSentinel {
    Main(String),
    Leaf(String),
}

/// An intermediate representation of one parsed metadata fence (frontmatter
/// or leaf).
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
/// When `legacy_card_to_kind` is true, the first occurrence of the literal
/// sentinel key `CARD:` in the raw content is rewritten to `KIND:` before
/// parsing. This is the Release-N round-trip migration path (LEAF_REWORK.md
/// §7): legacy `---/CARD: …/---` blocks are recognised as leaves so the
/// canonical emitter can rewrite them to ` ```leaf ` form on the next
/// `to_markdown()`. Caller is responsible for verifying the first content
/// key is actually `CARD:` and for emitting the deprecation warning.
pub(super) fn build_block(
    markdown: &str,
    abs_pos: usize,
    content_start: usize,
    content_end: usize,
    block_end: usize,
    block_index: usize,
    legacy_card_to_kind: bool,
) -> Result<MetadataBlock, ParseError> {
    let raw_content_owned: String;
    let raw_content: &str = if legacy_card_to_kind {
        raw_content_owned = rewrite_first_card_to_kind(&markdown[content_start..content_end]);
        &raw_content_owned
    } else {
        &markdown[content_start..content_end]
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
    let (tag, quill_ref, yaml_value) = if content.is_empty() {
        (None, None, None)
    } else {
        match serde_saphyr::from_str_with_options::<serde_json::Value>(
            &content,
            yaml_parse_options(),
        ) {
            Ok(parsed) => extract_sentinels(parsed)?,
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

    // The fence-detection pass (`find_metadata_blocks`) commits to a fence
    // kind on the lexical cues alone — `---/---` with first key `QUILL:` for
    // block 0, ` ```leaf ` with first key `KIND:` for the rest. So by the
    // time we reach build_block, the expected sentinel is fully determined
    // by `block_index` and `extract_sentinels` will have produced exactly
    // the matching variant.
    let sentinel = match (block_index, quill_ref, tag) {
        (0, Some(r), _) => BlockSentinel::Main(r),
        (_, _, Some(t)) => BlockSentinel::Leaf(t),
        _ => unreachable!(
            "find_metadata_blocks validates first-key sentinel before calling build_block"
        ),
    };

    // Per-fence field-count check (spec §8, §6.1 of GAP analysis). Add +1
    // for the stripped sentinel so the cap matches what the user wrote.
    if let Some(serde_json::Value::Object(ref map)) = yaml_value {
        let size = map.len() + 1;
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
    // after it carries `BlockSentinel::Leaf`.
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
    )?;
    // Surface pre-scan warnings (nested-comment drops, unsupported tags).
    let mut warnings = warnings;
    for w in &frontmatter_block.pre_warnings {
        warnings.push(w.clone());
    }

    // Global body: between end of frontmatter (block 0) and start of the
    // first leaf (or EOF). When a fence follows, the raw slice ends with
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

    // Parse leaf blocks into typed Leaves.
    let mut leaves: Vec<Leaf> = Vec::new();
    for (idx, block) in blocks.iter().enumerate().skip(1) {
        let BlockSentinel::Leaf(ref tag_name) = block.sentinel else {
            unreachable!("blocks[1..] are leaves by construction")
        };
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

        // Leaf body: from this block's end to the next block's start (or EOF).
        // If another fence follows, the body slice ends with the F2 blank-line
        // terminator — strip it so stored bodies hold only authored content.
        let leaf_body = match blocks.get(idx + 1) {
            Some(next) => strip_f2_separator(&markdown[block.end..next.start]).to_string(),
            None => markdown[block.end..].to_string(),
        };

        leaves.push(Leaf::new_with_sentinel(
            Sentinel::Leaf(tag_name.clone()),
            leaf_frontmatter,
            leaf_body,
        ));
    }

    let quill_ref = QuillReference::from_str(quill_tag).map_err(|e| {
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
