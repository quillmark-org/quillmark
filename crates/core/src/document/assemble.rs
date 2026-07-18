//! Assembly of card-yaml blocks into a [`Document`].
//!
//! This module is the top-level parsing glue: it calls the fence scanner,
//! parses each block's YAML payload, extracts the `$`-prefixed system
//! metadata into typed values, and assembles a typed [`Document`] from the
//! pieces.
//!
//! ## Unified payload items
//!
//! Both `$`-prefixed system metadata and user fields end up as variants of
//! [`PayloadItem`] in the card's [`Payload`] item list, in source order.
//! There is no separate "metadata region" or "metadata vs payload" routing
//! — a comment is a comment, attached to whichever item precedes it. This
//! is what keeps inline-comment preservation symmetric across the
//! `$`/non-`$` boundary.
//!
//! `extract_meta_items` returns typed system [`PayloadItem`]s in source
//! order; `build_payload` then walks the prescan items, splicing each `$`
//! field marker back to the typed item produced from the parsed YAML and
//! preserving every comment as-authored.

use std::collections::HashMap;

use crate::error::ParseError;
use crate::value::QuillValue;
use crate::Diagnostic;

use super::fences::find_metadata_blocks;
use super::meta::{extract_meta_items, meta_key};
use super::payload::{Payload, PayloadItem};
use quillmark_richtext::RichText;

/// Import a sliced body string into the corpus, surfacing an over-nesting
/// failure as a [`ParseError`]. The parse-time half of the one markdown→corpus
/// boundary ([`super::import_body`]).
fn import_body_or_parse_error(md: &str) -> Result<RichText, ParseError> {
    super::import_body(md).map_err(|e| ParseError::BodyImport(e.to_string()))
}
use super::prescan::{prescan_fence_content, CommentPathSegment, NestedComment, PreItem};
use super::{Card, Document};

/// Build a `MissingQuill` message that names the specific malformation when
/// the document has none of the recognised root block forms the parser
/// requires.
///
/// LLM authors hit one recurring shape here: a bare YAML mapping with
/// `$quill: ...` (or `quill: ...`) at the top, no fences at all. Naming the
/// concrete edit helps the model converge faster than the generic "open a
/// `~~~` block" advice.
fn missing_block_message(markdown: &str) -> String {
    let trimmed = markdown.trim_start();

    // Bare YAML with `$quill:` at the top: the model wrote the metadata but
    // forgot the fence entirely.
    if trimmed.starts_with("$quill:") || trimmed.starts_with("quill:") {
        return "Missing required root card-yaml block. Your document starts with \
                YAML metadata but is missing the `~~~` fence. Wrap the \
                metadata: add a line `~~~` above the `$quill:` line and a \
                line containing exactly `~~~` (three tildes, no info string) below \
                the last metadata field, before the prose body."
            .to_string();
    }

    "Missing required root card-yaml block. The document must open with a \
     `~~~` block declaring `$quill: <name>` (and `$kind: main`) as \
     the first two lines, closed by a line containing exactly `~~~`."
        .to_string()
}

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

/// An intermediate representation of one `~~~ … ~~~` card-yaml block.
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize, // Position of the opening `~~~`
    pub(super) end: usize,   // Position after the closing `~~~`
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML payload as JSON
    /// Typed `$` system-metadata payload items in source order.
    pub(super) meta_items: Vec<PayloadItem>,
    /// Pre-scan items (comments + fill-tagged field keys) in source order.
    pub(super) pre_items: Vec<PreItem>,
    /// Pre-scan nested comments (with structural paths).
    pub(super) pre_nested_comments: Vec<NestedComment>,
    /// Pre-scan nested `!must_fill` paths (rooted at the owning top-level key).
    pub(super) pre_nested_fills: Vec<Vec<CommentPathSegment>>,
    /// Pre-scan warnings (unknown-tag strips, ...).
    pub(super) pre_warnings: Vec<Diagnostic>,
}

/// Process one recognised `~~~` card-yaml block and build a [`MetadataBlock`].
///
/// `block_start` / `block_end` bound the whole block (used to slice card
/// bodies). `content_start` / `content_end` bound the block content between
/// the `~~~` opener and its `~~~` closer. `block_index` is used only
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

    let pre = prescan_fence_content(raw_content);

    if let Some(err) = pre.fill_target_errors.first() {
        return Err(ParseError::InvalidStructure(err.clone()));
    }

    // `!must_fill` is not permitted on `$` metadata keys — those are extracted into
    // typed values and have no placeholder semantics.
    for item in &pre.items {
        if let PreItem::Field { key, fill: true } = item {
            if key.starts_with('$') {
                return Err(ParseError::InvalidStructure(format!(
                    "`!must_fill` on `{}` is not permitted — system-metadata keys \
                     cannot be placeholders",
                    key
                )));
            }
        }
    }

    let content = pre.cleaned_yaml.trim().to_string();
    let (meta_items, yaml_value) = if content.is_empty() {
        (Vec::new(), None)
    } else {
        let mut parsed = match serde_saphyr::from_str_with_options::<serde_json::Value>(
            &content,
            super::limits::yaml_parse_options(),
        ) {
            Ok(parsed) => parsed,
            Err(e) => {
                let line = markdown[..block_start].lines().count() + 1;
                let enriched = super::yaml_hints::enrich_yaml_error(&e.to_string(), &content);
                return Err(ParseError::YamlErrorWithLocation {
                    message: enriched.message,
                    line,
                    block_index,
                    hint: enriched.hint,
                });
            }
        };
        let meta = extract_meta_items(&mut parsed)?;
        (meta, Some(parsed))
    };

    // Per-block field-count check (spec §8) — applied after `$`-key
    // extraction so the user-data field count is what is bounded.
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
        meta_items,
        pre_items: pre.items,
        pre_nested_comments: pre.nested_comments,
        pre_nested_fills: pre.nested_fills,
        pre_warnings: pre.warnings,
    })
}

/// Decompose markdown, discarding warnings. Test-only convenience over
/// [`decompose_with_warnings`]; the public entry [`super::Document::parse`]
/// keeps the warnings, so nothing in the shipping build discards them here.
#[cfg(test)]
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
             Provide at least a root card-yaml block declaring `$quill: <name>`."
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
            missing_block_message(markdown),
        ));
    }

    // The root block must declare a `$quill` reference.
    let root_block = &blocks[0];
    let has_root_quill = root_block
        .meta_items
        .iter()
        .any(|m| matches!(m, PayloadItem::Quill { .. }));
    if !has_root_quill {
        return Err(ParseError::MissingQuill(
            "The document's root card-yaml block must declare `$quill: <name>` as \
             its first line (e.g. `$quill: usaf_memo@0.2.0`)."
                .to_string(),
        ));
    }

    // The root block's `$kind` is `main` by position (markdown-spec.md §3.3).
    // An explicit `$kind: main` is accepted; an omitted `$kind` is accepted
    // and synthesised below at the canonical position; any other value is a
    // parse error.
    let root_kind = root_block.meta_items.iter().find_map(|m| match m {
        PayloadItem::Kind { value } => Some(value.as_str()),
        _ => None,
    });
    if let Some(other) = root_kind {
        if other != "main" {
            return Err(ParseError::InvalidStructure(format!(
                "The document's root card-yaml block has `$kind: {}`, but \
                 `main` is reserved for the document root. Remove the line \
                 (the root's kind is `main` by position) or change it to \
                 `$kind: main`.",
                other
            )));
        }
    }

    // Build the root block's payload, then synthesise `$kind: main` if the
    // source omitted it. Using `set_kind` here inserts at the canonical
    // position (after `$quill`, before any `$id`/`$ext`/user fields), so
    // canonical input round-trips byte-equal and non-canonical input
    // converges on first emit (markdown-spec.md §9).
    let mut main_payload = build_payload(
        &root_block.meta_items,
        &root_block.pre_items,
        &root_block.pre_nested_comments,
        &root_block.pre_nested_fills,
        &root_block.yaml_value,
    )?;
    if main_payload.kind().is_none() {
        main_payload.set_kind("main");
    }
    let mut warnings = warnings;
    for w in &root_block.pre_warnings {
        warnings.push(w.clone());
    }

    // Global body: between the end of the root block and the start of the
    // first composable card block (or EOF). Strip the structural blank-line
    // separator when a block follows.
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

    let main = Card::from_parts(main_payload, import_body_or_parse_error(&global_body)?);

    // Parse composable card blocks (every block after the root) into Cards.
    let mut cards: Vec<Card> = Vec::new();
    for idx in 1..blocks.len() {
        let block = &blocks[idx];

        // Only the root block binds the document to a quill.
        if block
            .meta_items
            .iter()
            .any(|m| matches!(m, PayloadItem::Quill { .. }))
        {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not declare `$quill` — only \
                 the document's root block binds the document to a quill."
                    .to_string(),
            ));
        }

        // `main` is reserved for the document root.
        let kind_is_main = block.meta_items.iter().any(|m| match m {
            PayloadItem::Kind { value } => value == "main",
            _ => false,
        });
        if kind_is_main {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not declare `$kind: main` — \
                 `main` is reserved for the document root."
                    .to_string(),
            ));
        }

        // Seeding overlays live on the document root only (like `$quill`).
        if block
            .meta_items
            .iter()
            .any(|m| matches!(m, PayloadItem::Meta { key, .. } if key.is_root_only()))
        {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not carry `$seed` — only the \
                 document's root block carries seeding overlays."
                    .to_string(),
            ));
        }

        let card_payload = build_payload(
            &block.meta_items,
            &block.pre_items,
            &block.pre_nested_comments,
            &block.pre_nested_fills,
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
            card_payload,
            import_body_or_parse_error(&card_body)?,
        ));
    }

    let doc = Document::from_main_and_cards(main, cards);

    Ok((doc, warnings))
}

/// Build a unified [`Payload`] from the pre-scan items, the typed `$`
/// system-metadata items, and the parsed YAML mapping.
///
/// Walks `pre_items` in source order. Each non-`$` field pulls its typed
/// value from `yaml_value`; each `$` field is replaced with the matching
/// typed system [`PayloadItem`] from `meta_items`; comments pass through
/// verbatim. Any parsed-map keys the pre-scan didn't capture are appended
/// at the end so we never silently drop values.
/// Validate a user field entering the payload from the parse path,
/// mapping a violation to `ParseError::InvalidStructure` (spec §10).
fn validate_parsed_field(key: &str, value: &serde_json::Value) -> Result<(), ParseError> {
    use crate::document::edit::{validate_field, FieldViolation};
    validate_field(key, value).map_err(|v| match v {
        FieldViolation::InvalidName => ParseError::InvalidStructure(format!(
            "invalid data-field name `{}`: field names must match [A-Za-z_][A-Za-z0-9_]*",
            key
        )),
        FieldViolation::TooDeep => ParseError::InvalidStructure(format!(
            "field `{}` nests deeper than the maximum of {} levels",
            key,
            crate::document::limits::MAX_YAML_DEPTH
        )),
    })
}

fn build_payload(
    meta_items: &[PayloadItem],
    pre_items: &[PreItem],
    pre_nested_comments: &[NestedComment],
    pre_nested_fills: &[Vec<CommentPathSegment>],
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

    // Look up typed `$` items by `$key`. Each entry is consumed at most
    // once; anything left over at the end is appended in source order.
    // `extract_meta_items` only ever returns the closed-set system
    // variants; assert that contract here so a regression upstream is
    // loud, not a silent drop of user fields/comments.
    let mut typed_by_key: HashMap<&'static str, PayloadItem> =
        HashMap::with_capacity(meta_items.len());
    for m in meta_items {
        let k = meta_key(m).expect(
            "build_payload: meta_items must contain only system variants \
             ($quill/$kind/$id/$ext); got a Field or Comment",
        );
        typed_by_key.insert(k, m.clone());
    }

    let mut items: Vec<PayloadItem> = Vec::new();
    let mut consumed_user_keys: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for item in pre_items {
        match item {
            PreItem::Comment { text, inline } => {
                items.push(PayloadItem::Comment {
                    text: text.clone(),
                    inline: *inline,
                });
            }
            PreItem::Field { key, fill } => {
                if key.starts_with('$') {
                    if let Some(meta) = typed_by_key.remove(key.as_str()) {
                        items.push(meta);
                    }
                    continue;
                }
                if let Some(value) = mapping.get(key).cloned() {
                    if *fill && value.is_object() {
                        return Err(ParseError::InvalidStructure(format!(
                            "`!must_fill` on key `{}` targets a mapping; `!must_fill` is supported on scalars and sequences only",
                            key
                        )));
                    }
                    validate_parsed_field(key, &value)?;
                    let mut qv = QuillValue::from_json(value);
                    apply_nested_fills(key, &mut qv, pre_nested_fills)?;
                    items.push(PayloadItem::Field {
                        key: key.clone(),
                        value: qv,
                        fill: *fill,
                        nested_comments: Vec::new(),
                    });
                    consumed_user_keys.insert(key.clone());
                }
            }
        }
    }

    // Drain any typed `$` entries the prescan didn't reach (shouldn't
    // happen in well-formed input but keeps the conversion total). Walk
    // `meta_items` in source order so the relative `$` ordering is
    // preserved.
    for meta in meta_items {
        let k = meta_key(meta).expect("see invariant above");
        if typed_by_key.remove(k).is_some() {
            items.push(meta.clone());
        }
    }

    // Append any parsed-map keys that the pre-scan didn't capture.
    for (key, value) in &mapping {
        if consumed_user_keys.contains(key) {
            continue;
        }
        validate_parsed_field(key, value)?;
        let mut qv = QuillValue::from_json(value.clone());
        apply_nested_fills(key, &mut qv, pre_nested_fills)?;
        items.push(PayloadItem::Field {
            key: key.clone(),
            value: qv,
            fill: false,
            nested_comments: Vec::new(),
        });
    }

    // Partition the prescan's flat `nested_comments` onto the matching
    // Field / Ext items (paths become relative to the owning value).
    Ok(Payload::from_items_with_flat_nested(
        items,
        pre_nested_comments.to_vec(),
    ))
}

/// Apply the nested `!must_fill` markers rooted at `key` onto `value`'s tree.
///
/// Paths in `pre_nested_fills` are rooted at the owning top-level key; the
/// first segment is stripped and the remainder addresses a node within
/// `value`. `!must_fill` on a mapping node is rejected, mirroring the
/// top-level rule.
fn apply_nested_fills(
    key: &str,
    value: &mut QuillValue,
    pre_nested_fills: &[Vec<CommentPathSegment>],
) -> Result<(), ParseError> {
    for path in pre_nested_fills {
        let Some((CommentPathSegment::Key(first), rest)) = path.split_first() else {
            continue;
        };
        if first != key {
            continue;
        }
        if value.is_object_at(rest) {
            return Err(ParseError::InvalidStructure(format!(
                "`!must_fill` on `{}` targets a mapping; `!must_fill` is supported on scalars and sequences only",
                render_path(path)
            )));
        }
        // The path came from our own prescan over the same source, so it must
        // resolve against the parsed tree. A miss means prescan and the YAML
        // parser disagreed on structure — surface it loudly in dev/test builds
        // rather than silently dropping the marker.
        let applied = value.set_fill_at(rest);
        debug_assert!(
            applied,
            "prescan recorded a nested fill path that did not resolve against \
             the parsed value: `{}`",
            render_path(path)
        );
    }
    Ok(())
}

/// Render a structural path as a dotted/bracketed string for diagnostics,
/// e.g. `addr.street` or `recipients[0].name`.
fn render_path(path: &[CommentPathSegment]) -> String {
    let mut out = String::new();
    for seg in path {
        match seg {
            CommentPathSegment::Key(k) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(k);
            }
            CommentPathSegment::Index(i) => {
                out.push_str(&format!("[{}]", i));
            }
        }
    }
    out
}
