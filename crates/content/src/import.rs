//! Markdown import (cold): `normalize → pulldown → content`.
//!
//! The one place the `<u>` allowlist and `***` fixups run — once, at the
//! boundary (issue #831 § Codecs). Input is normalized by
//! [`crate::normalize::normalize_markdown`] (CRLF→LF, bidi strip, HTML
//! comment-fence repair) so the content invariants hold by construction, then
//! parsed with `pulldown_cmark` (CommonMark + strikethrough + pipe tables) and
//! walked into a [`Content`].
//!
//! ## Canonicalizations (documented, not bugs)
//!
//! Import maps some distinct markdown to one canonical content. All of them, in
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
//!   is not yet implemented; sequential ids round-trip (export drops them,
//!   re-import re-mints the same sequence).
//! - **Tables and images are islands.** Tables are block islands (their own
//!   `Island` line); images are inline island slots. Both `Lossless` — pipe
//!   tables and `![alt](url)` carry them faithfully.
//! - **Thematic breaks are `Rule` lines.** `---`/`***`/`___` in prose (never
//!   the root-block frontmatter alias, resolved before this layer runs) map
//!   to a `LineKind::Rule` line carrying no text — the break is the line
//!   itself.

use crate::model::{
    Container, Island, Line, LineKind, Loss, Mark, MarkKind, Content, ISLAND_SLOT,
};
use crate::normalize::normalize_markdown;
use crate::MAX_NESTING_DEPTH;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

/// Byte offsets at which the [`MarkdownFixer`] converted a `<u>` open tag into a
/// `Tag::Strong` event. The fixer is the one place that classifies `<u>` (via
/// [`is_u_open_tag`]); it records the fact here so the [`Builder`] can tell a
/// `<u>`-derived strong from a real `**` one without re-sniffing source bytes.
type UnderlineOpens = Rc<RefCell<HashSet<usize>>>;

/// Import errors: just the nesting guard (mirrors the typst backend's
/// `ConversionError::NestingTooDeep`).
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

/// Import markdown into a normalized, validated [`Content`] content.
pub fn from_markdown(markdown: &str) -> Result<Content, ImportError> {
    let normalized = normalize_markdown(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(&normalized, options);
    let underline_opens: UnderlineOpens = Rc::new(RefCell::new(HashSet::new()));
    let fixer = MarkdownFixer::new(
        parser.into_offset_iter(),
        &normalized,
        Rc::clone(&underline_opens),
    );

    let mut b = Builder::new(underline_opens);
    b.run(fixer)?;
    let mut rt = b.finish();
    rt.normalize();
    Ok(rt)
}

/// Import plain text (literal) into a [`Content`] content — the literal-codec
/// sibling of [`from_markdown`]. Every character is content, never syntax:
/// `*hi*` is four literal chars, not emphasis, and nothing is escaped. Paired
/// with [`crate::export::to_plaintext`] as its exporter, it pins the literal
/// fixed point `to_plaintext(from_plaintext(s)) == s` for any `s` free of `\r`,
/// bidi controls, and the reserved island slot ([`ISLAND_SLOT`]) — the same
/// boundary cleanup [`from_markdown`] performs, so the fixed point holds for
/// clean plaintext and the content invariants hold by construction.
///
/// Line structure is **derived, not stored**: a lone `\n` between two non-empty
/// segments is a within-paragraph break ([`Line::continues`] `true`, lowered as
/// a backend hard break); a blank line (`\n\n`) is a paragraph boundary (its own
/// empty line resets `continues`). The text is stored verbatim, so the round
/// trip is byte-exact and idempotent regardless of how structure is later
/// re-derived.
pub fn from_plaintext(s: &str) -> Content {
    // Boundary cleanup so the content invariants hold: CRLF→LF (drop `\r`), strip
    // bidi controls, drop the reserved island slot. Clean plaintext passes
    // through untouched, so the literal fixed point holds for it.
    let text: String = s
        .chars()
        .filter(|&c| c != '\r' && c != ISLAND_SLOT && !crate::normalize::is_bidi_char(c))
        .collect();
    // One line per `\n`-separated segment. `continues` marks a within-paragraph
    // break — a lone `\n` joining two non-empty segments; any empty segment is a
    // paragraph boundary and resets it. A single streaming pass carries the prior
    // segment's non-emptiness, so line 0 is `false` (the flag starts `false`) and
    // no intermediate segment vector is allocated.
    let mut prev_nonempty = false;
    let lines = text
        .split('\n')
        .map(|seg| {
            let continues = prev_nonempty && !seg.is_empty();
            prev_nonempty = !seg.is_empty();
            Line {
                kind: LineKind::Para,
                containers: Vec::new(),
                continues,
            }
        })
        .collect();
    Content {
        text,
        lines,
        marks: Vec::new(),
        islands: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Content builder
// ---------------------------------------------------------------------------

/// A flat inline accumulator: `text` plus `marks` over local USV offsets, with
/// the content char-filtering baked in. One implementation serves both a prose
/// line's inline content (embedded in the [`Builder`], which layers the
/// line/block scaffolding on top) and a table cell's isolated content (offsets
/// `0..cell_len`) — the mark-building logic is written once, not copied per site.
#[derive(Default)]
struct Inline {
    /// The accumulated text (a whole content for the [`Builder`]; one cell's text
    /// for a table cell). USV length is tracked in [`Self::pos`].
    text: String,
    /// USV position = char count of [`Self::text`].
    pos: usize,
    marks: Vec<Mark>,
    /// `(kind, start)` for each mark opened but not yet closed.
    open: Vec<(MarkKind, usize)>,
}

impl Inline {
    /// Append inline text, stripping characters the content forbids: `\r` and a
    /// stray [`ISLAND_SLOT`] are dropped; a stray `\n` becomes a space (inline
    /// text carries no line boundary — real ones go through [`Self::push_raw`]).
    ///
    /// A literal [`ISLAND_SLOT`] (U+FFFC) in source markdown is dropped
    /// *silently and by design*: the slot char is the reserved island sentinel,
    /// so admitting a bare one would break the slot-count invariant. Such a char
    /// in prose is a paste/render artifact, never authored content, so its loss
    /// carries no signal — this is a fixed point, not lossy round-tripping.
    fn push_text(&mut self, s: &str) {
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

    /// Append one char verbatim (a line-boundary `\n`, an island slot), bypassing
    /// the [`Self::push_text`] filtering.
    fn push_raw(&mut self, c: char) {
        self.text.push(c);
        self.pos += 1;
    }

    /// Open a mark at the current position.
    fn open_mark(&mut self, kind: MarkKind) {
        self.open.push((kind, self.pos));
    }

    /// Close the innermost open mark (pulldown nests them well).
    fn close_mark(&mut self) {
        if let Some((kind, start)) = self.open.pop() {
            self.marks.push(Mark {
                start,
                end: self.pos,
                kind,
            });
        }
    }

    /// Append inline code text and record its [`MarkKind::Code`] mark over it.
    fn push_code(&mut self, s: &str) {
        let start = self.pos;
        self.push_text(s);
        self.marks.push(Mark {
            start,
            end: self.pos,
            kind: MarkKind::Code,
        });
    }
}

struct Builder {
    /// A `Tag::Strong` whose `range.start` is here was a `<u>` in source (the
    /// fixer recorded it); the [`Builder`] opens [`MarkKind::Underline`] for it
    /// instead of [`MarkKind::Strong`] — the carried form of the distinction the
    /// fixer would otherwise erase.
    underline_opens: UnderlineOpens,
    /// The content text + marks; the [`Builder`] adds line/block structure around
    /// it (a `\n` boundary is [`Inline::push_raw`], inline content is the mark
    /// machinery). A table cell reuses the same [`Inline`] in isolation.
    inline: Inline,
    lines: Vec<Line>,
    cur: Option<Line>, // the line currently open (kind + containers fixed at open)
    /// A block start records `(kind, continues)` the next inline content should
    /// open a fresh line with. Set at Paragraph/Heading/Item (tight lists emit no
    /// Paragraph wrapper, so Item must force a line) with `continues = false`; a
    /// hard break sets `continues = true`. Cleared when a block that owns its own
    /// lines (List/Quote/CodeBlock/Table) takes over.
    pending: Option<(LineKind, bool)>,
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
    /// Cells as canonical `{text, marks}` JSON (via `serial::cell_to_value`), so
    /// nothing downstream re-parses markdown to render a formatted cell.
    header: Vec<serde_json::Value>,
    rows: Vec<Vec<serde_json::Value>>,
    cur_row: Vec<serde_json::Value>,
    in_head: bool,
    /// The cell currently open (between `Tag::TableCell` start/end), building its
    /// inline text + marks with the same [`Inline`] machinery prose uses.
    cell: Option<Inline>,
    /// Open-image nesting inside the current cell. GFM permits inline images in
    /// cells, but a cell has no island slot to carry one; while `> 0` the image's
    /// alt flows into the cell as plain text (the degraded projection) and its
    /// url is dropped. Mirrors the top-level `image_depth` interception.
    img_depth: usize,
    /// Whether any cell dropped an image's url — the island is then minted
    /// [`Loss::Degraded`], not `Lossless`: the markdown/Typst projection carries
    /// the alt text but not the image.
    degraded: bool,
}

fn align_str(a: &pulldown_cmark::Alignment) -> &'static str {
    match a {
        pulldown_cmark::Alignment::None => "none",
        pulldown_cmark::Alignment::Left => "left",
        pulldown_cmark::Alignment::Center => "center",
        pulldown_cmark::Alignment::Right => "right",
    }
}

impl Builder {
    fn new(underline_opens: UnderlineOpens) -> Self {
        Builder {
            underline_opens,
            inline: Inline::default(),
            lines: Vec::new(),
            cur: None,
            pending: None,
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
            self.inline.push_raw('\n');
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
    /// content invariants forbid (stray `\r`, stray island slots; stray `\n`
    /// becomes a space — inline text should carry none).
    fn push_inline(&mut self, s: &str) {
        self.ensure_open(LineKind::Para);
        self.inline.push_text(s);
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
        self.inline.open_mark(kind);
    }

    fn close_mark(&mut self) {
        // Well-nested by pulldown: close the innermost open mark.
        self.inline.close_mark();
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
        let depth = self.containers.len() + self.inline.open.len();
        if depth > MAX_NESTING_DEPTH {
            return Err(ImportError::NestingTooDeep {
                depth,
                max: MAX_NESTING_DEPTH,
            });
        }
        Ok(())
    }

    /// [`MarkKind::Underline`] if the fixer converted a `<u>` open at `start`,
    /// else [`MarkKind::Strong`] — reads the classification the fixer carried,
    /// no source re-sniff.
    fn strong_kind(&self, start: usize) -> MarkKind {
        if self.underline_opens.borrow().contains(&start) {
            MarkKind::Underline
        } else {
            MarkKind::Strong
        }
    }

    fn run<'a, I>(&mut self, iter: I) -> Result<(), ImportError>
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

            // Table collection routes both structural events (head/row/cell) and
            // a cell's inline content (text/marks) to the accumulator, so each
            // cell is stored as canonical `{text, marks}` — no markdown re-parse
            // downstream.
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
                    self.inline.push_code(&t);
                }
                Event::Rule => self.open_line(LineKind::Rule, false),
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
                // math/footnotes/etc. produce no content.
                _ => {}
            }
        }
        Ok(())
    }

    fn start_tag<'a>(&mut self, tag: Tag<'a>, range: Range<usize>) -> Result<(), ImportError> {
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
                self.inline.push_raw(ISLAND_SLOT);
                self.table = Some(TableAcc {
                    aligns: aligns.iter().map(align_str).collect(),
                    header: Vec::new(),
                    rows: Vec::new(),
                    cur_row: Vec::new(),
                    in_head: false,
                    cell: None,
                    img_depth: 0,
                    degraded: false,
                });
            }
            Tag::Emphasis => {
                self.open_mark(MarkKind::Emph);
                self.check_depth()?;
            }
            Tag::Strong => {
                let kind = self.strong_kind(range.start);
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
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.close_mark()
            }
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
            // Code text is literal; still enforce content invariants.
            self.push_code_line(seg);
        }
    }

    fn push_code_line(&mut self, seg: &str) {
        for c in seg.chars() {
            match c {
                '\r' | '\n' => continue,
                ISLAND_SLOT => continue,
                other => self.inline.push_raw(other),
            }
        }
    }

    // ---- table ----

    /// The open table cell's inline accumulator, if one is open.
    fn cell_mut(&mut self) -> Option<&mut Inline> {
        self.table.as_mut()?.cell.as_mut()
    }

    /// Route one table event: structural events (head/row/cell boundaries) shape
    /// the accumulator; inline events (text/code/marks) build the open cell with
    /// the SAME [`Inline`] machinery prose uses — a cell is flat inline (no lines,
    /// no nested islands), so its marks are USV offsets into its own text.
    fn table_event(&mut self, event: &Event, range: &Range<usize>) {
        // An image open inside the current cell intercepts everything until it
        // closes: the alt text lands in the cell as plain text (marks flattened,
        // like the top-level image path), the url is dropped, and the island is
        // flagged degraded. A cell has no island slot to carry a real image.
        if self.table.as_ref().is_some_and(|a| a.img_depth > 0) {
            match event {
                Event::Start(Tag::Image { .. }) => {
                    if let Some(a) = self.table.as_mut() {
                        a.img_depth += 1;
                    }
                }
                Event::End(TagEnd::Image) => {
                    if let Some(a) = self.table.as_mut() {
                        a.img_depth -= 1;
                    }
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some(c) = self.cell_mut() {
                        c.push_text(t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(c) = self.cell_mut() {
                        c.push_text(" ");
                    }
                }
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(Tag::Image { .. }) => {
                if let Some(a) = self.table.as_mut() {
                    a.img_depth += 1;
                    a.degraded = true;
                }
            }
            Event::Start(Tag::TableHead) => {
                if let Some(a) = self.table.as_mut() {
                    a.in_head = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(a) = self.table.as_mut() {
                    a.header = std::mem::take(&mut a.cur_row);
                    a.in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(a) = self.table.as_mut() {
                    a.cur_row.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(a) = self.table.as_mut() {
                    if !a.in_head {
                        let row = std::mem::take(&mut a.cur_row);
                        a.rows.push(row);
                    }
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(a) = self.table.as_mut() {
                    a.cell = Some(Inline::default());
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(a) = self.table.as_mut() {
                    if let Some(mut cell) = a.cell.take() {
                        // Close any marks pulldown left open (malformed input).
                        while !cell.open.is_empty() {
                            cell.close_mark();
                        }
                        a.cur_row
                            .push(crate::serial::cell_to_value(&cell.text, &cell.marks));
                    }
                }
            }
            // Inline content of the open cell (pulldown already trimmed the cell's
            // surrounding whitespace; the fixer already stripped non-`<u>` HTML
            // and fixed `***`). A soft/hard break in a single-line cell is a space.
            Event::Text(t) => {
                if let Some(c) = self.cell_mut() {
                    c.push_text(t);
                }
            }
            Event::Code(t) => {
                if let Some(c) = self.cell_mut() {
                    c.push_code(t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(c) = self.cell_mut() {
                    c.push_text(" ");
                }
            }
            Event::Start(Tag::Emphasis) => {
                if let Some(c) = self.cell_mut() {
                    c.open_mark(MarkKind::Emph);
                }
            }
            Event::Start(Tag::Strong) => {
                let kind = self.strong_kind(range.start);
                if let Some(c) = self.cell_mut() {
                    c.open_mark(kind);
                }
            }
            Event::Start(Tag::Strikethrough) => {
                if let Some(c) = self.cell_mut() {
                    c.open_mark(MarkKind::Strike);
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let url = dest_url.to_string();
                if let Some(c) = self.cell_mut() {
                    c.open_mark(MarkKind::Link { url });
                }
            }
            Event::End(TagEnd::Emphasis)
            | Event::End(TagEnd::Strong)
            | Event::End(TagEnd::Strikethrough)
            | Event::End(TagEnd::Link) => {
                if let Some(c) = self.cell_mut() {
                    c.close_mark();
                }
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
            // Degraded when a cell dropped an inline image's url — the projection
            // then carries the alt text but not the image (not a fixed point).
            let loss = if acc.degraded {
                Loss::Degraded
            } else {
                Loss::Lossless
            };
            self.mint_island("table", props, loss);
        }
    }

    fn emit_image(&mut self) {
        self.ensure_open(LineKind::Para);
        self.inline.push_raw(ISLAND_SLOT);
        let props = json!({
            "url": self.image_url,
            "alt": self.image_alt.trim(),
        });
        self.mint_island("image", props, Loss::Lossless);
    }

    fn finish(mut self) -> Content {
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
        while !self.inline.open.is_empty() {
            self.close_mark();
        }
        Content {
            text: self.inline.text,
            lines: self.lines,
            marks: self.inline.marks,
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
// MarkdownFixer — the crate's single copy of the pulldown-cmark event fixups.
//
// Two jobs before events reach the builder: allowlist `<u>…</u>` as underline
// (rewrite to Strong start/end, detected by source peek) and strip all other
// raw HTML; and fix `***`-adjacency runs pulldown splits awkwardly.
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
    /// Shared with the [`Builder`]: each `<u>` open this fixer converts to
    /// `Tag::Strong` records its `range.start` here, so the builder recovers the
    /// underline without a second source-byte test (see [`UnderlineOpens`]).
    underline_opens: UnderlineOpens,
    buffer: Vec<(Event<'a>, Range<usize>)>,
    emph_depth: usize,
    strong_depth: usize,
}

impl<'a, I> MarkdownFixer<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    fn new(inner: I, source: &'a str, underline_opens: UnderlineOpens) -> Self {
        Self {
            inner: inner.peekable(),
            source,
            underline_opens,
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
                let next_is_star_text = if let Some((Event::Text(cow_str), _)) = self.buffer.last()
                {
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
                    // Carry the `<u>` classification to the builder keyed on the
                    // tag's start offset, so it need not re-sniff the source.
                    self.underline_opens.borrow_mut().insert(range.start);
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

    fn imp(md: &str) -> Content {
        let rt = from_markdown(md).unwrap();
        assert_eq!(rt.validate(), Ok(()), "invariants for {md:?}");
        rt
    }

    fn imp_plain(s: &str) -> Content {
        let rt = from_plaintext(s);
        assert_eq!(rt.validate(), Ok(()), "invariants for {s:?}");
        rt
    }

    /// Plaintext is literal: markdown delimiters are content, not syntax, and
    /// nothing is escaped or marked. The content is mark- and island-free.
    #[test]
    fn plaintext_is_literal_and_plain() {
        let rt = imp_plain("a *star* and _under_ #hash");
        assert_eq!(rt.text, "a *star* and _under_ #hash");
        assert!(rt.marks.is_empty());
        assert!(rt.islands.is_empty());
        assert!(rt.is_plain());
        assert!(rt.is_inline(), "one line with no formatting is also inline");
    }

    /// The literal fixed point: `to_plaintext(from_plaintext(s)) == s` for clean
    /// text, and re-import is idempotent.
    #[test]
    fn plaintext_round_trip_is_verbatim_and_idempotent() {
        for s in ["", "one line", "a\nb", "a\n\nb", "trailing\n", "*not bold*"] {
            let rt = imp_plain(s);
            assert_eq!(crate::export::to_plaintext(&rt), s, "verbatim for {s:?}");
            let rt2 = from_plaintext(&crate::export::to_plaintext(&rt));
            assert_eq!(rt2.text, rt.text, "idempotent for {s:?}");
            assert_eq!(rt2.lines, rt.lines, "idempotent structure for {s:?}");
        }
    }

    /// Lone `\n` between non-empty segments is a within-paragraph break
    /// (`continues: true`); a blank line resets it to a paragraph boundary.
    #[test]
    fn plaintext_derives_continues_from_line_structure() {
        let rt = imp_plain("a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert!(!rt.lines[0].continues);
        assert!(rt.lines[1].continues, "lone \\n is a within-paragraph break");

        let rt = imp_plain("a\n\nb");
        assert_eq!(rt.lines.len(), 3);
        assert!(!rt.lines[0].continues);
        assert!(!rt.lines[1].continues, "the blank line is a paragraph boundary");
        assert!(!rt.lines[2].continues, "text after a blank line starts a new block");
    }

    /// Boundary cleanup keeps the content invariants: CRLF collapses to LF, bidi
    /// controls and the reserved island slot are dropped. Clean text is
    /// unaffected, so the fixed point still holds for it.
    #[test]
    fn plaintext_strips_invariant_breakers() {
        let rt = imp_plain("a\r\nb");
        assert_eq!(rt.text, "a\nb", "CRLF collapses to LF");
        let rt = imp_plain(&format!("a{ISLAND_SLOT}b"));
        assert_eq!(rt.text, "ab", "the reserved island slot is dropped");
        assert_eq!(rt.islands.len(), 0);
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
        assert!(rt
            .marks
            .iter()
            .any(|m| m.kind == MarkKind::Underline && m.start == 2 && m.end == 3));
    }

    #[test]
    fn other_html_stripped() {
        let rt = imp("a <span>b</span> c");
        assert_eq!(rt.text, "a b c");
    }

    /// A `<u>`-lookalike must not be read as underline. The fixer's single
    /// `is_u_open_tag` classifier rejects `<ul>` (inner != "u"), so it is
    /// stripped like any other HTML — no underline, no strong. Regression for
    /// the old split where a separate 2-byte `<u` prefix peek would have
    /// mis-classified it had the fixer ever converted it.
    #[test]
    fn ul_lookalike_is_not_underline() {
        let rt = imp("x <ul>y</ul> z");
        assert_eq!(rt.text, "x y z");
        assert!(rt
            .marks
            .iter()
            .all(|m| m.kind != MarkKind::Underline && m.kind != MarkKind::Strong));
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
        assert!(rt.lines.iter().all(|l| l.kind
            == LineKind::Code {
                lang: Some("rust".into())
            }));
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
    fn thematic_break_is_rule_line() {
        for src in ["---", "***", "___"] {
            let md = format!("one\n\n{src}\n\ntwo");
            let rt = imp(&md);
            assert_eq!(rt.lines.len(), 3, "source: {src}");
            assert_eq!(rt.lines[0].kind, LineKind::Para);
            assert_eq!(rt.lines[1].kind, LineKind::Rule, "source: {src}");
            assert_eq!(rt.lines[2].kind, LineKind::Para);
            // The rule line carries no text of its own.
            assert_eq!(rt.text, "one\n\ntwo");
        }
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
    fn table_with_cell_image_degrades() {
        // GFM permits an inline image in a cell; the cell has no island slot to
        // carry it, so the alt text lands as plain cell text, the url is dropped,
        // and the island is Degraded (not the silent-Lossless lie).
        let rt = imp("| a | b |\n|---|---|\n| ![a cat](cat.png) | 2 |");
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].island_type, "table");
        assert_eq!(rt.islands[0].loss, Loss::Degraded);
        // The dropped image left no nested island; alt survived as cell text.
        assert_eq!(rt.islands[0].props["rows"][0][0]["text"], "a cat");
        // A table with no cell image stays Lossless (regression guard).
        let plain = imp("| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(plain.islands[0].loss, Loss::Lossless);
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
        // Distinct markdown, one content — but the content is a fixed point.
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
    fn import_and_editor_content_same_canonical_bytes() {
        // The freeze's central promise: equal content → equal bytes, whatever
        // the producer. Import of "a\n\n**b**" must byte-match a hand-built
        // editor content of the same content.
        let imported = imp("a\n\n**b**");
        let editor = Content {
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
        // content, unreachable via markdown import.)
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
        assert!(rt
            .marks
            .iter()
            .any(|m| m.start == 2 && m.end == 3 && m.kind == MarkKind::Strong));
    }
}
