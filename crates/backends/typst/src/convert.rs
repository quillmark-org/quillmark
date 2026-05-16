//! # Markdown to Typst Conversion
//!
//! This module transforms CommonMark markdown into Typst markup language.
//!
//! ## Key Functions
//!
//! - [`mark_to_typst()`] - Primary conversion function for Markdown to Typst
//! - [`escape_markup()`] - Escapes text for safe use in Typst markup context
//! - [`escape_string()`] - Escapes text for embedding in Typst string literals
//!
//! ## Quick Example
//!
//! ```
//! use quillmark_typst::convert::mark_to_typst;
//!
//! let markdown = "This is **bold** and _italic_.";
//! let typst = mark_to_typst(markdown).unwrap();
//! // Output: "This is #strong[bold] and #emph[italic].\n\n"
//! ```
//!
//! ## Detailed Documentation
//!
//! For comprehensive conversion details including:
//! - Character escaping strategies
//! - CommonMark feature coverage  
//! - Event-based conversion flow
//! - Implementation notes
//!
//! See [CONVERT.md](https://github.com/nibsbin/quillmark/blob/main/quillmark-typst/docs/designs/CONVERT.md) for the complete specification.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use quillmark_core::error::MAX_NESTING_DEPTH;
use std::ops::Range;

/// Errors that can occur during markdown to Typst conversion
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    /// Nesting depth exceeded maximum allowed
    #[error("Nesting too deep: {depth} levels (max: {max} levels)")]
    NestingTooDeep {
        /// Actual depth
        depth: usize,
        /// Maximum allowed depth
        max: usize,
    },
}

/// Escapes text for safe use in Typst markup context.
///
/// This function escapes all Typst-special characters to prevent:
/// - Markup injection (*, _, `, #, etc.)
/// - Layout manipulation (~, which is non-breaking space in Typst)
/// - Reference injection (@)
/// - Code/comment injection (//, $, {, }, etc.)
pub fn escape_markup(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace("//", "\\/\\/")
        .replace('~', "\\~") // Non-breaking space in Typst
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
        .replace('#', "\\#")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('$', "\\$")
        .replace('<', "\\<")
        .replace('>', "\\>")
        .replace('@', "\\@")
}

/// Escapes text for embedding in Typst string literals.
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Escape other ASCII controls with \u{..}
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone)]
enum ListType {
    Bullet,
    Ordered,
}

#[derive(Debug, Clone, Copy)]
enum StrongKind {
    Bold,      // Source was ** or __
    Underline, // Source was <u> (synthesized by MarkdownFixer)
}

fn typst_alignment(align: &pulldown_cmark::Alignment) -> &'static str {
    match align {
        pulldown_cmark::Alignment::None => "auto",
        pulldown_cmark::Alignment::Left => "left",
        pulldown_cmark::Alignment::Center => "center",
        pulldown_cmark::Alignment::Right => "right",
    }
}

/// Returns true if the HTML string is a `<u>` open tag (tolerates whitespace and case).
fn is_u_open_tag(html: &str) -> bool {
    let s = html.trim();
    // Accept <u>, <U>, <u >, <U > (arbitrary whitespace before >)
    if s.starts_with('<') && s.ends_with('>') {
        let inner = s[1..s.len() - 1].trim();
        inner.eq_ignore_ascii_case("u")
    } else {
        false
    }
}

/// Returns true if the HTML string is a `</u>` close tag (tolerates whitespace and case).
fn is_u_close_tag(html: &str) -> bool {
    let s = html.trim();
    // Accept </u>, </U>, </u >, </U >
    if s.starts_with("</") && s.ends_with('>') {
        let inner = s[2..s.len() - 1].trim();
        inner.eq_ignore_ascii_case("u")
    } else {
        false
    }
}

/// Sanitizes a code-block language tag for safe inclusion in Typst raw blocks.
///
/// Only allows alphanumeric characters, hyphens, underscores, dots, and plus signs,
/// which are the characters commonly found in language identifiers (e.g., "c++",
/// "objective-c", "file.typ"). Characters like `#`, newlines, backticks, and other
/// Typst-special characters are treated as the end of the valid identifier.
/// This prevents injection via newlines or Typst-special characters in the info string.
fn sanitize_lang_tag(lang: &str) -> String {
    lang.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

/// Returns the length of the longest consecutive run of backtick characters in the string.
/// Used to determine how many backticks are needed for safe Typst raw text delimiters.
fn longest_backtick_run(s: &str) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;
    for ch in s.chars() {
        if ch == '`' {
            current_run += 1;
            if current_run > max_run {
                max_run = current_run;
            }
        } else {
            current_run = 0;
        }
    }
    max_run
}

/// Converts an iterator of markdown events to Typst markup
fn push_typst<'a, I>(output: &mut String, source: &str, iter: I) -> Result<(), ConversionError>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    let mut end_newline = true;
    let mut list_stack: Vec<ListType> = Vec::new();
    let mut strong_stack: Vec<StrongKind> = Vec::new();
    let mut in_list_item = false; // Track if we're inside a list item
    let mut list_item_first_block = false; // Track if we're on the first block of a list item
    let mut in_code_block = false; // Track if we're inside a code block
    let mut table_alignments: Vec<pulldown_cmark::Alignment> = Vec::new(); // Column alignments for current table
    let mut depth = 0; // Track nesting depth for DoS prevention
    let mut in_image = false; // Suppress text events inside ![alt](src)
    let iter = iter.peekable();

    for (event, range) in iter {
        match event {
            Event::Start(tag) => {
                // Track nesting depth
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Err(ConversionError::NestingTooDeep {
                        depth,
                        max: MAX_NESTING_DEPTH,
                    });
                }

                match tag {
                    Tag::Paragraph => {
                        if !in_list_item {
                            // Don't add extra newlines if we're already at start of line
                            if !end_newline {
                                output.push('\n');
                                end_newline = true;
                            }
                        } else if !list_item_first_block {
                            // Continuation paragraph in list item: blank line + indent
                            let cont_indent = "  ".repeat(list_stack.len());
                            if !end_newline {
                                output.push('\n');
                            }
                            output.push('\n');
                            output.push_str(&cont_indent);
                            end_newline = false;
                        }
                    }
                    Tag::CodeBlock(kind) => {
                        in_code_block = true;
                        if in_list_item {
                            // Code block inside list item: continuation indent
                            let cont_indent = "  ".repeat(list_stack.len());
                            if !list_item_first_block {
                                if !end_newline {
                                    output.push('\n');
                                }
                                output.push('\n');
                            } else if !end_newline {
                                output.push('\n');
                            }
                            output.push_str(&cont_indent);
                            list_item_first_block = false;
                        } else if !end_newline {
                            output.push('\n');
                        }
                        output.push_str("```");
                        if let pulldown_cmark::CodeBlockKind::Fenced(lang) = kind {
                            let sanitized = sanitize_lang_tag(&lang);
                            if !sanitized.is_empty() {
                                output.push_str(&sanitized);
                            }
                        }
                        output.push('\n');
                        end_newline = true;
                    }
                    Tag::HtmlBlock => {
                        // HTML blocks are handled, no special tracking needed
                    }
                    Tag::List(start_number) => {
                        if !end_newline {
                            output.push('\n');
                            end_newline = true;
                        }

                        let list_type = if start_number.is_some() {
                            ListType::Ordered
                        } else {
                            ListType::Bullet
                        };

                        list_stack.push(list_type);
                    }
                    Tag::Item => {
                        in_list_item = true;
                        list_item_first_block = true;
                        if let Some(list_type) = list_stack.last() {
                            let indent = "  ".repeat(list_stack.len().saturating_sub(1));

                            match list_type {
                                ListType::Bullet => {
                                    output.push_str(&format!("{}- ", indent));
                                }
                                ListType::Ordered => {
                                    output.push_str(&format!("{}+ ", indent));
                                }
                            }
                            end_newline = false;
                        }
                    }
                    Tag::Emphasis => {
                        output.push_str("#emph[");
                        end_newline = false;
                    }
                    Tag::Strong => {
                        // <u>…</u> is synthesized as Tag::Strong by MarkdownFixer; detect it
                        // by peeking at the source. Both ** and __ render as #strong[…].
                        let kind = if range.start + 2 <= source.len()
                            && source[range.start..range.start + 2].eq_ignore_ascii_case("<u")
                        {
                            StrongKind::Underline
                        } else {
                            StrongKind::Bold
                        };
                        strong_stack.push(kind);
                        match kind {
                            StrongKind::Underline => output.push_str("#underline["),
                            StrongKind::Bold => output.push_str("#strong["),
                        }
                        end_newline = false;
                    }
                    Tag::Strikethrough => {
                        output.push_str("#strike[");
                        end_newline = false;
                    }
                    Tag::Link {
                        dest_url, title: _, ..
                    } => {
                        output.push_str("#link(\"");
                        output.push_str(&escape_string(&dest_url));
                        output.push_str("\")[");
                        end_newline = false;
                    }
                    Tag::Image {
                        dest_url, title: _, ..
                    } => {
                        // Spec §6.3: images are required for v1. Emit #image("url") and
                        // suppress alt-text events until TagEnd::Image.
                        output.push_str("#image(\"");
                        output.push_str(&escape_string(&dest_url));
                        output.push_str("\")");
                        in_image = true;
                        end_newline = false;
                    }
                    Tag::Heading { level, .. } => {
                        if !end_newline {
                            output.push('\n');
                        }
                        let equals = "=".repeat(level as usize);
                        output.push_str(&equals);
                        output.push(' ');
                        end_newline = false;
                    }
                    Tag::Table(alignments) => {
                        if !end_newline {
                            output.push('\n');
                        }
                        let col_count = alignments.len();
                        output.push_str(&format!("#table(\n  columns: {},\n", col_count));
                        // Emit align array if any column has non-default alignment
                        if alignments
                            .iter()
                            .any(|a| !matches!(a, pulldown_cmark::Alignment::None))
                        {
                            output.push_str("  align: (");
                            for (i, align) in alignments.iter().enumerate() {
                                if i > 0 {
                                    output.push_str(", ");
                                }
                                output.push_str(typst_alignment(align));
                            }
                            output.push_str("),\n");
                        }
                        table_alignments = alignments;
                        end_newline = false;
                    }
                    Tag::TableHead => {
                        output.push_str("  table.header(");
                        end_newline = false;
                    }
                    Tag::TableRow => {
                        output.push_str("  ");
                        end_newline = false;
                    }
                    Tag::TableCell => {
                        output.push('[');
                        end_newline = false;
                    }
                    _ => {
                        // Ignore other start tags not in requirements
                    }
                }
            }
            Event::End(tag) => {
                // Decrement depth
                depth = depth.saturating_sub(1);

                match tag {
                    TagEnd::Paragraph => {
                        if !in_list_item {
                            output.push('\n');
                            output.push('\n'); // Extra newline for paragraph separation
                            end_newline = true;
                        } else {
                            // End of a block within a list item
                            list_item_first_block = false;
                            if !end_newline {
                                output.push('\n');
                                end_newline = true;
                            }
                        }
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        if !end_newline {
                            output.push('\n');
                        }
                        if in_list_item {
                            let cont_indent = "  ".repeat(list_stack.len());
                            output.push_str(&cont_indent);
                        }
                        output.push_str("```\n");
                        if !in_list_item {
                            output.push('\n');
                        }
                        end_newline = true;
                        list_item_first_block = false;
                    }
                    TagEnd::HtmlBlock => {
                        // HTML blocks are handled, no special tracking needed
                    }
                    TagEnd::List(_) => {
                        list_stack.pop();
                        if list_stack.is_empty() {
                            output.push('\n');
                            end_newline = true;
                        }
                    }
                    TagEnd::Item => {
                        in_list_item = false;
                        list_item_first_block = false;
                        if !end_newline {
                            output.push('\n');
                            end_newline = true;
                        }
                    }
                    TagEnd::Emphasis => {
                        output.push(']');
                        end_newline = false;
                    }
                    TagEnd::Strong => {
                        match strong_stack.pop() {
                            Some(StrongKind::Bold) | Some(StrongKind::Underline) => {
                                output.push(']');
                            }
                            None => {
                                // Malformed: more end tags than start tags — skip
                                // to avoid producing an unmatched ']'
                            }
                        }
                        end_newline = false;
                    }
                    TagEnd::Strikethrough => {
                        output.push(']');
                        end_newline = false;
                    }
                    TagEnd::Link => {
                        output.push(']');
                        end_newline = false;
                    }
                    TagEnd::Image => {
                        // Alt text was suppressed; just clear the in_image flag.
                        in_image = false;
                    }
                    TagEnd::Heading(_) => {
                        output.push('\n');
                        output.push('\n'); // Extra newline after heading
                        end_newline = true;
                    }
                    TagEnd::Table => {
                        output.push_str(")\n\n");
                        table_alignments.clear();
                        end_newline = true;
                    }
                    TagEnd::TableHead => {
                        output.push_str("),\n");
                        end_newline = false;
                    }
                    TagEnd::TableRow => {
                        output.push('\n');
                        end_newline = true;
                    }
                    TagEnd::TableCell => {
                        output.push_str("], ");
                        end_newline = false;
                    }
                    _ => {
                        // Ignore other end tags not in requirements
                    }
                }
            }
            Event::Text(text) => {
                if in_image {
                    // Suppress alt text inside ![alt](src) — spec §6.3
                } else if in_code_block {
                    // Code block content - no escaping needed
                    output.push_str(&text);
                    end_newline = text.ends_with('\n');
                } else {
                    let escaped = escape_markup(&text);
                    output.push_str(&escaped);
                    end_newline = escaped.ends_with('\n');
                }
            }
            Event::Code(text) => {
                // Inline code: use enough backticks to avoid delimiter collision
                let max_run = longest_backtick_run(&text);
                let delim_len = max_run + 1;
                let delim: String = std::iter::repeat('`').take(delim_len).collect();
                output.push_str(&delim);
                // When using multi-backtick delimiters, Typst requires spaces
                // to separate the delimiters from the content
                if delim_len > 1 {
                    output.push(' ');
                }
                output.push_str(&text);
                if delim_len > 1 {
                    output.push(' ');
                }
                output.push_str(&delim);
                end_newline = false;
            }
            Event::HardBreak => {
                output.push_str("#linebreak()");
                end_newline = false;
            }
            Event::SoftBreak => {
                output.push(' ');
                end_newline = false;
            }
            _ => {
                // Ignore other events not specified in requirements
                // (math, footnotes, etc.)
                // Note: per spec §6.2, raw HTML produces no output except <u>…</u>,
                // which MarkdownFixer rewrites to Start/End(Tag::Strong) and is detected
                // as Underline in the Tag::Strong handler above.
            }
        }
    }

    Ok(())
}

/// Iterator that post-processes markdown events to handle two edge cases:
/// 1. Allowlists `<u>…</u>` as underline; strips all other raw HTML.
/// 2. Fixes `***` adjacency issues.
struct MarkdownFixer<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>> {
    inner: std::iter::Peekable<I>,
    source: &'a str,
    buffer: Vec<(Event<'a>, Range<usize>)>,
    emph_depth: usize,
    strong_depth: usize,
}

impl<'a, I> MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    fn new(inner: I, source: &'a str) -> Self {
        Self {
            inner: inner.peekable(),
            source,
            buffer: Vec::new(),
            emph_depth: 0,
            strong_depth: 0,
        }
    }

    /// Helper to generate events for a run of stars
    fn events_for_stars(
        star_count: usize,
        is_start: bool,
        start_idx: usize,
    ) -> Vec<(Event<'a>, Range<usize>)> {
        let mut events = Vec::new();
        let mut offset = 0;
        let mut remaining = star_count;

        // 3 stars = Strong + Emph (***)
        // 2 stars = Strong (**)
        // 1 star = Emph (*)

        if remaining >= 2 {
            let len = 2;
            let range = start_idx + offset..start_idx + offset + len;
            let event = if is_start {
                Event::Start(Tag::Strong)
            } else {
                Event::End(TagEnd::Strong)
            };
            events.push((event, range));
            remaining -= 2;
            offset += 2;
        }

        if remaining >= 1 {
            let len = 1;
            let range = start_idx + offset..start_idx + offset + len;
            let event = if is_start {
                Event::Start(Tag::Emphasis)
            } else {
                Event::End(TagEnd::Emphasis)
            };
            events.push((event, range));
        }

        // For closing tags, we need to reverse the order to close inner then outer
        // Opened: Strong, Emph -> Closes: Emph, Strong
        if !is_start {
            events.reverse();
        }

        events
    }

    /// Coalesce consecutive Text events into a single range.
    /// Returns the merged range covering all adjacent Text events.
    fn coalesce_text_range(&mut self, initial_range: Range<usize>) -> Range<usize> {
        let mut merged_range = initial_range;

        // Keep consuming Text events as long as they're adjacent
        while let Some((next_event, next_range)) = self.inner.peek() {
            if matches!(next_event, Event::Text(_)) && next_range.start == merged_range.end {
                merged_range.end = next_range.end;
                self.inner.next(); // Consume the peeked event
            } else {
                break;
            }
        }

        merged_range
    }

    /// Count how many unclosed emphasis/strong tags the trailing stars could close.
    /// Returns the number of stars that can be consumed as closing events.
    fn closable_star_count(&self, star_count: usize) -> usize {
        let mut remaining = star_count;
        let mut consumed = 0;

        // 2 stars close a Strong, 1 star closes an Emphasis
        // Match greedily: try Strong first (2 stars), then Emphasis (1 star)
        if remaining >= 2 && self.strong_depth > 0 {
            remaining -= 2;
            consumed += 2;
        }
        if remaining >= 1 && self.emph_depth > 0 {
            consumed += 1;
        }

        consumed
    }

    fn handle_candidate(
        &mut self,
        candidate: (Event<'a>, Range<usize>),
    ) -> Option<(Event<'a>, Range<usize>)> {
        let (event, range) = candidate;

        // Track emphasis/strong nesting depth
        match &event {
            Event::Start(Tag::Emphasis) => self.emph_depth += 1,
            Event::Start(Tag::Strong) => self.strong_depth += 1,
            Event::End(TagEnd::Emphasis) => self.emph_depth = self.emph_depth.saturating_sub(1),
            Event::End(TagEnd::Strong) => self.strong_depth = self.strong_depth.saturating_sub(1),
            _ => {}
        }

        match &event {
            Event::Text(cow_str) => {
                let s = cow_str.as_ref();
                if s.ends_with('*') {
                    // Peek next event
                    let is_strong_start = if let Some(next) = self.buffer.last() {
                        matches!(next.0, Event::Start(Tag::Strong))
                    } else {
                        matches!(self.inner.peek(), Some((Event::Start(Tag::Strong), _)))
                    };

                    if is_strong_start {
                        let star_count = s.chars().rev().take_while(|c| *c == '*').count();
                        if star_count > 0 && star_count <= 3 {
                            let text_len = s.len() - star_count;
                            let text_content = &s[..text_len];
                            // Generate star events
                            let star_events =
                                Self::events_for_stars(star_count, true, range.start + text_len);

                            // Consume next event
                            let next_event = if !self.buffer.is_empty() {
                                self.buffer.pop().unwrap()
                            } else {
                                self.inner.next().unwrap()
                            };

                            // Push reverse
                            self.buffer.push(next_event);
                            for ev in star_events.into_iter().rev() {
                                self.buffer.push(ev);
                            }

                            if !text_content.is_empty() {
                                return Some((
                                    Event::Text(text_content.to_string().into()),
                                    range.start..range.start + text_len,
                                ));
                            } else {
                                return None;
                            }
                        }
                    }
                }
            }
            Event::End(TagEnd::Strong) | Event::End(TagEnd::Emphasis) => {
                // Check if next event starts with *, which means we might need to fix closing tags
                // This happens when we have something like __strong__***
                // The __ produces End(Strong), and following *** should be interpreted as closing.

                // Only apply this fixup if there are still unclosed emphasis/strong tags
                // that the trailing stars could close. Otherwise the stars are literal text
                // (e.g., `*lethality**` where pulldown_cmark already handled the emphasis).
                let has_open_tags = self.emph_depth > 0 || self.strong_depth > 0;
                if !has_open_tags {
                    return Some((event, range));
                }

                // Peek next event (from buffer or inner)
                let next_is_star_text = if let Some((Event::Text(cow_str), _)) = self.buffer.last()
                {
                    cow_str.starts_with('*')
                } else if let Some((Event::Text(cow_str), _)) = self.inner.peek() {
                    cow_str.starts_with('*')
                } else {
                    false
                };

                if next_is_star_text {
                    // Retrieve the text event
                    let (text_event, text_range) = if !self.buffer.is_empty() {
                        self.buffer.pop().unwrap()
                    } else {
                        // Coalesce text from inner iterator
                        let (_ev, rng) = self.inner.next().unwrap();
                        let merged_range = self.coalesce_text_range(rng);
                        let text = self.source[merged_range.clone()].into();
                        (Event::Text(text), merged_range)
                    };

                    if let Event::Text(cow_str) = text_event {
                        let s = cow_str.as_ref();
                        let star_count = s.chars().take_while(|c| *c == '*').count();

                        // Only consume stars that correspond to actually open tags
                        let consumable = self.closable_star_count(star_count);

                        if consumable > 0 {
                            // Perform fix: Close tags using the consumable stars
                            let star_events =
                                Self::events_for_stars(consumable, false, text_range.start);
                            let text_after = &s[consumable..];

                            // We emit the `End(Strong)` event (which caused this check).

                            // We need to push remaining text first (so it comes out last)
                            if !text_after.is_empty() {
                                self.buffer.push((
                                    Event::Text(text_after.to_string().into()),
                                    text_range.start + consumable..text_range.end,
                                ));
                            }

                            // Then push star events (reversed)
                            // Self::events_for_stars returns [End(Emph), End(Strong)] (if 3 stars)
                            // We want End(Strong), then End(Emph) to be popped.
                            // So we push End(Strong) (bottom), then End(Emph) (top).
                            // This is exactly reversed order.
                            for ev in star_events.into_iter().rev() {
                                self.buffer.push(ev);
                            }

                            return Some((event, range));
                        } else {
                            // No open tags to close - put the text back as literal
                            self.buffer.push((Event::Text(cow_str), text_range));
                        }
                    }
                }
            }
            _ => {}
        }

        Some((event, range))
    }
}

impl<'a, I> Iterator for MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    type Item = (Event<'a>, Range<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // 1. Process buffer
            if let Some(event) = self.buffer.pop() {
                if let Some(result) = self.handle_candidate(event) {
                    return Some(result);
                } else {
                    // handle_candidate pushed to buffer and returned None
                    continue;
                }
            }

            // 2. Pull from inner
            let (event, range) = self.inner.next()?;

            // 3. Handle HTML: allowlist <u>…</u> as underline; strip everything else.
            // Spec §6.2 / §6.3: <br> and all other raw HTML produce no output.
            let (event, range) = match event {
                Event::InlineHtml(ref html) | Event::Html(ref html) if is_u_open_tag(html) => {
                    (Event::Start(Tag::Strong), range)
                }
                Event::InlineHtml(ref html) | Event::Html(ref html) if is_u_close_tag(html) => {
                    (Event::End(TagEnd::Strong), range)
                }
                Event::Html(_) | Event::InlineHtml(_) => continue,
                other => (other, range),
            };

            if let Some(result) = self.handle_candidate((event, range)) {
                return Some(result);
            } else {
                continue;
            }
        }
    }
}
pub fn mark_to_typst(markdown: &str) -> Result<String, ConversionError> {
    let mut options = pulldown_cmark::Options::empty();
    options.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    options.insert(pulldown_cmark::Options::ENABLE_TABLES);

    let parser = Parser::new_ext(markdown, options);
    let fixer = MarkdownFixer::new(parser.into_offset_iter(), markdown);
    let mut typst_output = String::new();

    push_typst(&mut typst_output, markdown, fixer)?;

    Ok(typst_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for escape_markup function
    #[test]
    fn test_escape_markup_basic() {
        assert_eq!(escape_markup("plain text"), "plain text");
    }

    #[test]
    fn test_escape_markup_backslash() {
        // Backslash must be escaped first to prevent double-escaping
        assert_eq!(escape_markup("\\"), "\\\\");
        assert_eq!(escape_markup("C:\\Users\\file"), "C:\\\\Users\\\\file");
    }

    #[test]
    fn test_escape_markup_formatting_chars() {
        assert_eq!(escape_markup("*bold*"), "\\*bold\\*");
        assert_eq!(escape_markup("_italic_"), "\\_italic\\_");
        assert_eq!(escape_markup("`code`"), "\\`code\\`");
    }

    #[test]
    fn test_escape_markup_typst_special_chars() {
        assert_eq!(escape_markup("#function"), "\\#function");
        assert_eq!(escape_markup("[link]"), "\\[link\\]");
        assert_eq!(escape_markup("$math$"), "\\$math\\$");
        assert_eq!(escape_markup("<tag>"), "\\<tag\\>");
        assert_eq!(escape_markup("@ref"), "\\@ref");
    }

    #[test]
    fn test_escape_markup_combined() {
        assert_eq!(
            escape_markup("Use * for bold and # for functions"),
            "Use \\* for bold and \\# for functions"
        );
    }

    #[test]
    fn test_escape_markup_tilde() {
        // Tilde is non-breaking space in Typst - must be escaped to prevent layout manipulation
        assert_eq!(escape_markup("Hello~World"), "Hello\\~World");
        assert_eq!(escape_markup("a~b~c"), "a\\~b\\~c");
    }

    // Tests for escape_string function
    #[test]
    fn test_escape_string_basic() {
        assert_eq!(escape_string("plain text"), "plain text");
    }

    #[test]
    fn test_escape_string_quotes_and_backslash() {
        assert_eq!(escape_string("\"quoted\""), "\\\"quoted\\\"");
        assert_eq!(escape_string("\\"), "\\\\");
    }

    #[test]
    fn test_escape_markup_double_curly_brackets() {
        assert_eq!(escape_markup("{{"), "\\{\\{");
        assert_eq!(escape_markup("}}"), "\\}\\}");
    }

    #[test]
    fn test_mark_to_typst_double_curly_brackets() {
        let output = mark_to_typst("Text {{ content }}").unwrap();
        assert_eq!(output, "Text \\{\\{ content \\}\\}\n\n");
    }

    #[test]
    fn test_escape_string_control_chars() {
        // ASCII control character (e.g., NUL)
        assert_eq!(escape_string("\x00"), "\\u{0}");
        assert_eq!(escape_string("\x01"), "\\u{1}");
    }

    // Tests for mark_to_typst - Basic Text Formatting
    #[test]
    fn test_basic_text_formatting() {
        let markdown = "This is **bold**, _italic_, and ~~strikethrough~~ text.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(
            typst,
            "This is #strong[bold], #emph[italic], and #strike[strikethrough] text.\n\n"
        );
    }

    #[test]
    fn test_bold_formatting() {
        assert_eq!(mark_to_typst("**bold**").unwrap(), "#strong[bold]\n\n");
        assert_eq!(
            mark_to_typst("This is **bold** text").unwrap(),
            "This is #strong[bold] text\n\n"
        );
    }

    #[test]
    fn test_italic_formatting() {
        assert_eq!(mark_to_typst("_italic_").unwrap(), "#emph[italic]\n\n");
        assert_eq!(mark_to_typst("*italic*").unwrap(), "#emph[italic]\n\n");
    }

    #[test]
    fn test_strikethrough_formatting() {
        assert_eq!(mark_to_typst("~~strike~~").unwrap(), "#strike[strike]\n\n");
    }

    #[test]
    fn test_inline_code() {
        assert_eq!(mark_to_typst("`code`").unwrap(), "`code`\n\n");
        assert_eq!(
            mark_to_typst("Text with `inline code` here").unwrap(),
            "Text with `inline code` here\n\n"
        );
    }

    // Tests for Lists
    #[test]
    fn test_unordered_list() {
        let markdown = "- Item 1\n- Item 2\n- Item 3";
        let typst = mark_to_typst(markdown).unwrap();
        // Lists end with extra newline per CONVERT.md examples
        assert_eq!(typst, "- Item 1\n- Item 2\n- Item 3\n\n");
    }

    #[test]
    fn test_ordered_list() {
        let markdown = "1. First\n2. Second\n3. Third";
        let typst = mark_to_typst(markdown).unwrap();
        // Typst auto-numbers, so we always use 1.
        // Lists end with extra newline per CONVERT.md examples
        assert_eq!(typst, "+ First\n+ Second\n+ Third\n\n");
    }

    #[test]
    fn test_nested_list() {
        let markdown = "- Item 1\n- Item 2\n  - Nested item\n- Item 3";
        let typst = mark_to_typst(markdown).unwrap();
        // Lists end with extra newline per CONVERT.md examples
        assert_eq!(typst, "- Item 1\n- Item 2\n  - Nested item\n- Item 3\n\n");
    }

    #[test]
    fn test_deeply_nested_list() {
        let markdown = "- Level 1\n  - Level 2\n    - Level 3";
        let typst = mark_to_typst(markdown).unwrap();
        // Lists end with extra newline per CONVERT.md examples
        assert_eq!(typst, "- Level 1\n  - Level 2\n    - Level 3\n\n");
    }

    // Tests for Links
    #[test]
    fn test_link() {
        let markdown = "[Link text](https://example.com)";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#link(\"https://example.com\")[Link text]\n\n");
    }

    #[test]
    fn test_link_in_sentence() {
        let markdown = "Visit [our site](https://example.com) for more.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(
            typst,
            "Visit #link(\"https://example.com\")[our site] for more.\n\n"
        );
    }

    // Tests for Mixed Content
    #[test]
    fn test_mixed_content() {
        let markdown = "A paragraph with **bold** and a [link](https://example.com).\n\nAnother paragraph with `inline code`.\n\n- A list item\n- Another item";
        let typst = mark_to_typst(markdown).unwrap();
        // Lists end with extra newline per CONVERT.md examples
        assert_eq!(
            typst,
            "A paragraph with #strong[bold] and a #link(\"https://example.com\")[link].\n\nAnother paragraph with `inline code`.\n\n- A list item\n- Another item\n\n"
        );
    }

    // Tests for Paragraphs
    #[test]
    fn test_single_paragraph() {
        let markdown = "This is a paragraph.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "This is a paragraph.\n\n");
    }

    #[test]
    fn test_multiple_paragraphs() {
        let markdown = "First paragraph.\n\nSecond paragraph.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "First paragraph.\n\nSecond paragraph.\n\n");
    }

    #[test]
    fn test_hard_break() {
        let markdown = "Line one  \nLine two";
        let typst = mark_to_typst(markdown).unwrap();
        // Hard break (two spaces) becomes Typst hard line break
        assert_eq!(typst, "Line one#linebreak()Line two\n\n");
    }

    #[test]
    fn test_backslash_hard_break() {
        let markdown = "Line one\\\nLine two";
        let typst = mark_to_typst(markdown).unwrap();
        // Backslash hard break becomes Typst hard line break
        assert_eq!(typst, "Line one#linebreak()Line two\n\n");
    }

    #[test]
    fn test_soft_break() {
        let markdown = "Line one\nLine two";
        let typst = mark_to_typst(markdown).unwrap();
        // Soft break (single newline) becomes space
        assert_eq!(typst, "Line one Line two\n\n");
    }

    #[test]
    fn test_soft_break_multiple_lines() {
        let markdown = "This is some\ntext on multiple\nlines";
        let typst = mark_to_typst(markdown).unwrap();
        // Soft breaks should join with spaces
        assert_eq!(typst, "This is some text on multiple lines\n\n");
    }

    // Tests for Character Escaping
    #[test]
    fn test_escaping_special_characters() {
        let markdown = "Typst uses * for bold and # for functions.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "Typst uses \\* for bold and \\# for functions.\n\n");
    }

    #[test]
    fn test_escaping_in_text() {
        let markdown = "Use [brackets] and $math$ symbols.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "Use \\[brackets\\] and \\$math\\$ symbols.\n\n");
    }

    // Edge Cases
    #[test]
    fn test_empty_string() {
        assert_eq!(mark_to_typst("").unwrap(), "");
    }

    #[test]
    fn test_only_whitespace() {
        let markdown = "   ";
        let typst = mark_to_typst(markdown).unwrap();
        // Whitespace-only input produces empty output (no paragraph tags for empty content)
        assert_eq!(typst, "");
    }

    #[test]
    fn test_consecutive_formatting() {
        let markdown = "**bold** _italic_ ~~strike~~";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#strong[bold] #emph[italic] #strike[strike]\n\n");
    }

    #[test]
    fn test_nested_formatting() {
        let markdown = "**bold _and italic_**";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#strong[bold #emph[and italic]]\n\n");
    }

    #[test]
    fn test_list_with_formatting() {
        let markdown = "- **Bold** item\n- _Italic_ item\n- `Code` item";
        let typst = mark_to_typst(markdown).unwrap();
        // Lists end with extra newline
        assert_eq!(
            typst,
            "- #strong[Bold] item\n- #emph[Italic] item\n- `Code` item\n\n"
        );
    }

    #[test]
    fn test_mixed_list_types() {
        let markdown = "- Bullet item\n\n1. Ordered item\n2. Another ordered";
        let typst = mark_to_typst(markdown).unwrap();
        // Each list ends with extra newline
        assert_eq!(
            typst,
            "- Bullet item\n\n+ Ordered item\n+ Another ordered\n\n"
        );
    }

    #[test]
    fn test_list_item_paragraph_separation() {
        // Two paragraphs in a list item should produce continuation with blank line + indent
        let markdown = "- First line.\n\n  Second line.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "- First line.\n\n  Second line.\n\n");
    }

    #[test]
    fn test_list_item_three_paragraphs() {
        let markdown = "- Para 1.\n\n  Para 2.\n\n  Para 3.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "- Para 1.\n\n  Para 2.\n\n  Para 3.\n\n");
    }

    #[test]
    fn test_list_item_multiple_items_with_continuation() {
        let markdown = "- Item 1\n\n  More text.\n\n- Item 2";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "- Item 1\n\n  More text.\n- Item 2\n\n");
    }

    #[test]
    fn test_ordered_list_multi_para() {
        let markdown = "1. First para.\n\n   Second para.\n\n2. Next item.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "+ First para.\n\n  Second para.\n+ Next item.\n\n");
    }

    #[test]
    fn test_code_block_standalone() {
        let markdown = "```rust\nfn main() {}\n```";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "```rust\nfn main() {}\n```\n\n");
    }

    #[test]
    fn test_code_block_no_lang() {
        let markdown = "```\nhello\n```";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "```\nhello\n```\n\n");
    }

    #[test]
    fn test_code_block_no_escaping() {
        // Special chars in code blocks should NOT be escaped
        let markdown = "```\n*bold* #heading $math$\n```";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "```\n*bold* #heading $math$\n```\n\n");
    }

    #[test]
    fn test_code_block_in_list_item() {
        let markdown = "- Item text\n\n  ```\n  code here\n  ```";
        let typst = mark_to_typst(markdown).unwrap();
        // pulldown_cmark strips indentation from code block content
        assert_eq!(typst, "- Item text\n\n  ```\ncode here\n  ```\n\n");
    }

    #[test]
    fn test_code_block_between_paragraphs_in_list() {
        let markdown = "- First para.\n\n  ```\n  code\n  ```\n\n  After code.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(
            typst,
            "- First para.\n\n  ```\ncode\n  ```\n\n  After code.\n\n"
        );
    }

    #[test]
    fn test_link_with_special_chars_in_url() {
        let markdown = "[Link](https://example.com/foo_bar)";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#link(\"https://example.com/foo_bar\")[Link]\n\n");
    }

    #[test]
    fn test_link_with_anchor() {
        // URLs don't need # escaped in Typst string literals
        let markdown = "[Link](#anchor)";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#link(\"#anchor\")[Link]\n\n");
    }

    #[test]
    fn test_markdown_escapes() {
        // Backslash escapes in markdown should work
        let markdown = "Use \\* for lists";
        let typst = mark_to_typst(markdown).unwrap();
        // The parser removes the backslash, then we escape for Typst
        assert_eq!(typst, "Use \\* for lists\n\n");
    }

    #[test]
    fn test_double_backslash() {
        let markdown = "Path: C:\\\\Users\\\\file";
        let typst = mark_to_typst(markdown).unwrap();
        // Double backslash in markdown becomes single in parser, then doubled for Typst
        assert_eq!(typst, "Path: C:\\\\Users\\\\file\n\n");
    }

    // Tests for resource limits
    #[test]
    fn test_nesting_depth_limit() {
        // Create deeply nested blockquotes (each ">" adds one nesting level)
        let mut markdown = String::new();
        for _ in 0..=MAX_NESTING_DEPTH {
            markdown.push('>');
            markdown.push(' ');
        }
        markdown.push_str("text");

        // This should exceed the limit and return an error
        let result = mark_to_typst(&markdown);
        assert!(result.is_err());

        if let Err(ConversionError::NestingTooDeep { depth, max }) = result {
            assert!(depth > max);
            assert_eq!(max, MAX_NESTING_DEPTH);
        } else {
            panic!("Expected NestingTooDeep error");
        }
    }

    #[test]
    fn test_nesting_depth_within_limit() {
        // Create nested structure just within the limit
        let mut markdown = String::new();
        for _ in 0..50 {
            markdown.push('>');
            markdown.push(' ');
        }
        markdown.push_str("text");

        // This should succeed
        let result = mark_to_typst(&markdown);
        assert!(result.is_ok());
    }

    // Tests for // (comment syntax) escaping
    #[test]
    fn test_slash_comment_in_url() {
        let markdown = "Check out https://example.com for more.";
        let typst = mark_to_typst(markdown).unwrap();
        // The // in https:// should be escaped to prevent it from being treated as a comment
        assert!(typst.contains("https:\\/\\/example.com"));
    }

    #[test]
    fn test_slash_comment_at_line_start() {
        let markdown = "// This should not be a comment";
        let typst = mark_to_typst(markdown).unwrap();
        // // at the start of a line should be escaped
        assert!(typst.contains("\\/\\/"));
    }

    #[test]
    fn test_slash_comment_in_middle() {
        let markdown = "Some text // with slashes in the middle";
        let typst = mark_to_typst(markdown).unwrap();
        // // in the middle of text should be escaped
        assert!(typst.contains("text \\/\\/"));
    }

    #[test]
    fn test_file_protocol() {
        let markdown = "Use file://path/to/file protocol";
        let typst = mark_to_typst(markdown).unwrap();
        // file:// should be escaped
        assert!(typst.contains("file:\\/\\/"));
    }

    #[test]
    fn test_single_slash() {
        let markdown = "Use path/to/file for the file";
        let typst = mark_to_typst(markdown).unwrap();
        // Single slashes should not be escaped (only // is a comment in Typst)
        assert!(typst.contains("path/to/file"));
    }

    #[test]
    fn test_italic_followed_by_alphanumeric() {
        // Function syntax (#emph[]) handles word boundaries naturally
        let markdown = "*Write y*our paragraphs here.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#emph[Write y]our paragraphs here.\n\n");
    }

    #[test]
    fn test_italic_followed_by_space() {
        let markdown = "*italic* text";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#emph[italic] text\n\n");
    }

    #[test]
    fn test_italic_followed_by_punctuation() {
        let markdown = "*italic*.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#emph[italic].\n\n");
    }

    #[test]
    fn test_bold_followed_by_alphanumeric() {
        // Function syntax (#strong[]) handles word boundaries naturally
        let markdown = "**bold**text";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "#strong[bold]text\n\n");
    }

    // Tests for Headings
    #[test]
    fn test_heading_level_1() {
        let markdown = "# Heading 1";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "= Heading 1\n\n");
    }

    #[test]
    fn test_heading_level_2() {
        let markdown = "## Heading 2";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "== Heading 2\n\n");
    }

    #[test]
    fn test_heading_level_3() {
        let markdown = "### Heading 3";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "=== Heading 3\n\n");
    }

    #[test]
    fn test_heading_level_4() {
        let markdown = "#### Heading 4";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "==== Heading 4\n\n");
    }

    #[test]
    fn test_heading_level_5() {
        let markdown = "##### Heading 5";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "===== Heading 5\n\n");
    }

    #[test]
    fn test_heading_level_6() {
        let markdown = "###### Heading 6";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "====== Heading 6\n\n");
    }

    #[test]
    fn test_heading_with_formatting() {
        let markdown = "## Heading with **bold** and _italic_";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "== Heading with #strong[bold] and #emph[italic]\n\n");
    }

    #[test]
    fn test_multiple_headings() {
        let markdown = "# First\n\n## Second\n\n### Third";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "= First\n\n== Second\n\n=== Third\n\n");
    }

    #[test]
    fn test_heading_followed_by_paragraph() {
        let markdown = "# Heading\n\nThis is a paragraph.";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "= Heading\n\nThis is a paragraph.\n\n");
    }

    #[test]
    fn test_heading_with_special_chars() {
        let markdown = "# Heading with $math$ and #functions";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "= Heading with \\$math\\$ and \\#functions\n\n");
    }

    #[test]
    fn test_paragraph_then_heading() {
        let markdown = "A paragraph.\n\n# A Heading";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "A paragraph.\n\n= A Heading\n\n");
    }

    #[test]
    fn test_heading_with_inline_code() {
        let markdown = "## Code example: `fn main()`";
        let typst = mark_to_typst(markdown).unwrap();
        assert_eq!(typst, "== Code example: `fn main()`\n\n");
    }

    // Tests for __ as CommonMark strong (no longer underline)

    #[test]
    fn test_double_underscore_is_strong() {
        // Per CommonMark, __text__ renders as strong, identical to **text**.
        assert_eq!(mark_to_typst("__bolded__").unwrap(), "#strong[bolded]\n\n");
    }

    #[test]
    fn test_double_underscore_with_text() {
        assert_eq!(
            mark_to_typst("This is __bolded__ text").unwrap(),
            "This is #strong[bolded] text\n\n"
        );
    }

    #[test]
    fn test_bold_unchanged() {
        // Verify ** still works as bold
        assert_eq!(mark_to_typst("**bold**").unwrap(), "#strong[bold]\n\n");
    }

    #[test]
    fn test_double_underscore_in_list() {
        assert_eq!(
            mark_to_typst("- __bolded__ item").unwrap(),
            "- #strong[bolded] item\n\n"
        );
    }

    #[test]
    fn test_double_underscore_in_heading() {
        assert_eq!(
            mark_to_typst("# Heading with __bold__").unwrap(),
            "= Heading with #strong[bold]\n\n"
        );
    }

    #[test]
    fn test_quadruple_underscore_is_thematic_break() {
        // pulldown-cmark treats ____ as a thematic break / horizontal rule.
        // This test verifies we don't crash on this input.
        let result = mark_to_typst("____").unwrap();
        // The result is empty because Rule events are ignored in our converter
        assert_eq!(result, "");
    }

    // Tests for Tables

    #[test]
    fn test_basic_table() {
        let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
        let out = mark_to_typst(md).unwrap();
        assert_eq!(
            out,
            "#table(\n  columns: 2,\n  table.header([Name], [Age], ),\n  [Alice], [30], \n  [Bob], [25], \n)\n\n"
        );
    }

    #[test]
    fn test_table_default_alignment() {
        // No alignment specified — no align: row emitted
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            !out.contains("align:"),
            "should not emit align when all default"
        );
        assert!(out.contains("#table(\n  columns: 2,\n"));
    }

    #[test]
    fn test_table_with_alignment() {
        let md = "| L | C | R |\n|:---|:---:|---:|\n| a | b | c |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("  align: (left, center, right),\n"));
        assert!(out.contains("#table(\n  columns: 3,\n"));
    }

    #[test]
    fn test_table_header_only() {
        let md = "| Name | Value |\n|------|-------|\n";
        let out = mark_to_typst(md).unwrap();
        assert!(out.starts_with("#table(\n  columns: 2,\n"));
        assert!(out.contains("table.header([Name], [Value], )"));
        assert!(out.ends_with(")\n\n"));
    }

    #[test]
    fn test_table_single_column() {
        let md = "| Item |\n|------|\n| A |\n| B |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.starts_with("#table(\n  columns: 1,\n"));
        assert!(out.contains("table.header([Item], )"));
        assert!(out.contains("[A], \n"));
        assert!(out.contains("[B], \n"));
    }

    #[test]
    fn test_table_empty_cell() {
        let md = "| A | B |\n|---|---|\n| | x |";
        let out = mark_to_typst(md).unwrap();
        // Empty cell becomes []
        assert!(out.contains("[], [x], "));
    }

    #[test]
    fn test_table_with_formatting_in_cells() {
        let md = "| Name | Note |\n|------|------|\n| **bold** | _italic_ |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("[#strong[bold]]"));
        assert!(out.contains("[#emph[italic]]"));
    }

    #[test]
    fn test_table_with_inline_code_in_cells() {
        let md = "| Func | Desc |\n|------|------|\n| `foo()` | does stuff |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("[`foo()`]"));
    }

    #[test]
    fn test_table_with_link_in_cell() {
        let md = "| Site |\n|------|\n| [Example](https://example.com) |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("[#link(\"https://example.com\")[Example]]"));
    }

    #[test]
    fn test_table_special_chars_in_cells() {
        let md = "| Col |\n|-----|\n| use #tag |";
        let out = mark_to_typst(md).unwrap();
        // # must be escaped in cell text
        assert!(out.contains("[use \\#tag]"));
    }

    #[test]
    fn test_table_in_document_with_paragraphs() {
        let md = "Before.\n\n| A |\n|---|\n| 1 |\n\nAfter.";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("Before.\n\n"));
        assert!(out.contains("#table("));
        assert!(out.contains("After.\n\n"));
    }

    // Edge case tests for table conversion robustness

    #[test]
    fn test_table_pipe_in_cell() {
        // Escaped pipe character should appear in cell content
        let md = "| A |\n|---|\n| a\\|b |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("[a|b]"),
            "pipe should be literal in cell: {out}"
        );
    }

    #[test]
    fn test_table_strikethrough_in_cell() {
        let md = "| A |\n|---|\n| ~~deleted~~ |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("[#strike[deleted]]"),
            "strikethrough should work in cells: {out}"
        );
    }

    #[test]
    fn test_table_multiple_consecutive() {
        let md = "| A |\n|---|\n| 1 |\n\n| B |\n|---|\n| 2 |";
        let out = mark_to_typst(md).unwrap();
        assert_eq!(
            out.matches("#table(").count(),
            2,
            "should have two tables: {out}"
        );
        assert!(out.contains("table.header([A]"));
        assert!(out.contains("table.header([B]"));
    }

    #[test]
    fn test_table_mixed_alignment() {
        // Some columns aligned, some default
        let md = "| L | D | R |\n|:---|---|---:|\n| a | b | c |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("align: (left, auto, right),"),
            "mixed alignment: {out}"
        );
    }

    #[test]
    fn test_table_all_alignment_types() {
        let md = "| L | C | R | D |\n|:---|:---:|---:|---|\n| a | b | c | d |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("align: (left, center, right, auto),"),
            "all alignments: {out}"
        );
    }

    #[test]
    fn test_table_wide() {
        let md = "| A | B | C | D | E | F |\n|---|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 | 6 |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("columns: 6,"), "wide table columns: {out}");
        assert!(out.contains("[1], [2], [3], [4], [5], [6],"));
    }

    #[test]
    fn test_table_nested_bold_italic() {
        let md = "| A |\n|---|\n| **bold** and _italic_ |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("#strong[bold]"), "bold in cell: {out}");
        assert!(out.contains("#emph[italic]"), "italic in cell: {out}");
    }

    #[test]
    fn test_table_typst_special_chars() {
        // All Typst-special characters that need escaping
        let md = "| A |\n|---|\n| $100 @ref ~space {brace} |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("\\$100"), "dollar escaped: {out}");
        assert!(out.contains("\\@ref"), "at escaped: {out}");
        assert!(out.contains("\\~space"), "tilde escaped: {out}");
        assert!(out.contains("\\{brace\\}"), "braces escaped: {out}");
    }

    #[test]
    fn test_table_angle_brackets_in_cell() {
        let md = "| A |\n|---|\n| a < b > c |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("\\<"), "< escaped: {out}");
        assert!(out.contains("\\>"), "> escaped: {out}");
    }

    #[test]
    fn test_table_double_slash_in_cell() {
        let md = "| A |\n|---|\n| a // comment |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("\\/\\/"), "double slash escaped: {out}");
    }

    #[test]
    fn test_table_square_brackets_in_cell() {
        let md = "| A |\n|---|\n| [item] |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("\\[item\\]"), "brackets escaped: {out}");
    }

    #[test]
    fn test_table_curly_braces_in_cell() {
        let md = "| A |\n|---|\n| {value} |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("\\{value\\}"), "braces escaped: {out}");
    }

    #[test]
    fn test_table_br_tag_in_cell_is_stripped() {
        // Per MARKDOWN.md §6.2 / §6.3, raw HTML (including <br>) produces no output.
        let md = "| A |\n|---|\n| line1<br>line2 |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            !out.contains("linebreak"),
            "<br> must not produce #linebreak(): {out}"
        );
        assert!(!out.contains("<br"), "<br> literal must be stripped: {out}");
        assert!(out.contains("line1"), "surrounding text preserved: {out}");
        assert!(out.contains("line2"), "surrounding text preserved: {out}");
    }

    #[test]
    fn test_table_br_tag_variants_stripped() {
        // <br/> and <br /> variants must also produce no output.
        for md in ["| A |\n|---|\n| a<br/>b |", "| A |\n|---|\n| a<br />b |"] {
            let out = mark_to_typst(md).unwrap();
            assert!(
                !out.contains("linebreak"),
                "<br> variants must not emit #linebreak(): {out}"
            );
            assert!(out.contains('a') && out.contains('b'), "text kept: {out}");
        }
    }

    #[test]
    fn test_u_tag_renders_as_underline() {
        // Spec §6.2: <u>…</u> is the one allowlisted HTML tag; renders as underline.
        let md = "This is <u>underlined</u> text.";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("#underline[underlined]"),
            "<u> must render as #underline[…]: {out}"
        );
    }

    #[test]
    fn test_u_tag_intraword_renders_as_underline() {
        // <u> exists specifically to cover arbitrary-range underline that __ cannot reach.
        let md = "pre<u>mid</u>post";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("#underline[mid]"),
            "intraword <u> must render as #underline[…]: {out}"
        );
        assert!(out.contains("pre"), "prefix preserved: {out}");
        assert!(out.contains("post"), "suffix preserved: {out}");
    }

    #[test]
    fn test_u_tag_case_insensitive() {
        // Accept <U>…</U> as well as mixed case.
        let md = "<U>upper</U>";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("#underline[upper]"),
            "<U> must render as #underline[…]: {out}"
        );
    }

    #[test]
    fn test_raw_html_is_stripped() {
        // Spec §6.2: all raw HTML except <u> produces no output.
        let md = "before <span class=\"x\">inner</span> after";
        let out = mark_to_typst(md).unwrap();
        assert!(!out.contains("<span"), "span tag stripped: {out}");
        assert!(!out.contains("</span>"), "span close tag stripped: {out}");
        assert!(out.contains("before"), "text preserved: {out}");
        assert!(out.contains("after"), "text preserved: {out}");
        assert!(out.contains("inner"), "inner text preserved: {out}");
    }

    #[test]
    fn test_image_renders_as_image() {
        // Spec §6.3: images are required for v1; emit #image("url").
        let md = "![alt text](path/to/img.png)";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("#image(\"path/to/img.png\")"),
            "image must emit #image(\"…\"): {out}"
        );
        assert!(!out.contains("alt text"), "alt text suppressed: {out}");
    }

    #[test]
    fn test_image_with_empty_alt() {
        let md = "![](x.png)";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("#image(\"x.png\")"),
            "empty-alt image emits #image: {out}"
        );
    }

    #[test]
    fn test_table_unicode_in_cell() {
        let md = "| Status |\n|--------|\n| ✅ Done |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("✅ Done"), "unicode in cell: {out}");
    }

    #[test]
    fn test_table_emoji_in_cell() {
        let md = "| A |\n|---|\n| 🎉 Party 🚀 |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("🎉 Party 🚀"), "emoji in cell: {out}");
    }

    #[test]
    fn test_table_after_heading() {
        let md = "# Title\n\n| A |\n|---|\n| 1 |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("= Title\n\n"), "heading before table: {out}");
        assert!(out.contains("#table("), "table after heading: {out}");
    }

    #[test]
    fn test_table_after_list() {
        let md = "- item1\n- item2\n\n| A |\n|---|\n| 1 |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("- item1"), "list before table: {out}");
        assert!(out.contains("#table("), "table after list: {out}");
    }

    #[test]
    fn test_table_many_rows() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |\n| 7 | 8 |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("[1], [2],"), "row 1: {out}");
        assert!(out.contains("[3], [4],"), "row 2: {out}");
        assert!(out.contains("[5], [6],"), "row 3: {out}");
        assert!(out.contains("[7], [8],"), "row 4: {out}");
    }

    #[test]
    fn test_table_bold_link_in_cell() {
        let md = "| A |\n|---|\n| **[link](https://x.com)** |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("#strong[#link("), "bold link in cell: {out}");
    }

    #[test]
    fn test_table_code_with_special_chars() {
        // Characters inside inline code should NOT be escaped
        let md = "| A |\n|---|\n| `a#b$c@d` |";
        let out = mark_to_typst(md).unwrap();
        assert!(
            out.contains("`a#b$c@d`"),
            "code content should be literal: {out}"
        );
    }

    #[test]
    fn test_table_empty_minimal() {
        // Single empty header cell, no body rows
        let md = "| |\n|---|";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("#table("), "should create table: {out}");
        assert!(out.contains("columns: 1,"), "single column: {out}");
    }

    #[test]
    fn test_table_multiple_empty_cells() {
        let md = "| A | B | C |\n|---|---|---|\n| | | |";
        let out = mark_to_typst(md).unwrap();
        assert!(out.contains("[], [], [],"), "multiple empty cells: {out}");
    }
}

// Additional robustness tests
#[cfg(test)]
mod robustness_tests {
    use super::*;

    // Empty and edge case inputs

    #[test]
    fn test_only_newlines() {
        let result = mark_to_typst("\n\n\n").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_only_spaces_and_newlines() {
        let result = mark_to_typst("   \n   \n   ").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_single_character() {
        assert_eq!(mark_to_typst("a").unwrap(), "a\n\n");
    }

    #[test]
    fn test_single_special_character() {
        // Note: Single * at line start is parsed as a list marker by pulldown-cmark
        // Single # at line start is parsed as a heading marker
        // So we test with characters in context where they're literal
        assert_eq!(mark_to_typst("a # b").unwrap(), "a \\# b\n\n");
        assert_eq!(mark_to_typst("$").unwrap(), "\\$\n\n");
        // Asterisk in middle of text is escaped
        assert_eq!(mark_to_typst("a * b").unwrap(), "a \\* b\n\n");
    }

    // Unicode handling

    #[test]
    fn test_unicode_text() {
        let result = mark_to_typst("你好世界").unwrap();
        assert_eq!(result, "你好世界\n\n");
    }

    #[test]
    fn test_unicode_with_formatting() {
        let result = mark_to_typst("**你好** _世界_").unwrap();
        assert_eq!(result, "#strong[你好] #emph[世界]\n\n");
    }

    #[test]
    fn test_emoji() {
        let result = mark_to_typst("Hello 🎉 World 🚀").unwrap();
        assert_eq!(result, "Hello 🎉 World 🚀\n\n");
    }

    #[test]
    fn test_emoji_in_link() {
        let result = mark_to_typst("[Click 🎉](https://example.com)").unwrap();
        assert_eq!(result, "#link(\"https://example.com\")[Click 🎉]\n\n");
    }

    #[test]
    fn test_rtl_text() {
        // Arabic text
        let result = mark_to_typst("مرحبا بالعالم").unwrap();
        assert_eq!(result, "مرحبا بالعالم\n\n");
    }

    // Escape edge cases

    #[test]
    fn test_multiple_consecutive_slashes() {
        let result = mark_to_typst("a///b").unwrap();
        // /// should become \/\// (first // escaped, third / stays)
        assert!(result.contains("\\/\\/"));
    }

    #[test]
    fn test_escape_at_string_boundaries() {
        // Test escaping at start of string
        assert!(mark_to_typst("*start").unwrap().starts_with("\\*"));
        // Test escaping at end of string
        assert!(mark_to_typst("end*").unwrap().contains("end\\*"));
    }

    #[test]
    fn test_backslash_before_special_char() {
        // Backslash followed by special char - both should be escaped
        let result = mark_to_typst("\\*").unwrap();
        // In markdown, \* is an escaped asterisk, becomes literal *
        // Then we escape it for Typst
        assert!(result.contains("\\*"));
    }

    #[test]
    fn test_all_special_chars_together() {
        let result = mark_to_typst("*_`#[]$<>@\\").unwrap();
        assert!(result.contains("\\*"));
        assert!(result.contains("\\_"));
        assert!(result.contains("\\`"));
        assert!(result.contains("\\#"));
        assert!(result.contains("\\["));
        assert!(result.contains("\\]"));
        assert!(result.contains("\\$"));
        assert!(result.contains("\\<"));
        assert!(result.contains("\\>"));
        assert!(result.contains("\\@"));
        assert!(result.contains("\\\\"));
    }

    // Link edge cases

    #[test]
    fn test_link_with_quotes_in_url() {
        let result = mark_to_typst("[link](https://example.com?q=\"test\")").unwrap();
        assert!(result.contains("\\\"test\\\""));
    }

    #[test]
    fn test_link_with_backslash_in_url() {
        let result = mark_to_typst("[link](https://example.com\\path)").unwrap();
        assert!(result.contains("\\\\"));
    }

    #[test]
    fn test_link_with_newline_in_text() {
        // Markdown link text can span lines with soft breaks
        let result = mark_to_typst("[multi\nline](https://example.com)").unwrap();
        // Soft break becomes space in link text
        assert!(result.contains("[multi line]"));
    }

    #[test]
    fn test_empty_link_text() {
        let result = mark_to_typst("[](https://example.com)").unwrap();
        assert_eq!(result, "#link(\"https://example.com\")[]\n\n");
    }

    #[test]
    fn test_link_with_special_chars_in_text() {
        let result = mark_to_typst("[*bold* link](https://example.com)").unwrap();
        assert!(result.contains("#emph[bold]"));
    }

    // List edge cases

    #[test]
    fn test_empty_list_item() {
        let result = mark_to_typst("- \n- item").unwrap();
        // Empty list items are valid
        assert!(result.contains("- "));
    }

    #[test]
    fn test_list_with_multiple_paragraphs() {
        let markdown = "- First para\n\n  Second para in same item";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "- First para\n\n  Second para in same item\n\n");
    }

    #[test]
    fn test_very_deeply_nested_list() {
        // Create a list nested 10 levels deep (within limit)
        let mut markdown = String::new();
        for i in 0..10 {
            markdown.push_str(&"  ".repeat(i));
            markdown.push_str("- item\n");
        }
        let result = mark_to_typst(&markdown);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_ordered_unordered_nested() {
        let markdown = "1. First\n   - Nested bullet\n   - Another bullet\n2. Second";
        let result = mark_to_typst(markdown).unwrap();
        assert!(result.contains("+ First"));
        assert!(result.contains("- Nested bullet"));
        assert!(result.contains("+ Second"));
    }

    // Heading edge cases

    #[test]
    fn test_heading_with_only_special_chars() {
        let result = mark_to_typst("# ***").unwrap();
        assert!(result.contains("= "));
    }

    #[test]
    fn test_heading_followed_by_list() {
        let result = mark_to_typst("# Heading\n\n- Item").unwrap();
        assert!(result.contains("= Heading\n\n"));
        assert!(result.contains("- Item"));
    }

    #[test]
    fn test_consecutive_headings() {
        let result = mark_to_typst("# One\n## Two\n### Three").unwrap();
        assert!(result.contains("= One"));
        assert!(result.contains("== Two"));
        assert!(result.contains("=== Three"));
    }

    #[test]
    fn test_atx_headings_still_work() {
        // ATX headings should still be converted properly
        let result = mark_to_typst("# H1\n## H2\n### H3").unwrap();
        assert!(result.contains("= H1"));
        assert!(result.contains("== H2"));
        assert!(result.contains("=== H3"));
    }

    // Code block handling

    #[test]
    fn test_fenced_code_block() {
        let markdown = "```rust\nfn main() {}\n```";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "```rust\nfn main() {}\n```\n\n");
    }

    #[test]
    fn test_indented_code_block() {
        let markdown = "    fn main() {}\n    println!()";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "```\nfn main() {}\nprintln!()\n```\n\n");
    }

    // Inline code edge cases

    #[test]
    fn test_inline_code_with_backticks() {
        // Using double backticks to include single backtick
        let result = mark_to_typst("`` `code` ``").unwrap();
        assert!(result.contains("`"));
    }

    #[test]
    fn test_inline_code_with_special_chars() {
        // Special chars in code should NOT be escaped
        let result = mark_to_typst("`*#$<>`").unwrap();
        assert_eq!(result, "`*#$<>`\n\n");
    }

    #[test]
    fn test_empty_inline_code() {
        // pulldown-cmark doesn't parse `` as empty inline code
        // It needs content or different backtick counts
        let result = mark_to_typst("` `").unwrap();
        assert!(result.contains("`")); // space-only code span
    }

    // Formatting edge cases

    #[test]
    fn test_adjacent_emphasis() {
        let result = mark_to_typst("*a**b*").unwrap();
        // Depends on how markdown parser handles this
        assert!(result.contains("#emph["));
    }

    #[test]
    fn test_emphasis_across_words() {
        let result = mark_to_typst("*multiple words here*").unwrap();
        assert_eq!(result, "#emph[multiple words here]\n\n");
    }

    #[test]
    fn test_strong_across_lines() {
        let result = mark_to_typst("**bold\nacross\nlines**").unwrap();
        // Soft breaks become spaces
        assert!(result.contains("bold across lines"));
    }

    #[test]
    fn test_strikethrough_with_special_chars() {
        let result = mark_to_typst("~~*text*~~").unwrap();
        // Strikethrough content: emphasis should still work
        assert!(result.contains("#strike["));
    }

    // Strong stack edge cases

    #[test]
    fn test_multiple_nested_strong() {
        // Unusual but valid: nested strongs
        let result = mark_to_typst("**a **b** a**");
        assert!(result.is_ok());
    }

    #[test]
    fn test_alternating_bold_styles() {
        // Both ** and __ now produce #strong[…].
        let result = mark_to_typst("**a** __b__ **c**").unwrap();
        assert!(result.contains("#strong[a]"));
        assert!(result.contains("#strong[b]"));
        assert!(result.contains("#strong[c]"));
        assert!(!result.contains("#underline["));
    }

    // escape_string function tests

    #[test]
    fn test_escape_string_unicode() {
        // Unicode should pass through unchanged
        assert_eq!(escape_string("你好"), "你好");
        assert_eq!(escape_string("🎉"), "🎉");
    }

    #[test]
    fn test_escape_string_all_escapes() {
        assert_eq!(escape_string("\\\"\n\r\t"), "\\\\\\\"\\n\\r\\t");
    }

    #[test]
    fn test_escape_string_nul_character() {
        assert_eq!(escape_string("\x00"), "\\u{0}");
    }

    #[test]
    fn test_escape_string_bell_character() {
        assert_eq!(escape_string("\x07"), "\\u{7}");
    }

    #[test]
    fn test_escape_string_mixed() {
        assert_eq!(
            escape_string("Hello\nWorld\t\"quoted\""),
            "Hello\\nWorld\\t\\\"quoted\\\""
        );
    }

    // escape_markup function tests

    #[test]
    fn test_escape_markup_empty() {
        assert_eq!(escape_markup(""), "");
    }

    #[test]
    fn test_escape_markup_unicode() {
        // Unicode should pass through unchanged
        assert_eq!(escape_markup("你好世界"), "你好世界");
    }

    #[test]
    fn test_escape_markup_triple_slash() {
        // /// should escape the first // and leave the third /
        assert_eq!(escape_markup("///"), "\\/\\//");
    }

    #[test]
    fn test_escape_markup_url() {
        assert_eq!(
            escape_markup("https://example.com"),
            "https:\\/\\/example.com"
        );
    }

    // Paragraph handling

    #[test]
    fn test_many_paragraphs() {
        let markdown = "P1.\n\nP2.\n\nP3.\n\nP4.\n\nP5.";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result.matches("P").count(), 5);
        assert!(result.contains("P1.\n\nP2."));
    }

    #[test]
    fn test_paragraph_with_only_formatting() {
        let result = mark_to_typst("**bold only**").unwrap();
        assert_eq!(result, "#strong[bold only]\n\n");
    }

    // Soft break and hard break

    #[test]
    fn test_hard_break_in_list() {
        let result = mark_to_typst("- line one  \n  line two").unwrap();
        // Hard break in list item
        assert!(result.contains("line one"));
    }

    #[test]
    fn test_multiple_hard_breaks() {
        let result = mark_to_typst("a  \nb  \nc").unwrap();
        assert_eq!(result, "a#linebreak()b#linebreak()c\n\n");
    }

    // Word boundary handling (no longer needed with function syntax)

    #[test]
    fn test_italic_before_number() {
        let result = mark_to_typst("*italic*1").unwrap();
        // Function syntax handles word boundaries naturally
        assert!(result.contains("#emph[italic]1"));
    }

    #[test]
    fn test_bold_before_underscore() {
        // In **bold**_after, the _ is literal text (not starting emphasis)
        // So it gets escaped in Typst output
        let result = mark_to_typst("**bold**_after").unwrap();
        // Underscore is escaped as literal text
        assert!(result.contains("#strong[bold]\\_after"));
    }

    #[test]
    fn test_emphasis_at_end_of_text() {
        let result = mark_to_typst("*italic*").unwrap();
        assert_eq!(result, "#emph[italic]\n\n");
    }

    // Complex real-world scenarios

    #[test]
    fn test_markdown_document() {
        let markdown = r#"# Title

This is a paragraph with **bold** and *italic* text.

## Section

- List item 1
- List item 2 with [link](https://example.com)

More text with `inline code`."#;

        let result = mark_to_typst(markdown).unwrap();
        assert!(result.contains("= Title"));
        assert!(result.contains("== Section"));
        assert!(result.contains("#strong[bold]"));
        assert!(result.contains("#emph[italic]"));
        assert!(result.contains("- List item"));
        assert!(result.contains("#link"));
        assert!(result.contains("`inline code`"));
    }

    #[test]
    fn test_typst_syntax_in_content() {
        // Content that looks like Typst syntax should be escaped
        let markdown = "Use #set for settings and $x^2$ for math.";
        let result = mark_to_typst(markdown).unwrap();
        assert!(result.contains("\\#set"));
        assert!(result.contains("\\$x^2\\$"));
    }

    #[test]
    fn test_midword_italic() {
        // Function syntax handles mid-word emphasis naturally
        let markdown = "a*sdfasd*f";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "a#emph[sdfasd]f\n\n");
    }

    #[test]
    fn test_midword_bold() {
        // Function syntax handles mid-word bold naturally
        let markdown = "word**bold**more";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "word#strong[bold]more\n\n");
    }

    #[test]
    fn test_emphasis_preceded_by_alphanumeric() {
        // Function syntax handles this naturally
        let markdown = "text*emph*";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "text#emph[emph]\n\n");
    }

    #[test]
    fn test_emphasis_after_space() {
        let markdown = "some *italic* text";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "some #emph[italic] text\n\n");
    }

    #[test]
    fn test_emphasis_after_punctuation() {
        let markdown = "(*italic*)";
        let result = mark_to_typst(markdown).unwrap();
        assert_eq!(result, "(#emph[italic])\n\n");
    }

    // Tests for long underscore runs (fill-in-the-blank lines)
    #[test]
    fn test_long_underscore_run_as_literal_text() {
        // Long underscore runs should be treated as literal text, not underline markers
        let input = "I acknowledge receipt and understanding of this letter on ________________ at ___________ hours.";
        let result = mark_to_typst(input).unwrap();
        // All underscores should be escaped, no brackets
        assert!(
            !result.contains('['),
            "Should not contain opening brackets: {}",
            result
        );
        assert!(
            !result.contains(']'),
            "Should not contain closing brackets: {}",
            result
        );
        assert!(
            result.contains("\\_"),
            "Underscores should be escaped: {}",
            result
        );
    }

    #[test]
    fn test_triple_underscore_as_literal() {
        // Three consecutive underscores should be literal
        let input = "fill in: ___";
        let result = mark_to_typst(input).unwrap();
        assert!(
            !result.contains('['),
            "Should not contain brackets: {}",
            result
        );
    }

    #[test]
    fn test_four_underscores_as_literal() {
        // Four consecutive underscores should be literal
        let input = "fill in: ____";
        let result = mark_to_typst(input).unwrap();
        assert!(
            !result.contains('['),
            "Should not contain brackets: {}",
            result
        );
    }

    #[test]
    fn test_double_underscore_produces_strong() {
        // Per CommonMark, __text__ produces strong, not underline.
        let input = "__bolded text__";
        let result = mark_to_typst(input).unwrap();
        assert!(
            result.contains("#strong["),
            "Should produce strong: {}",
            result
        );
        assert!(
            !result.contains("#underline["),
            "Should not produce underline: {}",
            result
        );
    }

    // Security tests: inline code backtick injection
    #[test]
    fn test_inline_code_backtick_injection() {
        // Content with a backtick must use multi-backtick delimiters
        // to prevent breaking out of Typst raw text
        let result = mark_to_typst("`` `inject` ``").unwrap();
        // The content is "`inject`" — must not produce `...`inject`...`
        // which would break the raw text delimiters
        assert!(
            !result.contains("``inject``"),
            "Backticks should not form nested delimiters"
        );
        // Multi-backtick delimiters should be used
        assert!(
            result.contains("`` "),
            "Should use double-backtick delimiters"
        );
    }

    #[test]
    fn test_inline_code_consecutive_backticks() {
        // Content with consecutive backticks
        let result = mark_to_typst("``` `` ```").unwrap();
        // Content is "``" — needs at least 3 backtick delimiters
        assert!(
            result.contains("```"),
            "Should use triple-backtick delimiters for double-backtick content"
        );
    }

    // Security tests: code block language string sanitization
    #[test]
    fn test_code_block_lang_sanitized_simple() {
        // Normal language tags should pass through
        let result = mark_to_typst("```rust\ncode\n```").unwrap();
        assert_eq!(result, "```rust\ncode\n```\n\n");
    }

    #[test]
    fn test_code_block_lang_sanitized_special_chars() {
        // Language tag with special characters should be sanitized
        // pulldown-cmark extracts info string as-is; we strip dangerous chars
        let result = mark_to_typst("```rust#evil\ncode\n```").unwrap();
        // '#' should be stripped, only "rust" remains
        assert!(
            result.starts_with("```rust\n"),
            "Lang tag should be sanitized to 'rust': got {}",
            result
        );
        assert!(
            !result.contains("#evil"),
            "Special chars should be stripped from lang tag"
        );
    }

    #[test]
    fn test_code_block_lang_allows_common_identifiers() {
        // c++, objective-c, c_sharp etc. should be preserved
        let result = mark_to_typst("```c++\ncode\n```").unwrap();
        assert!(
            result.starts_with("```c++\n"),
            "c++ lang tag should be preserved"
        );

        let result = mark_to_typst("```objective-c\ncode\n```").unwrap();
        assert!(
            result.starts_with("```objective-c\n"),
            "objective-c lang tag should be preserved"
        );
    }

    // Security tests: sanitize_lang_tag unit tests
    #[test]
    fn test_sanitize_lang_tag_basic() {
        assert_eq!(sanitize_lang_tag("rust"), "rust");
        assert_eq!(sanitize_lang_tag("c++"), "c++");
        assert_eq!(sanitize_lang_tag("objective-c"), "objective-c");
        assert_eq!(sanitize_lang_tag("file.typ"), "file.typ");
    }

    #[test]
    fn test_sanitize_lang_tag_strips_injection() {
        // Newlines, Typst markup chars, etc. should be stripped
        assert_eq!(sanitize_lang_tag("rust\n#import"), "rust");
        assert_eq!(sanitize_lang_tag("rust`code`"), "rust");
        assert_eq!(sanitize_lang_tag("rust[evil]"), "rust");
        assert_eq!(sanitize_lang_tag("rust$math$"), "rust");
        assert_eq!(sanitize_lang_tag(""), "");
    }

    // Security test: longest_backtick_run helper
    #[test]
    fn test_longest_backtick_run() {
        assert_eq!(longest_backtick_run("no backticks"), 0);
        assert_eq!(longest_backtick_run("one ` here"), 1);
        assert_eq!(longest_backtick_run("two `` here"), 2);
        assert_eq!(longest_backtick_run("mixed ` and `` here"), 2);
        assert_eq!(longest_backtick_run("```"), 3);
        assert_eq!(longest_backtick_run(""), 0);
    }

    #[test]
    fn test_mismatched_asterisks_graceful_degradation() {
        // `*lethality**` has mismatched asterisks — pulldown_cmark parses `*lethality*`
        // as emphasis and leaves the trailing `*` as literal text. Previously, the
        // MarkdownFixer incorrectly consumed the trailing `*` as a closing event,
        // producing an extra `]` bracket that caused a hard Typst compilation error.
        let result = mark_to_typst("Less formatting. More *lethality**.").unwrap();
        assert!(
            !result.contains("]]"),
            "Should not produce unmatched closing brackets: got {:?}",
            result
        );
        assert!(
            result.contains("#emph[lethality]"),
            "Should produce valid emphasis markup: got {:?}",
            result
        );
        // The trailing `*` should be escaped as literal text
        assert!(
            result.contains("\\*."),
            "Trailing asterisk should be escaped: got {:?}",
            result
        );
    }

    #[test]
    fn test_mismatched_asterisks_variants() {
        // Various mismatched asterisk patterns should not error
        let cases = vec![
            "Hello **world*",
            "*hello** world",
            "***triple* mismatch",
            "text *one *two* three",
        ];
        for input in cases {
            let result = mark_to_typst(input);
            assert!(
                result.is_ok(),
                "Should not error on {:?}: got {:?}",
                input,
                result
            );
        }
    }

    /// Helper: count unmatched brackets in Typst output (ignoring escaped ones)
    fn count_unmatched_brackets(typst: &str) -> (usize, usize) {
        let mut depth: i64 = 0;
        let mut max_negative: i64 = 0;
        let chars: Vec<char> = typst.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' {
                i += 2; // skip escaped char
                continue;
            }
            // Skip string literals
            if chars[i] == '"' {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1; // closing quote
                continue;
            }
            // Skip code blocks (``` ... ```)
            if i + 2 < chars.len() && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`'
            {
                i += 3;
                while i + 2 < chars.len()
                    && !(chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`')
                {
                    i += 1;
                }
                i += 3;
                continue;
            }
            match chars[i] {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth < max_negative {
                        max_negative = depth;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        // Returns (unclosed opens, unmatched closes)
        let unclosed = depth.max(0) as usize;
        let unmatched_close = (-max_negative).max(0) as usize;
        (unclosed, unmatched_close)
    }

    #[test]
    fn test_bracket_balance_on_malformed_markdown() {
        let cases = vec![
            "Less formatting. More *lethality**.",
            "*hello** world",
            "Hello **world*",
            "***triple* mismatch",
            "text *one *two* three",
            "**",
            "*",
            "***",
            "text with __unclosed",
            "hello __world",
            "__underline without close",
            "*a **b ***c",
            "***triple then single*",
            "mixed __under and *emph combo",
            "a]b", // literal bracket in text
            "a[b", // literal bracket in text
            "pre***__content__***",
            "text **bold **nested** end",
            "__a__ and __b",
            "~~strike~~ and ~~unclosed",
        ];

        for input in cases {
            let result = mark_to_typst(input);
            assert!(
                result.is_ok(),
                "Should not error on {:?}: {:?}",
                input,
                result
            );
            let typst = result.unwrap();
            let (unclosed, unmatched) = count_unmatched_brackets(&typst);
            assert_eq!(
                unclosed, 0,
                "Unclosed '[' in output for {:?}: output={:?}",
                input, typst
            );
            assert_eq!(
                unmatched, 0,
                "Unmatched ']' in output for {:?}: output={:?}",
                input, typst
            );
        }
    }
}
