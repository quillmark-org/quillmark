//! Line-oriented fence scanner for Quillmark Markdown.
//!
//! Detects the document's frontmatter (`---/---` at top, with `QUILL:` first
//! key) and leaf fences (CommonMark fenced code blocks with the info string
//! `leaf`, body starting with `KIND:`).

use crate::error::ParseError;
use crate::{Diagnostic, Severity};

use super::assemble::{BlockSentinel, MetadataBlock};
use super::sentinel::first_content_key;

/// Line-oriented view of the source.
pub(super) struct Lines<'a> {
    pub(super) source: &'a str,
    pub(super) starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        let mut starts = Vec::new();
        starts.push(0);
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { source, starts }
    }
    pub(super) fn len(&self) -> usize {
        self.starts.len()
    }
    pub(super) fn line_start(&self, k: usize) -> usize {
        self.starts[k]
    }
    pub(super) fn line_end_inclusive(&self, k: usize) -> usize {
        if k + 1 < self.starts.len() {
            self.starts[k + 1]
        } else {
            self.source.len()
        }
    }
    pub(super) fn line_text(&self, k: usize) -> &'a str {
        let start = self.starts[k];
        let mut end = self.line_end_inclusive(k);
        if end > start && self.source.as_bytes()[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && self.source.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        &self.source[start..end]
    }
    pub(super) fn is_blank(&self, k: usize) -> bool {
        self.line_text(k).chars().all(char::is_whitespace)
    }
}

/// Returns true if `line` is a `---` frontmatter-fence marker:
/// exactly three hyphens preceded by 0–3 spaces, followed by optional
/// trailing whitespace.
pub(super) fn is_fence_marker_line(line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    if indent > 3 {
        return false;
    }
    if line.as_bytes().first() == Some(&b'\t') {
        return false;
    }
    match line[indent..].strip_prefix("---") {
        Some(rest) => rest.chars().all(|c| c == ' ' || c == '\t'),
        None => false,
    }
}

/// Detect a CommonMark fenced code-block marker line. Returns `Some((char,
/// run_len, is_closing))` if matched.
pub(super) fn code_fence_on_line(
    line: &str,
    open_fence: Option<(u8, usize)>,
) -> Option<(u8, usize, bool)> {
    let indent = line.as_bytes().iter().take_while(|&&b| b == b' ').count();
    if indent > 3 {
        return None;
    }
    let trimmed = &line[indent..];
    let bytes = trimmed.as_bytes();
    let &first = bytes.first()?;

    if first != b'`' && first != b'~' {
        return None;
    }
    let run_len = bytes.iter().take_while(|&&b| b == first).count();
    if run_len < 3 {
        return None;
    }
    let rest = &trimmed[run_len..];
    match open_fence {
        Some((open_char, open_len)) => {
            if first == open_char
                && run_len >= open_len
                && rest.chars().all(|c| c == ' ' || c == '\t')
            {
                Some((first, run_len, true))
            } else {
                None
            }
        }
        None => Some((first, run_len, false)),
    }
}

/// First whitespace-delimited token of a fence opener's info string, or
/// `None` for non-opener lines and empty info strings.
fn fence_info_first_token(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let indent = line.as_bytes().iter().take_while(|&&b| b == b' ').count();
    let trimmed = &line[indent..];
    let &first = trimmed.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let run_len = trimmed
        .as_bytes()
        .iter()
        .take_while(|&&b| b == first)
        .count();
    trimmed[run_len..].split_whitespace().next()
}

/// Outcome of the fence-detection pass: the recognised metadata blocks
/// (block 0 is frontmatter if present, the rest are leaves in source order),
/// any non-fatal diagnostics, and (if applicable) a first-fence F1 failure
/// captured so the top-level error can be specific.
pub(super) type FenceScan = (Vec<MetadataBlock>, Vec<Diagnostic>, Option<(String, usize)>);

/// Find frontmatter (top `---/---` with `QUILL:`) and leaf code fences
/// (` ```leaf ` info string).
pub(super) fn find_metadata_blocks(markdown: &str) -> Result<FenceScan, ParseError> {
    let lines = Lines::new(markdown);
    let mut blocks: Vec<MetadataBlock> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    let mut first_fence_issue: Option<(String, usize)> = None;

    // ── Step 1: frontmatter ──────────────────────────────────────────────────
    // Scan all F2-valid `---/---` blocks; the first with `QUILL:` first key is
    // the frontmatter. Any prior `---/---` blocks are CommonMark thematic
    // breaks (with a near-miss warning if the first key looks like a typo).
    let mut post_frontmatter_k: usize = 0;
    let mut k: usize = 0;
    while k < lines.len() {
        let text = lines.line_text(k);
        if !is_fence_marker_line(text) {
            k += 1;
            continue;
        }
        let f2_ok = k == 0 || lines.is_blank(k - 1);
        if !f2_ok {
            k += 1;
            continue;
        }
        let closer_k =
            (k + 1..lines.len()).find(|&j| is_fence_marker_line(lines.line_text(j)));

        let content_start = lines.line_end_inclusive(k);
        let content_end = closer_k
            .map(|cj| lines.line_start(cj))
            .unwrap_or(markdown.len());
        let content = &markdown[content_start..content_end];
        let key = first_content_key(content);

        if key == Some("QUILL") {
            let Some(cj) = closer_k else {
                return Err(ParseError::InvalidStructure(
                    "Frontmatter block started but not closed with ---".to_string(),
                ));
            };
            let abs_pos = lines.line_start(k);
            let block_end = lines.line_end_inclusive(cj);
            let block = super::assemble::build_block(
                markdown,
                abs_pos,
                content_start,
                content_end,
                block_end,
                0,
                false,
            )?;
            blocks.push(block);
            post_frontmatter_k = cj + 1;
            break;
        }
        // Record the first non-QUILL first-key candidate so the top-level
        // MissingQuillField error can be specific (case-hint vs ordering-hint).
        if let Some(actual) = key {
            if first_fence_issue.is_none() {
                if actual.eq_ignore_ascii_case("QUILL") {
                    warnings.push(
                        Diagnostic::new(
                            Severity::Warning,
                            format!(
                                "Near-miss frontmatter sentinel `{}:` at line {} — expected `QUILL:` (uppercase).",
                                actual, k + 1
                            ),
                        )
                        .with_code("parse::near_miss_sentinel".to_string()),
                    );
                }
                first_fence_issue = Some((actual.to_string(), k + 1));
            }
        }
        // Not frontmatter — skip past this opener (and closer if present).
        match closer_k {
            Some(cj) => k = cj + 1,
            None => break,
        }
    }

    // ── Step 2: leaves ───────────────────────────────────────────────────────
    // Two paths interleave by source order:
    //
    // - **Canonical**: CommonMark fenced code block whose info-string first
    //   token is `leaf`, body keyed by `KIND:`.
    // - **Legacy (Release N only, LEAF_REWORK.md §7)**: `---/---` block (F2)
    //   whose first body key is `CARD:`. Each occurrence emits a
    //   `parse::deprecated_leaf_syntax` warning; the canonical emitter
    //   rewrites it to ` ```leaf ` on round-trip.
    let mut k = post_frontmatter_k;
    let mut open_code_fence: Option<(u8, usize, usize)> = None;
    while k < lines.len() {
        let text = lines.line_text(k);

        if let Some((ch, min, _)) = open_code_fence {
            if let Some((_, _, true)) = code_fence_on_line(text, Some((ch, min))) {
                open_code_fence = None;
            }
            k += 1;
            continue;
        }

        // Legacy `---/CARD: …/---` leaf — Release-N migration path.
        if is_fence_marker_line(text) && (k == 0 || lines.is_blank(k - 1)) {
            if let Some(cj) =
                (k + 1..lines.len()).find(|&j| is_fence_marker_line(lines.line_text(j)))
            {
                let content_start = lines.line_end_inclusive(k);
                let content_end = lines.line_start(cj);
                let content = &markdown[content_start..content_end];
                if first_content_key(content) == Some("CARD") {
                    let abs_pos = lines.line_start(k);
                    let block_end = lines.line_end_inclusive(cj);
                    let block = super::assemble::build_block(
                        markdown,
                        abs_pos,
                        content_start,
                        content_end,
                        block_end,
                        blocks.len(),
                        true,
                    )?;
                    blocks.push(block);
                    warnings.push(
                        Diagnostic::new(
                            Severity::Warning,
                            format!(
                                "Legacy `---/CARD: …/---` leaf at line {} is deprecated; \
                                 round-trip through `Document::to_markdown` to rewrite as \
                                 the canonical ` ```leaf / KIND: … / ``` ` form. The legacy \
                                 path will be removed in the next release.",
                                k + 1
                            ),
                        )
                        .with_code("parse::deprecated_leaf_syntax".to_string()),
                    );
                    k = cj + 1;
                    continue;
                }
            }
        }

        if let Some((ch, run_len, _)) = code_fence_on_line(text, None) {
            if fence_info_first_token(text) == Some("leaf") {
                let opener_k = k;
                let Some(cj) = (k + 1..lines.len()).find(|&j| {
                    matches!(
                        code_fence_on_line(lines.line_text(j), Some((ch, run_len))),
                        Some((_, _, true))
                    )
                }) else {
                    return Err(ParseError::InvalidStructure(format!(
                        "Leaf fence opened at line {} but never closed",
                        opener_k + 1
                    )));
                };

                let abs_pos = lines.line_start(opener_k);
                let content_start = lines.line_end_inclusive(opener_k);
                let content_end = lines.line_start(cj);
                let block_end = lines.line_end_inclusive(cj);

                // Spec §3.2/§9, LEAF_REWORK.md §3.3: leaf-info-string fence
                // commits to leaf parsing; missing or misplaced `KIND:` is a
                // hard error, not a silent classification miss.
                let content = &markdown[content_start..content_end];
                match first_content_key(content) {
                    Some("KIND") => {}
                    Some(other) => {
                        return Err(ParseError::InvalidStructure(format!(
                            "Leaf fence at line {} must have `KIND:` as its first body key (found `{}:`).",
                            opener_k + 1,
                            other
                        )));
                    }
                    None => {
                        return Err(ParseError::InvalidStructure(format!(
                            "Leaf fence at line {} is missing required `KIND:` first body key.",
                            opener_k + 1
                        )));
                    }
                }

                let block = super::assemble::build_block(
                    markdown,
                    abs_pos,
                    content_start,
                    content_end,
                    block_end,
                    blocks.len(),
                    false,
                )?;
                blocks.push(block);
                k = cj + 1;
                continue;
            }
            // Non-leaf fence — shield its contents.
            open_code_fence = Some((ch, run_len, k));
            k += 1;
            continue;
        }

        k += 1;
    }

    let leaf_count = blocks
        .iter()
        .filter(|b| matches!(b.sentinel, BlockSentinel::Leaf(_)))
        .count();
    if leaf_count > crate::error::MAX_LEAF_COUNT {
        return Err(ParseError::InputTooLarge {
            size: leaf_count,
            max: crate::error::MAX_LEAF_COUNT,
        });
    }

    if let Some((_, _, opener_line)) = open_code_fence {
        warnings.push(
            Diagnostic::new(
                Severity::Warning,
                format!(
                    "Unclosed fenced code block opened at line {} — end-of-document reached without a matching closing fence.",
                    opener_line + 1
                ),
            )
            .with_code("parse::unclosed_code_block".to_string()),
        );
    }

    Ok((blocks, warnings, first_fence_issue))
}
