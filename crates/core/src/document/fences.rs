//! Line-oriented fence scanner for Quillmark card-yaml blocks.
//!
//! Detects `~~~` metadata blocks (the canonical card-yaml fence) and skips
//! over ordinary CommonMark fenced code blocks so that a `~~~` opener inside a
//! code block is treated as literal content, not a card-yaml block. A
//! column-zero `~~~` fence (three or more tildes, no info string) opens a
//! card-yaml block; the canonical opener is three tildes (`to_markdown`
//! always emits `~~~`) and the legacy `~~~card-yaml` info string is also
//! accepted but is no longer canonical. To write a literal fenced *code* block
//! in prose, use a backtick fence (or a `~~~` fence with a language info
//! string).

use crate::error::ParseError;
use crate::{Diagnostic, Severity};

use super::assemble::MetadataBlock;

/// The legacy info string that also opens a card-yaml block. Accepted on input
/// for backward compatibility but never emitted — canonical openers are bare
/// `~~~` (see [`card_yaml_opener_run`]).
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

/// If `line` opens a card-yaml metadata block, returns the tilde-run length of
/// the opener (`>= 3`); otherwise `None`.
///
/// A card-yaml opener is a tilde fence (three **or more** tildes) at **column
/// zero** (spec §3.2) whose info string is empty (the canonical form) or
/// exactly `card-yaml` (the accepted, non-canonical legacy alias). The
/// canonical opener is three tildes — `to_markdown` always emits `~~~` — but a
/// longer run is accepted and normalised on emit; its closer must be at least
/// as long (the run length is threaded into the closer scan to honour
/// CommonMark's fence-matching rule). To write a literal fenced *code* block in
/// prose, use a backtick fence: backtick fences and `~~~` fences carrying any
/// other info string (e.g. a language) are never card-yaml openers.
///
/// An indented `~~~` (1–3 leading spaces) is still a valid CommonMark code
/// fence, but it is *not* a card-yaml opener — recognising it would both
/// contradict the spec and split at an offset the body renderer disagrees with.
fn card_yaml_opener_run(line: &str) -> Option<usize> {
    if line.starts_with(' ') {
        return None;
    }
    match code_fence_on_line(line, None) {
        Some((b'~', run, false)) => {
            let info = code_fence_info(line, run);
            (info.is_empty() || info == CARD_YAML_INFO).then_some(run)
        }
        _ => None,
    }
}

/// `true` when `line` has the shape of a card-yaml opener (see
/// [`card_yaml_opener_run`]). Used by the `Quill.yaml` `body.example` guard so
/// the blueprint-corruption check stays in lock-step with the parser.
pub(crate) fn is_card_yaml_opener_line(line: &str) -> bool {
    card_yaml_opener_run(line).is_some()
}

/// `true` when `line` is a `---` YAML-frontmatter fence line — exactly three
/// dashes at column zero, followed only by whitespace.
///
/// `---` is accepted ONLY as the root-block opener/closer (see
/// [`find_metadata_blocks`]); it is never a composable-card fence. It must be
/// at column zero, matching the YAML-metadata-block semantics of CommonMark
/// renderers (an indented `---` is a thematic break, not frontmatter).
fn is_dash_fence_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'-' {
        return false;
    }
    let run_len = bytes.iter().take_while(|&&b| b == b'-').count();
    if run_len != 3 {
        return false;
    }
    line[run_len..].chars().all(|c| c == ' ' || c == '\t')
}

/// `true` when `line` looks like a YAML mapping key (e.g. `key: value` or
/// `$key: value`). Used to disambiguate a stray `---` thematic break from a
/// would-be composable-card block.
fn looks_like_yaml_key_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }
    let bytes = trimmed.as_bytes();
    let mut i;
    if bytes[0] == b'$' {
        if bytes.len() < 2 || !(bytes[1].is_ascii_alphabetic() || bytes[1] == b'_') {
            return false;
        }
        i = 2;
    } else if bytes[0].is_ascii_alphabetic() || bytes[0] == b'_' {
        i = 1;
    } else {
        return false;
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b':'
}

/// `true` when a matching `---` line appears further down with at least one
/// YAML-key-shaped line between the two `---` markers.
///
/// Used after the root block has already been parsed: such a paired `---`
/// block is almost certainly a misplaced composable card.
fn has_paired_dash_with_yaml_keys(lines: &Lines<'_>, opener_k: usize) -> bool {
    let mut saw_yaml_key = false;
    let mut j = opener_k + 1;
    while j < lines.len() {
        let text = lines.line_text(j);
        if is_dash_fence_line(text) {
            return saw_yaml_key;
        }
        if looks_like_yaml_key_line(text) {
            saw_yaml_key = true;
        }
        j += 1;
    }
    false
}

/// Outcome of the fence-detection pass: the recognised metadata blocks and any
/// non-fatal diagnostics accumulated along the way.
pub(super) type FenceScan = (Vec<MetadataBlock>, Vec<Diagnostic>);

/// Find all `~~~` card-yaml metadata blocks in the document.
///
/// A card-yaml block opens with `~~~` (or the legacy `~~~card-yaml` alias),
/// requires a blank line above it (so body round-tripping stays stable), and
/// closes with `~~~`. Openers inside ordinary fenced code blocks are ignored.
pub(super) fn find_metadata_blocks(markdown: &str) -> Result<FenceScan, ParseError> {
    let lines = Lines::new(markdown);
    let mut blocks: Vec<MetadataBlock> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    // (char, run_len, opener_line_index) of an open ordinary code fence.
    let mut open_code_fence: Option<(u8, usize, usize)> = None;

    let mut k: usize = 0;
    while k < lines.len() {
        let text = lines.line_text(k);

        // Inside an ordinary fenced code block: `~~~` openers here
        // are literal content, ignored until the code fence closes.
        if let Some((ch, min, _opener)) = open_code_fence {
            if let Some((_, _, true)) = code_fence_on_line(text, Some((ch, min))) {
                open_code_fence = None;
            }
            k += 1;
            continue;
        }

        // A card-yaml opener?
        if let Some(open_run) = card_yaml_opener_run(text) {
            // Leading-blank rule: a card-yaml block is a block-level construct
            // and requires a blank line above it. Without one it is delegated
            // to CommonMark as an ordinary `~~~` code block.
            let blank_above = k == 0 || lines.is_blank(k - 1);
            if !blank_above {
                warnings.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "`~~~` card-yaml block at line {} has no blank line above it — it is treated as an ordinary code block, not a card-yaml block. Insert a blank line before it to register it.",
                            k + 1
                        ),
                    )
                    .with_code("parse::card_fence_missing_blank".to_string()),
                );
                open_code_fence = Some((b'~', open_run, k));
                k += 1;
                continue;
            }

            // Scan for the matching `~~~` closer. A closer must be a tilde run
            // at least as long as the opener (CommonMark fence matching), so a
            // shorter `~~~` inside a longer-fenced block stays payload.
            let mut closer_k: Option<usize> = None;
            let mut j = k + 1;
            while j < lines.len() {
                if let Some((_, _, true)) =
                    code_fence_on_line(lines.line_text(j), Some((b'~', open_run)))
                {
                    closer_k = Some(j);
                    break;
                }
                j += 1;
            }
            let Some(cj) = closer_k else {
                // No closer before EOF. Per CommonMark an unclosed `~~~` fence
                // is an ordinary fenced code block running to end of document,
                // not a card-yaml block — so delegate it rather than erroring.
                // Shielding here also lets the end-of-document check below
                // surface the unclosed-fence warning.
                open_code_fence = Some((b'~', open_run, k));
                k += 1;
                continue;
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

        // `---` YAML-frontmatter style: accepted ONLY for the root block.
        //
        // Two cases:
        //   * `blocks.is_empty()` and this is the first non-blank construct we
        //     hit: treat as the root opener, scan for a `---` closer, build a
        //     block. (Mixed openers like `---` … `~~~` are NOT accepted: a
        //     `---` opener requires a `---` closer.)
        //   * Otherwise: if it looks like a would-be composable card
        //     (paired `---` … `---` with YAML-key content between, blank
        //     line above), reject with the standard "expected `~~~card-yaml`"
        //     error so LLM authors don't silently get a body-text result.
        //     Otherwise fall through and let CommonMark treat the line as
        //     prose (thematic break / setext underline).
        if is_dash_fence_line(text) {
            let blank_above = k == 0 || lines.is_blank(k - 1);

            if blocks.is_empty() && blank_above {
                // Only accept `---` as the root opener if we have not yet
                // seen any prose content (i.e. the `---` is at document
                // start, modulo leading blank lines). The pre-block prefix
                // is required to be blank for the root case so we don't
                // race against setext headings or thematic breaks deep in
                // a prose preamble. `blocks.is_empty()` plus "every line
                // above is blank" guarantees this is document-start.
                let above_all_blank = (0..k).all(|i| lines.is_blank(i));
                if above_all_blank {
                    // Scan for the matching `---` closer.
                    let mut closer_k: Option<usize> = None;
                    let mut j = k + 1;
                    while j < lines.len() {
                        if is_dash_fence_line(lines.line_text(j)) {
                            closer_k = Some(j);
                            break;
                        }
                        j += 1;
                    }
                    let Some(cj) = closer_k else {
                        // No matching `---` closer: per CommonMark a lone
                        // leading `---` is a thematic break, not frontmatter.
                        // Fall through (no root block is built here, so the
                        // document surfaces MissingQuill downstream).
                        k += 1;
                        continue;
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
            }

            // After the root block: a `---` line that pairs with another
            // `---` further down AND has YAML-key content between is almost
            // certainly a misplaced composable-card attempt — surface the
            // standard error rather than silently treating it as body text.
            if !blocks.is_empty() && blank_above && has_paired_dash_with_yaml_keys(&lines, k) {
                return Err(ParseError::InvalidStructure(
                    "Composable card block opened with `---` but composable cards \
                     must use `~~~` fences. Replace the opening `---` and the \
                     closing `---` with `~~~` (three tildes, no info string). The \
                     `---` style is accepted only for the document's root block."
                        .to_string(),
                ));
            }

            // Fall through: a lone `---` is delegated to CommonMark as a
            // thematic break / setext underline. Do not treat as a code
            // fence opener.
            k += 1;
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
                    "Unclosed fenced code block opened at line {} — end-of-document reached without a matching closing fence. Any `~~~` card-yaml blocks after this line were treated as code and not parsed.",
                    opener_line + 1
                ),
            )
            .with_code("parse::unclosed_code_block".to_string()),
        );
    }

    Ok((blocks, warnings))
}
