//! Line-oriented fence scanner for Quillmark card-yaml blocks.
//!
//! Detects `~~~card-yaml` metadata blocks and skips over ordinary CommonMark
//! fenced code blocks so that a `~~~card-yaml` opener inside a code block is
//! treated as literal content, not a card-yaml block.

use crate::error::ParseError;
use crate::{Diagnostic, Severity};

use super::assemble::MetadataBlock;

/// The exact info string that promotes a `~~~` fence to a card-yaml block.
const CARD_YAML_INFO: &str = "card-yaml";

/// Line-oriented view of the source, used for fence detection.
pub(super) struct Lines<'a> {
    pub(super) source: &'a str,
    pub(super) starts: Vec<usize>, // byte offset of each line's first character
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
    /// Byte position immediately after line k's trailing `\n` (or end-of-source
    /// if no newline follows).
    pub(super) fn line_end_inclusive(&self, k: usize) -> usize {
        if k + 1 < self.starts.len() {
            self.starts[k + 1]
        } else {
            self.source.len()
        }
    }
    /// Line text without its trailing line ending.
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

/// Detect a CommonMark fenced code-block marker line. Returns `Some((char,
/// run_len, is_closing))` if the line opens (or, given `open_fence`, closes) a
/// fence.
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

/// Extract the info string of a CommonMark fenced code-block opener line.
///
/// `run_len` is the fence-marker run length reported by [`code_fence_on_line`].
/// The returned slice is the text after the run, trimmed of surrounding
/// whitespace — e.g. for `~~~card-yaml` it returns `card-yaml`.
pub(super) fn code_fence_info(line: &str, run_len: usize) -> &str {
    let indent = line.as_bytes().iter().take_while(|&&b| b == b' ').count();
    line[indent + run_len..].trim()
}

/// `true` when `line` opens a card-yaml metadata block — a `~~~` fence
/// (exactly three tildes) whose info string is exactly `card-yaml`.
fn is_card_yaml_opener(line: &str) -> bool {
    match code_fence_on_line(line, None) {
        Some((b'~', 3, false)) => code_fence_info(line, 3) == CARD_YAML_INFO,
        _ => false,
    }
}

/// Outcome of the fence-detection pass: the recognised metadata blocks and any
/// non-fatal diagnostics accumulated along the way.
pub(super) type FenceScan = (Vec<MetadataBlock>, Vec<Diagnostic>);

/// Find all `~~~card-yaml` metadata blocks in the document.
///
/// A card-yaml block opens with `~~~card-yaml`, requires a blank line above it
/// (so body round-tripping stays stable), and closes with `~~~`. Openers
/// inside ordinary fenced code blocks are ignored.
pub(super) fn find_metadata_blocks(markdown: &str) -> Result<FenceScan, ParseError> {
    let lines = Lines::new(markdown);
    let mut blocks: Vec<MetadataBlock> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    // (char, run_len, opener_line_index) of an open ordinary code fence.
    let mut open_code_fence: Option<(u8, usize, usize)> = None;

    let mut k: usize = 0;
    while k < lines.len() {
        let text = lines.line_text(k);

        // Inside an ordinary fenced code block: `~~~card-yaml` openers here
        // are literal content, ignored until the code fence closes.
        if let Some((ch, min, _opener)) = open_code_fence {
            if let Some((_, _, true)) = code_fence_on_line(text, Some((ch, min))) {
                open_code_fence = None;
            }
            k += 1;
            continue;
        }

        // A card-yaml opener?
        if is_card_yaml_opener(text) {
            // Leading-blank rule: a card-yaml block is a block-level construct
            // and requires a blank line above it. Without one it is delegated
            // to CommonMark as an ordinary `~~~` code block.
            let blank_above = k == 0 || lines.is_blank(k - 1);
            if !blank_above {
                warnings.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "`~~~card-yaml` block at line {} has no blank line above it — it is treated as an ordinary code block, not a card-yaml block. Insert a blank line before it to register it.",
                            k + 1
                        ),
                    )
                    .with_code("parse::card_fence_missing_blank".to_string()),
                );
                open_code_fence = Some((b'~', 3, k));
                k += 1;
                continue;
            }

            // Scan for the matching `~~~` closer.
            let mut closer_k: Option<usize> = None;
            let mut j = k + 1;
            while j < lines.len() {
                if let Some((_, _, true)) =
                    code_fence_on_line(lines.line_text(j), Some((b'~', 3)))
                {
                    closer_k = Some(j);
                    break;
                }
                j += 1;
            }
            let Some(cj) = closer_k else {
                return Err(ParseError::InvalidStructure(
                    "card-yaml block opened with `~~~card-yaml` but never closed with `~~~`"
                        .to_string(),
                ));
            };

            let block = super::assemble::build_block(
                markdown,
                lines.line_start(k),
                lines.line_end_inclusive(k),
                lines.line_start(cj),
                lines.line_end_inclusive(cj),
                blocks.len(),
            )?;
            blocks.push(block);
            k = cj + 1;
            continue;
        }

        // Any other fence opener is an ordinary fenced code block.
        if let Some((ch, run_len, _)) = code_fence_on_line(text, None) {
            open_code_fence = Some((ch, run_len, k));
        }
        k += 1;
    }

    // Card-count check counts composable card blocks — every block after the
    // root (spec §8).
    let card_count = blocks.len().saturating_sub(1);
    if card_count > crate::error::MAX_CARD_COUNT {
        return Err(ParseError::InputTooLarge {
            size: card_count,
            max: crate::error::MAX_CARD_COUNT,
        });
    }

    // Unclosed fenced code block at end-of-document: any card-yaml blocks
    // below the unclosed opener were silently shielded, which is almost never
    // what the author intended. Surface it as a non-fatal warning.
    if let Some((_, _, opener_line)) = open_code_fence {
        warnings.push(
            Diagnostic::new(
                Severity::Warning,
                format!(
                    "Unclosed fenced code block opened at line {} — end-of-document reached without a matching closing fence. Any `~~~card-yaml` blocks after this line were treated as code and not parsed.",
                    opener_line + 1
                ),
            )
            .with_code("parse::unclosed_code_block".to_string()),
        );
    }

    Ok((blocks, warnings))
}
