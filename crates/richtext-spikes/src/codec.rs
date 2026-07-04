//! Markdown ⇄ corpus codecs (the cold-import path and its inverse projection).
//! The corpus is canonical; markdown is a projection, so the honest round-trip
//! property is **corpus-stable-through-the-projection**: `import(export(rt))
//! == rt` for the round-trippable subset, and islands degrade per their loss
//! class. A spike subset — paragraphs, ATX headings, bullet/ordered items,
//! blockquotes, the `strong`/`emph`/`strike`/`code`/`link` marks, and tables as
//! degraded islands — enough to test normalization and diff-rebase, not the
//! full CommonMark surface.

use crate::model::*;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Cold markdown import → corpus. One `Line` per block; soft breaks inside a
/// paragraph collapse to a space (matching the Typst converter). Marks carry
/// canonical char ranges.
pub fn import_markdown(md: &str) -> RichText {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let mut b = Builder::default();
    let mut table_depth = 0usize;

    for ev in Parser::new_ext(md, opts) {
        // A table has no honest inline encoding: swallow its events, emit one
        // island slot on its own line. `loss: degraded` — the grid survives as
        // structured props, the markdown projection does not.
        if table_depth > 0 {
            match ev {
                Event::Start(Tag::Table(_)) => table_depth += 1,
                Event::End(TagEnd::Table) => {
                    table_depth -= 1;
                    if table_depth == 0 {
                        b.emit_island_line();
                    }
                }
                _ => {}
            }
            continue;
        }

        match ev {
            Event::Start(Tag::Table(_)) => table_depth += 1,
            Event::Start(Tag::Paragraph) => b.open_line(LineKind::Para),
            Event::End(TagEnd::Paragraph) => b.close_line(),
            Event::Start(Tag::Heading { level, .. }) => {
                b.open_line(LineKind::Heading { level: level as u8 })
            }
            Event::End(TagEnd::Heading(_)) => b.close_line(),
            Event::Start(Tag::List(start)) => b.push_container(Container::ListItem {
                ordered: start.is_some(),
            }),
            Event::End(TagEnd::List(_)) => b.pop_container(),
            Event::Start(Tag::Item) => {}
            // A tight list item emits `Text` directly (no `Paragraph`), so the
            // line is auto-opened by `push_text`; close it here. A loose item's
            // `Paragraph` already closed it, so this is a no-op.
            Event::End(TagEnd::Item) => b.close_line(),
            Event::Start(Tag::BlockQuote(_)) => b.push_container(Container::Quote),
            Event::End(TagEnd::BlockQuote(_)) => b.pop_container(),
            Event::Start(Tag::Emphasis) => b.open_mark(MarkKind::Emph),
            Event::End(TagEnd::Emphasis) => b.close_mark(MarkKind::Emph),
            Event::Start(Tag::Strong) => b.open_mark(MarkKind::Strong),
            Event::End(TagEnd::Strong) => b.close_mark(MarkKind::Strong),
            Event::Start(Tag::Strikethrough) => b.open_mark(MarkKind::Strike),
            Event::End(TagEnd::Strikethrough) => b.close_mark(MarkKind::Strike),
            Event::Start(Tag::Link { dest_url, .. }) => {
                b.open_mark(MarkKind::Link { url: dest_url.to_string() })
            }
            Event::End(TagEnd::Link) => b.close_link(),
            Event::Text(t) => b.push_text(&t),
            Event::Code(t) => {
                let start = b.pos;
                b.push_text(&t);
                b.marks.push(Mark {
                    range: CharRange::new(start, b.pos),
                    kind: MarkKind::Code,
                });
            }
            Event::SoftBreak => b.push_text(" "),
            Event::HardBreak => b.push_text(" "),
            _ => {}
        }
    }

    let mut rt = b.finish();
    rt.normalize_marks();
    rt
}

/// Corpus → markdown projection. Inverse of [`import_markdown`] for the
/// round-trippable subset; islands render per loss class (degraded → an HTML
/// comment placeholder that import discards, so the island survives via the
/// diff-rebase path, not the text).
pub fn export_markdown(rt: &RichText) -> String {
    let chars: Vec<char> = rt.text.chars().collect();
    let line_bounds = line_char_ranges(&rt.text);
    let mut out = String::new();

    for (i, ((lo, hi), line)) in line_bounds.iter().zip(&rt.lines).enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        // Container prefix (single level; the spike does not nest projections).
        for c in &line.containers {
            match c {
                Container::ListItem { ordered } => {
                    out.push_str(if *ordered { "1. " } else { "- " })
                }
                Container::Quote => out.push_str("> "),
            }
        }
        match &line.kind {
            LineKind::Heading { level } => {
                for _ in 0..*level {
                    out.push('#');
                }
                out.push(' ');
            }
            LineKind::Island => {
                // The slot char sits in [lo, hi); resolve its island and render
                // the loss-class placeholder.
                out.push_str("<!--island-->");
                continue;
            }
            LineKind::Para | LineKind::Code { .. } => {}
        }
        render_run(&chars, *lo, *hi, &rt.marks, &mut out);
    }
    out
}

/// Render the char span `[lo, hi)` with its overlapping marks as markdown
/// syntax. Anchor marks are **omitted** (they survive via diff-rebase, never
/// the projection — the plan's key move). Handles the spike's non-overlapping
/// common case; nested marks render outermost-first.
fn render_run(chars: &[char], lo: usize, hi: usize, marks: &[Mark], out: &mut String) {
    // Collect visible (non-anchor) marks touching this line, longest first so
    // enclosing marks wrap inner ones.
    let mut active: Vec<&Mark> = marks
        .iter()
        .filter(|m| !matches!(m.kind, MarkKind::Anchor { .. }))
        .filter(|m| m.range.start < hi && m.range.end > lo && !m.range.is_empty())
        .collect();
    active.sort_by_key(|m| (m.range.start, std::cmp::Reverse(m.range.end)));

    let mut pos = lo;
    let mut i = 0;
    render_seq(chars, &mut pos, hi, &active, &mut i, out);
}

fn render_seq(
    chars: &[char],
    pos: &mut usize,
    end: usize,
    active: &[&Mark],
    i: &mut usize,
    out: &mut String,
) {
    while *pos < end {
        if *i < active.len() && active[*i].range.start <= *pos {
            let m = active[*i];
            *i += 1;
            let (open, close) = mark_delims(&m.kind);
            out.push_str(open);
            let inner_end = m.range.end.min(end);
            render_seq(chars, pos, inner_end, active, i, out);
            out.push_str(close);
        } else {
            let next_mark = active.get(*i).map(|m| m.range.start).unwrap_or(end);
            let stop = next_mark.min(end);
            for &c in &chars[*pos..stop] {
                out.push(c);
            }
            *pos = stop;
        }
    }
}

fn mark_delims(kind: &MarkKind) -> (&'static str, &'static str) {
    match kind {
        MarkKind::Strong => ("**", "**"),
        MarkKind::Emph => ("_", "_"),
        MarkKind::Underline => ("<u>", "</u>"),
        MarkKind::Strike => ("~~", "~~"),
        MarkKind::Code => ("`", "`"),
        MarkKind::Link { .. } => ("[", "]"), // url appended by caller path; spike keeps it simple
        MarkKind::Anchor { .. } => ("", ""),
    }
}

/// The lowering pdfform uses: drop island slots, keep the corpus text. Marks
/// and line structure are discarded — a form field is plaintext.
pub fn to_plaintext(rt: &RichText) -> String {
    rt.text.chars().filter(|c| *c != ISLAND_SLOT).collect()
}

/// Char ranges `[start, end)` of each `\n`-separated line in `text`.
fn line_char_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut pos = 0usize;
    for c in text.chars() {
        if c == '\n' {
            ranges.push((start, pos));
            start = pos + 1;
        }
        pos += 1;
    }
    ranges.push((start, pos));
    ranges
}

#[derive(Default)]
struct Builder {
    text: String,
    pos: usize,
    lines: Vec<Line>,
    marks: Vec<Mark>,
    islands: Vec<Island>,
    containers: Vec<Container>,
    open_line: Option<LineKind>,
    open_marks: Vec<(MarkKind, usize)>,
    line_started: bool,
    island_counter: usize,
}

impl Builder {
    fn push_text(&mut self, s: &str) {
        // Tight list items deliver text with no enclosing `Paragraph`; open a
        // Para line lazily so every run of text belongs to some line.
        if self.open_line.is_none() {
            self.open_line(LineKind::Para);
        }
        self.text.push_str(s);
        self.pos += s.chars().count();
    }

    fn open_line(&mut self, kind: LineKind) {
        if self.line_started {
            self.text.push('\n');
            self.pos += 1;
        }
        self.open_line = Some(kind);
        self.line_started = true;
    }

    fn close_line(&mut self) {
        if let Some(kind) = self.open_line.take() {
            self.lines.push(Line {
                kind,
                containers: self.containers.clone(),
            });
        }
    }

    fn emit_island_line(&mut self) {
        if self.line_started {
            self.text.push('\n');
            self.pos += 1;
        }
        self.line_started = true;
        self.text.push(ISLAND_SLOT);
        self.pos += 1;
        let id = format!("isl_{}", self.island_counter);
        self.island_counter += 1;
        self.islands.push(Island {
            id,
            island_type: "table".into(),
            props: serde_json::json!({ "note": "spike table island" }),
            loss: Loss::Degraded,
        });
        self.lines.push(Line {
            kind: LineKind::Island,
            containers: self.containers.clone(),
        });
    }

    fn push_container(&mut self, c: Container) {
        self.containers.push(c);
    }
    fn pop_container(&mut self) {
        self.containers.pop();
    }

    fn open_mark(&mut self, kind: MarkKind) {
        self.open_marks.push((kind, self.pos));
    }

    fn close_mark(&mut self, kind: MarkKind) {
        if let Some(idx) = self.open_marks.iter().rposition(|(k, _)| *k == kind) {
            let (k, start) = self.open_marks.remove(idx);
            self.marks.push(Mark {
                range: CharRange::new(start, self.pos),
                kind: k,
            });
        }
    }

    /// Links carry a url captured at open; close by matching the most recent
    /// open Link regardless of url.
    fn close_link(&mut self) {
        if let Some(idx) = self
            .open_marks
            .iter()
            .rposition(|(k, _)| matches!(k, MarkKind::Link { .. }))
        {
            let (k, start) = self.open_marks.remove(idx);
            self.marks.push(Mark {
                range: CharRange::new(start, self.pos),
                kind: k,
            });
        }
    }

    fn finish(mut self) -> RichText {
        self.close_line();
        RichText {
            text: self.text,
            lines: self.lines,
            marks: self.marks,
            islands: self.islands,
        }
    }
}
