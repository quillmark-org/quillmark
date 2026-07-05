//! Markdown import (cold): `normalize → pulldown → corpus`.
//!
//! The one place the `<u>` allowlist and `***` fixups run — once, at the
//! boundary (issue #831 § Codecs). Input is normalized by
//! [`crate::normalize::normalize_markdown`] (CRLF→LF, bidi strip, HTML
//! comment-fence repair) so the corpus invariants hold by construction, then
//! parsed with `pulldown_cmark` (CommonMark + strikethrough + pipe tables) and
//! walked into a [`RichText`].
//!
//! ## Phase-1 canonicalizations (documented, not bugs)
//!
//! Import maps some distinct markdown to one canonical corpus. All of them, in
//! one place:
//!
//! - **Soft breaks → space; hard breaks → a `continues` line.** A soft break is
//!   a space (CommonMark rendering); a hard break (two trailing spaces or `\`)
//!   is a within-block continuation line ([`crate::model::Line::continues`]),
//!   kept distinct from a paragraph boundary. A hard break inside a heading is a
//!   space (ATX headings can't carry one).
//! - **Adjacent sibling lists of the same shape merge.** Two consecutive lists
//!   of the same kind whose items share an `ordinal` (`* a` then `+ b`, or two
//!   ordered lists both starting at 1) are indistinguishable from one list /
//!   one multi-paragraph item — item identity is positional `ordinal`, not a
//!   minted list instance. Adjacent block quotes likewise merge into one.
//! - **Empty blocks and containers keep their line.** An empty heading (`#`),
//!   empty paragraph, empty `- ` item, or empty `>` quote each yields one empty
//!   line so the structure survives, rather than vanishing.
//! - **Island ids are minted sequentially** (`isl-0`, `isl-1`, …) so import is a
//!   pure, deterministic function. Real minting (the hash-nondeterminism source)
//!   is phase 4; sequential ids round-trip (export drops them, re-import
//!   re-mints the same sequence).
//! - **Tables and images are islands.** Tables are block islands (their own
//!   `Island` line); images are inline island slots. Both `Lossless` — pipe
//!   tables and `![alt](url)` carry them faithfully.

use crate::model::{
    Container, Island, Line, LineKind, Loss, Mark, MarkKind, RichText, ISLAND_SLOT,
};
use crate::normalize::normalize_markdown;
use crate::MAX_NESTING_DEPTH;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::json;
use std::ops::Range;

/// Import errors. Phase-1 surface is just the nesting guard (mirrors the typst
/// backend's `ConversionError::NestingTooDeep`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Container nesting exceeded [`MAX_NESTING_DEPTH`].
    NestingTooDeep { depth: usize, max: usize },
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NestingTooDeep { depth, max } => {
                write!(f, "nesting too deep: {depth} (max {max})")
            }
        }
    }
}
impl std::error::Error for ImportError {}

/// Import markdown into a normalized, validated [`RichText`] corpus.
pub fn from_markdown(markdown: &str) -> Result<RichText, ImportError> {
    let normalized = normalize_markdown(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(&normalized, options);
    let fixer = MarkdownFixer::new(parser.into_offset_iter(), &normalized);

    let mut b = Builder::new(&normalized);
    b.run(fixer)?;
    let mut rt = b.finish();
    rt.normalize();
    Ok(rt)
}

// ---------------------------------------------------------------------------
// Corpus builder
// ---------------------------------------------------------------------------

struct Builder<'a> {
    source: &'a str,
    text: String,
    pos: usize, // USV position = char count of `text`
    lines: Vec<Line>,
    cur: Option<Line>, // the line currently open (kind + containers fixed at open)
    /// A block start records `(kind, continues)` the next inline content should
    /// open a fresh line with. Set at Paragraph/Heading/Item (tight lists emit no
    /// Paragraph wrapper, so Item must force a line) with `continues = false`; a
    /// hard break sets `continues = true`. Cleared when a block that owns its own
    /// lines (List/Quote/CodeBlock/Table) takes over.
    pending: Option<(LineKind, bool)>,
    marks: Vec<Mark>,
    open_marks: Vec<(MarkKind, usize)>, // (kind, start)
    islands: Vec<Island>,
    island_seq: usize,
    containers: Vec<Container>,
    /// Parallel to `containers`: the [`Self::emitted`] count when each container
    /// opened, so a container that closes having emitted no line (an empty `>`
    /// quote, an empty `- ` item) can still get one.
    container_marks: Vec<usize>,
    list_stack: Vec<ListInfo>,
    // code block
    code_lang: Option<String>,
    in_code: bool,
    code_opened: bool, // whether the current code block has opened its first line
    // image collection
    image_depth: usize,
    image_url: String,
    image_alt: String,
    // table collection
    table: Option<TableAcc>,
}

#[derive(Clone)]
struct ListInfo {
    ordered: bool,
    start: u64,
    /// 0-based index of the next item — becomes the item's `ordinal`.
    count: u64,
}

struct TableAcc {
    aligns: Vec<&'static str>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    cur_row: Vec<String>,
    in_head: bool,
}

fn align_str(a: &pulldown_cmark::Alignment) -> &'static str {
    match a {
        pulldown_cmark::Alignment::None => "none",
        pulldown_cmark::Alignment::Left => "left",
        pulldown_cmark::Alignment::Center => "center",
        pulldown_cmark::Alignment::Right => "right",
    }
}

impl<'a> Builder<'a> {
    fn new(source: &'a str) -> Self {
        Builder {
            source,
            text: String::new(),
            pos: 0,
            lines: Vec::new(),
            cur: None,
            pending: None,
            marks: Vec::new(),
            open_marks: Vec::new(),
            islands: Vec::new(),
            island_seq: 0,
            containers: Vec::new(),
            container_marks: Vec::new(),
            list_stack: Vec::new(),
            code_lang: None,
            in_code: false,
            code_opened: false,
            image_depth: 0,
            image_url: String::new(),
            image_alt: String::new(),
            table: None,
        }
    }

    /// Open a fresh line with `kind` and the current container path. The first
    /// open sets the line directly; each later one first closes the previous
    /// line with a single `\n` boundary — so `lines.len()` always equals the
    /// `\n`-segment count.
    fn open_line(&mut self, kind: LineKind, continues: bool) {
        // The first line (no line yet open) can never continue anything.
        let continues = continues && self.cur.is_some();
        if let Some(prev) = self.cur.take() {
            self.text.push('\n');
            self.pos += 1;
            self.lines.push(prev);
        }
        self.cur = Some(Line {
            kind,
            containers: self.containers.clone(),
            continues,
        });
    }

    /// Open a fresh line for a `pending_kind` set at the last block start, or
    /// (defensively) a `default` line if inline content arrives with none
    /// pending and no line open. A no-op when a line is already open and no new
    /// one is pending — inline content flows onto the current line.
    fn ensure_open(&mut self, default: LineKind) {
        if let Some((k, cont)) = self.pending.take() {
            self.open_line(k, cont);
        } else if self.cur.is_none() {
            self.open_line(default, false);
        }
    }

    /// Append inline text to the current line, stripping any characters the
    /// corpus invariants forbid (stray `\r`, stray island slots; stray `\n`
    /// becomes a space — inline text should carry none).
    fn push_inline(&mut self, s: &str) {
        self.ensure_open(LineKind::Para);
        for c in s.chars() {
            let c = match c {
                '\r' => continue,
                ISLAND_SLOT => continue,
                '\n' => ' ',
                other => other,
            };
            self.text.push(c);
            self.pos += 1;
        }
    }

    /// Lines emitted so far, counting the line currently open. A container that
    /// closes with this unchanged from when it opened produced nothing.
    fn emitted(&self) -> usize {
        self.lines.len() + usize::from(self.cur.is_some())
    }

    /// Open a line for a block that ended with no inline content (an empty
    /// heading `#`, an empty paragraph) — otherwise the block, and any content
    /// model it carries, is silently lost.
    fn flush_empty_block(&mut self) {
        if let Some((k, cont)) = self.pending.take() {
            self.open_line(k, cont);
        }
    }

    /// Close a container: if it emitted no line, give it one empty `Para` line
    /// (an empty `- ` item, an empty `>` quote) so the structure survives; then
    /// pop it. `mark` is the [`Self::emitted`] snapshot from when it opened.
    fn close_container(&mut self, mark: usize) {
        if self.emitted() == mark {
            self.pending = None;
            self.open_line(LineKind::Para, false);
        }
        self.containers.pop();
    }

    fn open_mark(&mut self, kind: MarkKind) {
        // Resolve any armed line first, so a mark that begins a block records
        // the position *after* the block's line boundary — not the `\n` before
        // it. Without this the mark swallows the separator and equal content
        // from an editor vs from import serializes to different canonical bytes.
        self.ensure_open(LineKind::Para);
        self.open_marks.push((kind, self.pos));
    }

    fn close_mark(&mut self) {
        // Well-nested by pulldown: close the innermost open mark.
        if let Some((kind, start)) = self.open_marks.pop() {
            self.marks.push(Mark {
                start,
                end: self.pos,
                kind,
            });
        }
    }

    fn mint_island(&mut self, island_type: &str, props: serde_json::Value, loss: Loss) {
        let id = format!("isl-{}", self.island_seq);
        self.island_seq += 1;
        self.islands.push(Island {
            id,
            island_type: island_type.to_string(),
            props,
            loss,
        });
    }

    fn check_depth(&self) -> Result<(), ImportError> {
        // Container path plus open marks approximates the structural depth the
        // typst backend caps; bound it identically for parity.
        let depth = self.containers.len() + self.open_marks.len();
        if depth > MAX_NESTING_DEPTH {
            return Err(ImportError::NestingTooDeep {
                depth,
                max: MAX_NESTING_DEPTH,
            });
        }
        Ok(())
    }

    fn run<I>(&mut self, iter: I) -> Result<(), ImportError>
    where
        I: Iterator<Item = (Event<'a>, Range<usize>)>,
    {
        for (event, range) in iter {
            // Image alt collection intercepts everything until the image closes.
            if self.image_depth > 0 {
                match &event {
                    Event::Start(Tag::Image { .. }) => self.image_depth += 1,
                    Event::End(TagEnd::Image) => {
                        self.image_depth -= 1;
                        if self.image_depth == 0 {
                            self.emit_image();
                        }
                    }
                    Event::Text(t) | Event::Code(t) => self.image_alt.push_str(t),
                    Event::SoftBreak | Event::HardBreak => self.image_alt.push(' '),
                    _ => {}
                }
                continue;
            }

            // Table collection routes structural events to the accumulator and
            // ignores inline content (captured by cell source-slicing).
            if self.table.is_some() {
                self.table_event(&event, &range);
                if matches!(event, Event::End(TagEnd::Table)) {
                    self.emit_table();
                }
                continue;
            }

            match event {
                Event::Start(tag) => self.start_tag(tag, range)?,
                Event::End(tag) => self.end_tag(tag),
                Event::Text(t) => {
                    if self.in_code {
                        self.push_code_content(&t);
                    } else {
                        self.push_inline(&t);
                    }
                }
                Event::Code(t) => {
                    self.ensure_open(LineKind::Para);
                    let start = self.pos;
                    self.push_inline(&t);
                    self.marks.push(Mark {
                        start,
                        end: self.pos,
                        kind: MarkKind::Code,
                    });
                }
                Event::SoftBreak => self.push_inline(" "),
                Event::HardBreak => {
                    match self.cur.as_ref().map(|l| &l.kind) {
                        // ATX headings can't carry a hard break in markdown, so
                        // one inside a heading canonicalizes to a space (a
                        // documented, representable choice).
                        Some(LineKind::Heading { .. }) => self.push_inline(" "),
                        // Elsewhere: a within-block line break — arm a pending
                        // continuation line (same kind, continues = true) so it
                        // stays one block and export re-emits a hard break, not a
                        // paragraph split.
                        _ => {
                            let kind = self
                                .cur
                                .as_ref()
                                .map(|l| l.kind.clone())
                                .unwrap_or(LineKind::Para);
                            self.pending = Some((kind, true));
                        }
                    }
                }
                // Html/InlineHtml already stripped or rewritten by the fixer;
                // math/footnotes/etc. produce no corpus content.
                _ => {}
            }
        }
        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'a>, range: Range<usize>) -> Result<(), ImportError> {
        match tag {
            // Block starts arm a pending line (new block, continues = false);
            // the next inline content opens it.
            Tag::Paragraph => self.pending = Some((LineKind::Para, false)),
            Tag::Heading { level, .. } => {
                self.pending = Some((
                    LineKind::Heading {
                        level: heading_level(level),
                    },
                    false,
                ))
            }
            Tag::CodeBlock(kind) => {
                self.pending = None; // code opens its own lines
                self.in_code = true;
                self.code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        let l = sanitize_lang(&lang);
                        if l.is_empty() {
                            None
                        } else {
                            Some(l)
                        }
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
                // First code line opens on the first content chunk; nothing to
                // open yet (a code block with no content still yields one line,
                // handled in push_code_content / end).
                self.code_opened = false;
            }
            Tag::List(start) => {
                self.pending = None; // nested list content sets its own
                self.list_stack.push(ListInfo {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    count: 0,
                });
            }
            Tag::Item => {
                // Tight-list items carry no Paragraph wrapper, so the item start
                // is what forces a new line for the item's first inline content.
                self.pending = Some((LineKind::Para, false));
                self.container_marks.push(self.emitted());
                let container = match self.list_stack.last_mut() {
                    Some(info) => {
                        let ordinal = info.count;
                        info.count += 1;
                        Container::ListItem {
                            ordered: info.ordered,
                            start: info.start,
                            ordinal,
                        }
                    }
                    None => Container::ListItem {
                        ordered: false,
                        start: 1,
                        ordinal: 0,
                    },
                };
                self.containers.push(container);
                self.check_depth()?;
            }
            Tag::BlockQuote(_) => {
                self.pending = None; // quote content sets its own
                self.container_marks.push(self.emitted());
                self.containers.push(Container::Quote);
                self.check_depth()?;
            }
            Tag::Table(aligns) => {
                self.pending = None;
                self.open_line(LineKind::Island, false);
                self.text.push(ISLAND_SLOT);
                self.pos += 1;
                self.table = Some(TableAcc {
                    aligns: aligns.iter().map(align_str).collect(),
                    header: Vec::new(),
                    rows: Vec::new(),
                    cur_row: Vec::new(),
                    in_head: false,
                });
            }
            Tag::Emphasis => {
                self.open_mark(MarkKind::Emph);
                self.check_depth()?;
            }
            Tag::Strong => {
                // The fixer rewrites `<u>` to Start(Strong); distinguish it by
                // peeking the source (parity with the typst backend).
                let kind = if self
                    .source
                    .get(range.start..range.start + 2)
                    .is_some_and(|s| s.eq_ignore_ascii_case("<u"))
                {
                    MarkKind::Underline
                } else {
                    MarkKind::Strong
                };
                self.open_mark(kind);
                self.check_depth()?;
            }
            Tag::Strikethrough => {
                self.open_mark(MarkKind::Strike);
                self.check_depth()?;
            }
            Tag::Link { dest_url, .. } => {
                self.open_mark(MarkKind::Link {
                    url: dest_url.to_string(),
                });
                self.check_depth()?;
            }
            Tag::Image { dest_url, .. } => {
                self.image_url = dest_url.to_string();
                self.image_alt.clear();
                self.image_depth = 1;
            }
            _ => {}
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::CodeBlock => {
                if !self.code_opened {
                    // Empty code block: one empty Code line.
                    let lang = self.code_lang.take();
                    self.open_line(LineKind::Code { lang }, false);
                }
                self.in_code = false;
                self.code_lang = None;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
            }
            TagEnd::BlockQuote(_) => {
                let mark = self.container_marks.pop().unwrap_or(0);
                self.close_container(mark);
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link => self.close_mark(),
            // A block that produced no inline content still gets its line.
            TagEnd::Heading(_) | TagEnd::Paragraph => self.flush_empty_block(),
            _ => {}
        }
    }

    fn push_code_content(&mut self, content: &str) {
        // pulldown appends a trailing newline as the last line's terminator, not
        // content; drop exactly one so an N-line block yields N lines.
        let content = content.strip_suffix('\n').unwrap_or(content);
        for seg in content.split('\n') {
            // First line of the block starts it (continues = false); every later
            // line is a within-block continuation, so the fence stays one block.
            let continues = self.code_opened;
            self.open_line(
                LineKind::Code {
                    lang: self.code_lang.clone(),
                },
                continues,
            );
            self.code_opened = true;
            // Code text is literal; still enforce corpus invariants.
            self.push_code_line(seg);
        }
    }

    fn push_code_line(&mut self, seg: &str) {
        for c in seg.chars() {
            let c = match c {
                '\r' | '\n' => continue,
                ISLAND_SLOT => continue,
                other => other,
            };
            self.text.push(c);
            self.pos += 1;
        }
    }

    // ---- table ----

    fn table_event(&mut self, event: &Event, range: &Range<usize>) {
        let Some(acc) = self.table.as_mut() else {
            return;
        };
        match event {
            Event::Start(Tag::TableHead) => acc.in_head = true,
            Event::End(TagEnd::TableHead) => {
                acc.header = std::mem::take(&mut acc.cur_row);
                acc.in_head = false;
            }
            Event::Start(Tag::TableRow) => acc.cur_row.clear(),
            Event::End(TagEnd::TableRow) => {
                if !acc.in_head {
                    let row = std::mem::take(&mut acc.cur_row);
                    acc.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                // Capture the cell's markdown source verbatim (preserves inline
                // formatting), so a pipe table round-trips losslessly.
                let cell = self.source.get(range.clone()).unwrap_or("").trim();
                acc.cur_row.push(cell.to_string());
            }
            _ => {}
        }
    }

    fn emit_table(&mut self) {
        if let Some(acc) = self.table.take() {
            let props = json!({
                "aligns": acc.aligns,
                "header": acc.header,
                "rows": acc.rows,
            });
            self.mint_island("table", props, Loss::Lossless);
        }
    }

    fn emit_image(&mut self) {
        self.ensure_open(LineKind::Para);
        self.text.push(ISLAND_SLOT);
        self.pos += 1;
        let props = json!({
            "url": self.image_url,
            "alt": self.image_alt.trim(),
        });
        self.mint_island("image", props, Loss::Lossless);
    }

    fn finish(mut self) -> RichText {
        if let Some(last) = self.cur.take() {
            self.lines.push(last);
        }
        if self.lines.is_empty() {
            // Empty document: one empty Para line.
            self.lines.push(Line {
                kind: LineKind::Para,
                containers: Vec::new(),
                continues: false,
            });
        }
        // Close any marks left open (unterminated `<u>`, malformed input).
        while !self.open_marks.is_empty() {
            self.close_mark();
        }
        RichText {
            text: self.text,
            lines: self.lines,
            marks: self.marks,
            islands: self.islands,
        }
    }
}

fn heading_level(level: pulldown_cmark::HeadingLevel) -> u8 {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => 1,
        H2 => 2,
        H3 => 3,
        H4 => 4,
        H5 => 5,
        H6 => 6,
    }
}

/// Sanitize a code-block info string to a language identifier (parity with the
/// typst backend's `sanitize_lang_tag`).
fn sanitize_lang(lang: &str) -> String {
    lang.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

// ---------------------------------------------------------------------------
// MarkdownFixer — ported from crates/backends/typst/src/convert.rs.
//
// Two jobs before events reach the builder: allowlist `<u>…</u>` as underline
// (rewrite to Strong start/end, detected by source peek) and strip all other
// raw HTML; and fix `***`-adjacency runs pulldown splits awkwardly. Phase 2
// unifies this with the backend's copy; phase 1 duplicates it to stay
// engine-off.
// ---------------------------------------------------------------------------

fn is_u_open_tag(html: &str) -> bool {
    let s = html.trim();
    if s.starts_with('<') && s.ends_with('>') {
        s[1..s.len() - 1].trim().eq_ignore_ascii_case("u")
    } else {
        false
    }
}

fn is_u_close_tag(html: &str) -> bool {
    let s = html.trim();
    if s.starts_with("</") && s.ends_with('>') {
        s[2..s.len() - 1].trim().eq_ignore_ascii_case("u")
    } else {
        false
    }
}

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

    fn events_for_stars(
        star_count: usize,
        is_start: bool,
        start_idx: usize,
    ) -> Vec<(Event<'a>, Range<usize>)> {
        let mut events = Vec::new();
        let mut offset = 0;
        let mut remaining = star_count;

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
        if !is_start {
            events.reverse();
        }
        events
    }

    fn coalesce_text_range(&mut self, initial_range: Range<usize>) -> Range<usize> {
        let mut merged_range = initial_range;
        while let Some((next_event, next_range)) = self.inner.peek() {
            if matches!(next_event, Event::Text(_)) && next_range.start == merged_range.end {
                merged_range.end = next_range.end;
                self.inner.next();
            } else {
                break;
            }
        }
        merged_range
    }

    fn closable_star_count(&self, star_count: usize) -> usize {
        let mut remaining = star_count;
        let mut consumed = 0;
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
                            let star_events =
                                Self::events_for_stars(star_count, true, range.start + text_len);
                            let next_event = if !self.buffer.is_empty() {
                                self.buffer.pop().unwrap()
                            } else {
                                self.inner.next().unwrap()
                            };
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
                let has_open_tags = self.emph_depth > 0 || self.strong_depth > 0;
                if !has_open_tags {
                    return Some((event, range));
                }
                let next_is_star_text =
                    if let Some((Event::Text(cow_str), _)) = self.buffer.last() {
                        cow_str.starts_with('*')
                    } else if let Some((Event::Text(cow_str), _)) = self.inner.peek() {
                        cow_str.starts_with('*')
                    } else {
                        false
                    };
                if next_is_star_text {
                    let (text_event, text_range) = if !self.buffer.is_empty() {
                        self.buffer.pop().unwrap()
                    } else {
                        let (_ev, rng) = self.inner.next().unwrap();
                        let merged_range = self.coalesce_text_range(rng);
                        let text = self.source[merged_range.clone()].into();
                        (Event::Text(text), merged_range)
                    };
                    if let Event::Text(cow_str) = text_event {
                        let s = cow_str.as_ref();
                        let star_count = s.chars().take_while(|c| *c == '*').count();
                        let consumable = self.closable_star_count(star_count);
                        if consumable > 0 {
                            let star_events =
                                Self::events_for_stars(consumable, false, text_range.start);
                            let text_after = &s[consumable..];
                            if !text_after.is_empty() {
                                self.buffer.push((
                                    Event::Text(text_after.to_string().into()),
                                    text_range.start + consumable..text_range.end,
                                ));
                            }
                            for ev in star_events.into_iter().rev() {
                                self.buffer.push(ev);
                            }
                            return Some((event, range));
                        } else {
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
            if let Some(event) = self.buffer.pop() {
                if let Some(result) = self.handle_candidate(event) {
                    return Some(result);
                } else {
                    continue;
                }
            }
            let (event, range) = self.inner.next()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineKind;

    fn imp(md: &str) -> RichText {
        let rt = from_markdown(md).unwrap();
        assert_eq!(rt.validate(), Ok(()), "invariants for {md:?}");
        rt
    }

    #[test]
    fn plain_paragraph() {
        let rt = imp("Hello world");
        assert_eq!(rt.text, "Hello world");
        assert_eq!(rt.lines.len(), 1);
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert!(rt.marks.is_empty());
    }

    #[test]
    fn bold_and_italic_marks() {
        let rt = imp("a **b** _c_");
        assert_eq!(rt.text, "a b c");
        // "b" at 2..3 strong, "c" at 4..5 emph
        assert!(rt.marks.contains(&Mark {
            start: 2,
            end: 3,
            kind: MarkKind::Strong
        }));
        assert!(rt.marks.contains(&Mark {
            start: 4,
            end: 5,
            kind: MarkKind::Emph
        }));
    }

    #[test]
    fn underline_from_u_tag() {
        let rt = imp("x <u>y</u> z");
        assert_eq!(rt.text, "x y z");
        assert!(rt.marks.iter().any(|m| m.kind == MarkKind::Underline
            && m.start == 2
            && m.end == 3));
    }

    #[test]
    fn other_html_stripped() {
        let rt = imp("a <span>b</span> c");
        assert_eq!(rt.text, "a b c");
    }

    #[test]
    fn two_paragraphs_two_lines() {
        let rt = imp("one\n\ntwo");
        assert_eq!(rt.text, "one\ntwo");
        assert_eq!(rt.lines.len(), 2);
        assert!(rt.lines.iter().all(|l| l.kind == LineKind::Para));
    }

    #[test]
    fn heading_line_kind() {
        let rt = imp("## Title");
        assert_eq!(rt.text, "Title");
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 2 });
    }

    #[test]
    fn inline_code_mark() {
        let rt = imp("run `cargo test` now");
        assert_eq!(rt.text, "run cargo test now");
        assert!(rt
            .marks
            .iter()
            .any(|m| m.kind == MarkKind::Code && m.start == 4 && m.end == 14));
    }

    #[test]
    fn code_block_lines() {
        let rt = imp("```rust\nfn a() {}\nfn b() {}\n```");
        assert_eq!(rt.text, "fn a() {}\nfn b() {}");
        assert_eq!(rt.lines.len(), 2);
        assert!(rt
            .lines
            .iter()
            .all(|l| l.kind == LineKind::Code { lang: Some("rust".into()) }));
    }

    #[test]
    fn bullet_list_containers() {
        let rt = imp("- a\n- b");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        // Two items: same list (ordered=false, start=1), distinct ordinals.
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 0
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 1
            }]
        );
    }

    #[test]
    fn ordered_list_custom_start() {
        let rt = imp("3. a\n4. b");
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 0
            }]
        );
        assert_eq!(
            rt.lines[1].containers,
            vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1
            }]
        );
    }

    #[test]
    fn multi_paragraph_list_item_shares_container() {
        // One item with two paragraphs -> two Para lines sharing one ListItem.
        let rt = imp("- first\n\n  second");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.lines[0].containers, rt.lines[1].containers);
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal: 0
            }]
        );
    }

    #[test]
    fn blockquote_container() {
        let rt = imp("> quoted");
        assert_eq!(rt.text, "quoted");
        assert_eq!(rt.lines[0].containers, vec![Container::Quote]);
    }

    #[test]
    fn table_is_block_island() {
        let rt = imp("| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(rt.text, "\u{FFFC}");
        assert_eq!(rt.lines[0].kind, LineKind::Island);
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, "table");
        assert_eq!(rt.islands[0].loss, Loss::Lossless);
    }

    #[test]
    fn image_is_inline_island() {
        let rt = imp("see ![a cat](cat.png) here");
        assert_eq!(rt.text, "see \u{FFFC} here");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, "image");
        assert_eq!(rt.islands[0].props["url"], "cat.png");
        assert_eq!(rt.islands[0].props["alt"], "a cat");
    }

    #[test]
    fn empty_list_item_keeps_its_line() {
        // An empty `- ` item (here an empty bullet nested in an ordered item)
        // must not vanish (regression for the container-flush fix).
        let rt = imp("- a\n-\n- b");
        assert_eq!(rt.lines.len(), 3, "empty middle item preserved");
    }

    #[test]
    fn empty_blockquote_keeps_its_line() {
        let rt = imp("> ");
        assert_eq!(rt.lines.len(), 1);
        assert_eq!(rt.lines[0].containers, vec![Container::Quote]);
    }

    #[test]
    fn adjacent_sibling_lists_merge_is_stable() {
        // Documented canonicalization: two sibling bullet lists collapse to one.
        // Distinct markdown, one corpus — but the corpus is a fixed point.
        let rt = imp("* a\n\n+ b");
        let rt2 = from_markdown(&crate::export::to_markdown(&rt)).unwrap();
        assert_eq!(rt, rt2, "merged sibling lists still round-trip");
    }

    #[test]
    fn empty_input_one_empty_line() {
        let rt = imp("");
        assert_eq!(rt.text, "");
        assert_eq!(rt.lines.len(), 1);
    }

    #[test]
    fn mark_does_not_swallow_leading_newline() {
        // Regression (review finding 1): a mark starting a block must begin at
        // the content, not on the preceding line boundary.
        let rt = imp("a\n\n**b**");
        assert_eq!(rt.text, "a\nb");
        let m = &rt.marks[0];
        assert_eq!((m.start, m.end), (2, 3));
        assert_eq!(rt.text.chars().nth(m.start), Some('b'));
    }

    #[test]
    fn import_and_editor_corpus_same_canonical_bytes() {
        // The freeze's central promise: equal content → equal bytes, whatever
        // the producer. Import of "a\n\n**b**" must byte-match a hand-built
        // editor corpus of the same content.
        let imported = imp("a\n\n**b**");
        let editor = RichText {
            text: "a\nb".into(),
            lines: vec![
                Line {
                    kind: LineKind::Para,
                    containers: vec![],
                    continues: false,
                },
                Line {
                    kind: LineKind::Para,
                    containers: vec![],
                    continues: false,
                },
            ],
            marks: vec![Mark {
                start: 2,
                end: 3,
                kind: MarkKind::Strong,
            }],
            islands: vec![],
        };
        assert_eq!(imported.to_canonical_json(), editor.to_canonical_json());
    }

    #[test]
    fn hard_break_is_a_continuation_line() {
        let rt = imp("line one\\\nline two");
        assert_eq!(rt.text, "line one\nline two");
        assert_eq!(rt.lines.len(), 2);
        assert!(!rt.lines[0].continues);
        assert!(rt.lines[1].continues, "hard break -> continuation line");
    }

    #[test]
    fn heading_cannot_carry_hard_break() {
        // ATX headings are single-line: `## a  \nb` is a heading plus a separate
        // paragraph, never a heading with a continuation. (The heading→space
        // canonicalization in HardBreak handling is defensive for editor-built
        // corpora, unreachable via markdown import.)
        let rt = imp("## a  \nb");
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.lines[0].kind, LineKind::Heading { level: 2 });
        assert_eq!(rt.lines[1].kind, LineKind::Para);
        assert!(!rt.lines[1].continues, "separate block, not a continuation");
    }

    #[test]
    fn astral_positions_are_usv() {
        let rt = imp("a😀**b**");
        // 'a'(0) '😀'(1) 'b'(2) — strong over "b" is 2..3 in USV.
        assert_eq!(rt.text, "a😀b");
        assert!(rt.marks.iter().any(|m| m.start == 2 && m.end == 3 && m.kind == MarkKind::Strong));
    }
}
