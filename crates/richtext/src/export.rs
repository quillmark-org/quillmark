//! Markdown export: corpus → markdown, per island loss class.
//!
//! The projection back to markdown. Marks become syntax; islands are emitted per
//! their [`Loss`] class; identity ([`MarkKind::Anchor`]) marks are **omitted** —
//! they survive across edits via diff-rebase, not the projection (issue #831
//! § Codecs). Anchors carry no markdown encoding, so dropping them here is by
//! design, not loss.
//!
//! The contract phase 1 pins is the **corpus fixed point**: for a corpus `rt`
//! obtained from [`crate::import::from_markdown`],
//! `from_markdown(to_markdown(rt)) == rt` modulo island loss class — markdown
//! source is not canonical, but the corpus is, so round-trip is defined at the
//! corpus, not the string.
//!
//! ## Documented codec limits (degenerate, non-authorable corpora)
//!
//! The fixed point holds for corpora a well-behaved producer emits. Two
//! degenerate shapes markdown cannot represent do **not** round-trip, and are
//! recorded here rather than hidden (see `tests::known_hard_break_limits`):
//!
//! - **A mark spanning a hard break** — per-line rendering splits it into two
//!   per-line marks (they do not re-union across the `\n`).
//! - **An empty first line in a hard-break block** — markdown has no
//!   blank-then-forced-break syntax, so the leading empty line is dropped.
//!
//! Both arise only from adversarial delimiter/break placement, never from clean
//! markdown or a form editor. Hardening them is deferred (a phase-3 concern once
//! a live editor defines what it can even produce).

use crate::model::{Container, Island, LineKind, Loss, MarkKind, RichText, ISLAND_SLOT};

/// Render a corpus to markdown. Lossless/degraded islands emit their markdown;
/// unrepresentable islands emit a placeholder comment.
pub fn to_markdown(rt: &RichText) -> String {
    // Per-line char ranges, so global marks can be clipped to a line.
    let segments = line_segments(rt);
    let ctx = Ctx {
        rt,
        segments: &segments,
    };
    let mut out = String::new();
    emit_block(&ctx, 0..rt.lines.len(), 0, &mut out);
    // Collapse any trailing blank lines to a single newline.
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

struct Ctx<'a> {
    rt: &'a RichText,
    segments: &'a [Segment],
}

/// One line's char range `[start, end)` into the corpus.
struct Segment {
    start: usize,
    end: usize,
}

fn line_segments(rt: &RichText) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(rt.lines.len());
    let mut start = 0usize;
    let mut pos = 0usize;
    for c in rt.text.chars() {
        if c == '\n' {
            segs.push(Segment { start, end: pos });
            start = pos + 1;
        }
        pos += 1;
    }
    segs.push(Segment { start, end: pos });
    // Defensive: a malformed corpus (lines.len() != segments) still gets one
    // segment per line so indexing never panics.
    while segs.len() < rt.lines.len() {
        segs.push(Segment { start: pos, end: pos });
    }
    segs
}

/// Emit the lines in `range`, all sharing the container prefix of length
/// `depth`. Leaf lines (containers.len() == depth) render at this level;
/// deeper lines are grouped by their `depth`-th container and recursed into.
fn emit_block(ctx: &Ctx, range: std::ops::Range<usize>, depth: usize, out: &mut String) {
    let lines = &ctx.rt.lines;
    let mut i = range.start;
    let mut first_block = true;
    while i < range.end {
        let line = &lines[i];
        if line.containers.len() > depth {
            // A nested container starts here; gather its run and recurse.
            let key = &line.containers[depth];
            let mut j = i + 1;
            while j < range.end
                && lines[j].containers.len() > depth
                && &lines[j].containers[depth] == key
            {
                j += 1;
            }
            block_separator(out, first_block);
            emit_container(ctx, key, i..j, depth, out);
            first_block = false;
            i = j;
        } else {
            // A leaf block: this line (continues == false) plus every following
            // line that continues it (a hard-break run, or a code fence's lines).
            let mut j = i + 1;
            while j < range.end && lines[j].containers.len() == depth && lines[j].continues {
                j += 1;
            }
            block_separator(out, first_block);
            emit_leaf_block(ctx, i..j, out);
            first_block = false;
            i = j;
        }
    }
}

fn block_separator(out: &mut String, first_block: bool) {
    if !first_block {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
}

/// Emit a container run (a list item's block, or a quote's block) by prefixing
/// each produced line. The inner blocks are emitted into a scratch buffer, then
/// each of its lines is prefixed.
fn emit_container(
    ctx: &Ctx,
    key: &Container,
    range: std::ops::Range<usize>,
    depth: usize,
    out: &mut String,
) {
    let mut inner = String::new();
    emit_block(ctx, range, depth + 1, &mut inner);

    match key {
        Container::ListItem {
            ordered,
            start,
            ordinal,
        } => {
            let marker = if *ordered {
                format!("{}. ", start + ordinal)
            } else {
                "- ".to_string()
            };
            let indent = " ".repeat(marker.len());
            prefix_lines(&inner, &marker, &indent, out);
        }
        Container::Quote => {
            // `> ` on content lines, `>` on blank lines so paragraphs stay in
            // one quote on re-import.
            prefix_quote(&inner, out);
        }
    }
}

/// Prefix the first produced line with `first`, the rest with `cont`.
fn prefix_lines(inner: &str, first: &str, cont: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx == 0 {
            out.push_str(first);
            out.push_str(line);
        } else {
            out.push('\n');
            if line.is_empty() {
                // blank continuation line: no trailing indent
            } else {
                out.push_str(cont);
                out.push_str(line);
            }
        }
    }
}

fn prefix_quote(inner: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
    }
}

fn emit_code(ctx: &Ctx, range: std::ops::Range<usize>, lang: Option<&str>, out: &mut String) {
    // Choose a fence long enough to not collide with backtick runs in content.
    let mut max_ticks = 0usize;
    for i in range.clone() {
        let s = seg_str(ctx, i);
        let mut run = 0usize;
        for c in s.chars() {
            if c == '`' {
                run += 1;
                max_ticks = max_ticks.max(run);
            } else {
                run = 0;
            }
        }
    }
    let fence = "`".repeat(max_ticks.max(2) + 1);
    out.push_str(&fence);
    if let Some(l) = lang {
        out.push_str(l);
    }
    out.push('\n');
    for i in range {
        out.push_str(seg_str(ctx, i));
        out.push('\n');
    }
    out.push_str(&fence);
}

/// Emit one leaf block: the lines `range.start` (a block start) plus any
/// continuation lines. A paragraph block joins its lines with a markdown hard
/// break (`\` + newline); a code block renders one fence; a heading/island is a
/// single line.
fn emit_leaf_block(ctx: &Ctx, range: std::ops::Range<usize>, out: &mut String) {
    let first = &ctx.rt.lines[range.start];
    match &first.kind {
        LineKind::Code { lang } => emit_code(ctx, range, lang.as_deref(), out),
        LineKind::Island => {
            // A block island: the segment is a single slot. Resolve and emit.
            if let Some(isl) = slot_island(ctx, range.start) {
                emit_island(isl, out);
            }
        }
        LineKind::Heading { level } => {
            for _ in 0..*level {
                out.push('#');
            }
            out.push(' ');
            // Headings never carry continuations (import maps a hard break in a
            // heading to a space), so only the first line contributes.
            out.push_str(&render_inline(ctx, range.start, false));
        }
        LineKind::Para => {
            // Join continuation lines with a backslash hard break.
            let parts: Vec<String> = range
                .map(|i| render_inline(ctx, i, true))
                .collect();
            out.push_str(&parts.join("\\\n"));
        }
    }
}

fn seg_str<'a>(ctx: &'a Ctx, i: usize) -> &'a str {
    let seg = &ctx.segments[i];
    let bstart = crate::usv::char_to_byte(&ctx.rt.text, seg.start);
    let bend = crate::usv::char_to_byte(&ctx.rt.text, seg.end);
    &ctx.rt.text[bstart..bend]
}

/// The island backing the single slot on a block-island line `i`.
fn slot_island<'a>(ctx: &'a Ctx, i: usize) -> Option<&'a Island> {
    // Count slots before this line to find the island index.
    let seg = &ctx.segments[i];
    let before = ctx
        .rt
        .text
        .chars()
        .take(seg.start)
        .filter(|c| *c == ISLAND_SLOT)
        .count();
    ctx.rt.islands.get(before)
}

fn emit_island(isl: &Island, out: &mut String) {
    match (isl.island_type.as_str(), isl.loss) {
        ("table", _) => emit_table(isl, out),
        ("image", _) => emit_image(isl, out),
        (_, Loss::Unrepresentable) | (_, _) => {
            // Unknown island / unrepresentable: a comment placeholder that
            // survives round-trip as no corpus text (HTML comments are stripped
            // on re-import). The island itself is preserved via storage, not the
            // projection.
            out.push_str(&format!("<!-- island:{} -->", isl.island_type));
        }
    }
}

fn emit_table(isl: &Island, out: &mut String) {
    let header = isl.props.get("header").and_then(|v| v.as_array());
    let rows = isl.props.get("rows").and_then(|v| v.as_array());
    let aligns = isl.props.get("aligns").and_then(|v| v.as_array());
    let cols = header.map(|h| h.len()).unwrap_or(0);
    if cols == 0 {
        return;
    }
    // Cells are stored as raw markdown source slices (pipes already escaped as
    // `\|` at import), so emit them verbatim — re-escaping would double the
    // backslash and split the cell on re-import.
    let cell = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();

    // header row
    out.push_str("| ");
    if let Some(h) = header {
        out.push_str(
            &h.iter()
                .map(&cell)
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    out.push_str(" |\n|");
    for k in 0..cols {
        let a = aligns
            .and_then(|a| a.get(k))
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        out.push_str(match a {
            "left" => " :--- |",
            "center" => " :---: |",
            "right" => " ---: |",
            _ => " --- |",
        });
    }
    if let Some(rs) = rows {
        for row in rs {
            if let Some(r) = row.as_array() {
                out.push_str("\n| ");
                out.push_str(&r.iter().map(&cell).collect::<Vec<_>>().join(" | "));
                out.push_str(" |");
            }
        }
    }
}

fn emit_image(isl: &Island, out: &mut String) {
    let url = isl.props.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let alt = isl.props.get("alt").and_then(|v| v.as_str()).unwrap_or("");
    out.push_str(&format!("![{}]({})", alt, url));
}

// ---------------------------------------------------------------------------
// Inline rendering: marks -> syntax, over one line's char range.
// ---------------------------------------------------------------------------

fn render_inline(ctx: &Ctx, i: usize, escape_leading_block: bool) -> String {
    let seg = &ctx.segments[i];
    let line_start = seg.start;
    let text = seg_str(ctx, i);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();

    // Clip marks to this line, split into code (atomic) and formatting (nested).
    let mut code_ranges: Vec<(usize, usize)> = Vec::new();
    let mut fmt: Vec<(usize, usize, &MarkKind)> = Vec::new();
    let mut links: Vec<(usize, usize, &str)> = Vec::new();
    for m in &ctx.rt.marks {
        let s = m.start.saturating_sub(line_start);
        let e = m.end.saturating_sub(line_start);
        if m.end <= line_start || m.start >= line_start + n {
            continue; // outside this line
        }
        let s = s.min(n);
        let e = e.min(n);
        match &m.kind {
            MarkKind::Anchor { .. } => {} // omitted from the projection
            MarkKind::Code => code_ranges.push((s, e)),
            MarkKind::Link { url } => links.push((s, e, url)),
            _ => fmt.push((s, e, &m.kind)),
        }
    }

    // Leading ordered-list marker: a line whose text starts `<digits>.` or
    // `<digits>)` would re-import as an ordered list, so escape that punctuation.
    let escape_punct_at = if escape_leading_block {
        let lead_digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
        if lead_digits > 0
            && lead_digits < n
            && matches!(chars[lead_digits], '.' | ')')
        {
            Some(lead_digits)
        } else {
            None
        }
    } else {
        None
    };

    let mut out = String::new();
    let island_at = |pos_local: usize| -> Option<&Island> {
        let global = line_start + pos_local;
        let before = ctx
            .rt
            .text
            .chars()
            .take(global)
            .filter(|c| *c == ISLAND_SLOT)
            .count();
        ctx.rt.islands.get(before)
    };

    // Formatting delimiter open/close order at each boundary (proper nesting):
    // open longer marks first, close shorter marks first.
    let mut stack: Vec<(usize, &MarkKind)> = Vec::new(); // (end, kind)
    let mut pos = 0usize;
    while pos <= n {
        // Close marks ending here (innermost first).
        while let Some(&(end, kind)) = stack.last() {
            if end == pos {
                out.push_str(&delim_close(kind));
                stack.pop();
            } else {
                break;
            }
        }
        // Open formatting marks starting here, longest span (outer) first —
        // BEFORE any atomic run, so a formatting mark that begins at the same
        // position as inline code/link still wraps it (`**` + code at 457 →
        // `**`code`…**`, not a dropped strong).
        let mut opening: Vec<(usize, &MarkKind)> = fmt
            .iter()
            .filter(|(s, _, _)| *s == pos)
            .map(|(_, e, k)| (*e, *k))
            .collect();
        opening.sort_by(|a, b| b.0.cmp(&a.0));
        for (end, kind) in opening {
            out.push_str(&delim_open(kind));
            stack.push((end, kind));
        }
        // A link is emitted atomically as [text](url); its display text carries
        // plain content (nested marks in link text are out of scope for phase 1).
        if let Some(&(ls, le, url)) = links.iter().find(|(s, _, _)| *s == pos) {
            out.push('[');
            out.push_str(&escape_run(&chars[ls..le]));
            out.push_str("](");
            out.push_str(url);
            out.push(')');
            pos = le;
            continue;
        }
        // A code range is atomic.
        if let Some(&(cs, ce)) = code_ranges.iter().find(|(s, _)| *s == pos) {
            let content: String = chars[cs..ce].iter().collect();
            let ticks = longest_backtick_run(&content) + 1;
            let fence = "`".repeat(ticks.max(1));
            out.push_str(&fence);
            out.push_str(&content);
            out.push_str(&fence);
            pos = ce;
            continue;
        }
        if pos < n {
            let c = chars[pos];
            if c == ISLAND_SLOT {
                if let Some(isl) = island_at(pos) {
                    let mut tmp = String::new();
                    emit_island(isl, &mut tmp);
                    out.push_str(&tmp);
                }
            } else if Some(pos) == escape_punct_at {
                out.push('\\');
                out.push(c);
            } else {
                out.push_str(&escape_char(c, pos == 0 && escape_leading_block));
            }
        }
        pos += 1;
    }
    out
}

fn delim_open(kind: &MarkKind) -> String {
    match kind {
        MarkKind::Strong => "**".into(),
        // `*`, not `_`: `_` cannot do intraword emphasis (CommonMark flanking),
        // so `_a_你` re-imports as literal text; `*a*你` emphasizes correctly.
        MarkKind::Emph => "*".into(),
        MarkKind::Underline => "<u>".into(),
        MarkKind::Strike => "~~".into(),
        MarkKind::Unknown { .. } => String::new(),
        // Code/Link/Anchor handled elsewhere.
        _ => String::new(),
    }
}

fn delim_close(kind: &MarkKind) -> String {
    match kind {
        MarkKind::Strong => "**".into(),
        MarkKind::Emph => "*".into(),
        MarkKind::Underline => "</u>".into(),
        MarkKind::Strike => "~~".into(),
        _ => String::new(),
    }
}

fn escape_run(chars: &[char]) -> String {
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        s.push_str(&escape_char(*c, i == 0));
    }
    s
}

/// Escape a char so it re-imports as literal text. `leading` also escapes
/// block-starter chars that would otherwise open a heading/list/quote.
fn escape_char(c: char, leading: bool) -> String {
    match c {
        '\\' => "\\\\".into(),
        '*' => "\\*".into(),
        '_' => "\\_".into(),
        '`' => "\\`".into(),
        '[' => "\\[".into(),
        ']' => "\\]".into(),
        '<' => "\\<".into(),
        '~' => "\\~".into(),
        '#' if leading => "\\#".into(),
        '>' if leading => "\\>".into(),
        '-' if leading => "\\-".into(),
        '+' if leading => "\\+".into(),
        other => other.to_string(),
    }
}

fn longest_backtick_run(s: &str) -> usize {
    let mut max = 0;
    let mut run = 0;
    for c in s.chars() {
        if c == '`' {
            run += 1;
            max = max.max(run);
        } else {
            run = 0;
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::from_markdown;
    use crate::model::Mark;

    /// The phase-1 contract: export∘import is the identity on the corpus (modulo
    /// island loss class, which our test islands don't trigger).
    fn round_trips(md: &str) {
        let rt = from_markdown(md).unwrap();
        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap();
        assert_eq!(
            rt, rt2,
            "corpus not a fixed point.\n  in:  {md:?}\n  mid: {md2:?}"
        );
    }

    #[test]
    fn paragraph() {
        round_trips("Hello world");
    }

    #[test]
    fn two_paragraphs() {
        round_trips("one\n\ntwo");
    }

    #[test]
    fn marks() {
        round_trips("a **b** _c_ ~~d~~ <u>e</u>");
    }

    #[test]
    fn nested_marks() {
        round_trips("**bold _and italic_**");
    }

    #[test]
    fn heading() {
        round_trips("## Title here");
    }

    #[test]
    fn inline_code() {
        round_trips("run `cargo test` now");
    }

    #[test]
    fn code_block() {
        round_trips("```rust\nfn a() {}\nfn b() {}\n```");
    }

    #[test]
    fn bullet_list() {
        round_trips("- a\n- b\n- c");
    }

    #[test]
    fn ordered_list() {
        round_trips("3. a\n4. b");
    }

    #[test]
    fn multi_paragraph_item() {
        round_trips("- first\n\n  second");
    }

    #[test]
    fn blockquote() {
        round_trips("> quoted text");
    }

    #[test]
    fn link() {
        round_trips("see [our site](https://example.com) now");
    }

    #[test]
    fn table() {
        round_trips("| a | b |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn image() {
        round_trips("see ![a cat](cat.png) here");
    }

    #[test]
    fn literal_asterisks_escaped() {
        round_trips("2 * 3 = 6 and a_b_c");
    }

    #[test]
    fn hard_break_round_trips() {
        round_trips("line one\\\nline two");
    }

    #[test]
    fn hard_break_in_list_item() {
        round_trips("- one\\\ntwo\n- three");
    }

    #[test]
    fn leading_ordered_marker_escaped() {
        // Corpus prose that begins `N.` must not re-import as an ordered list.
        let mut rt = from_markdown("x").unwrap();
        rt.text = "1. not a list".into();
        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap();
        assert_eq!(back.lines[0].kind, LineKind::Para);
        assert!(back.lines[0].containers.is_empty());
        assert_eq!(back, rt);
    }

    #[test]
    fn table_with_escaped_pipe_round_trips() {
        round_trips("| a \\| b | c |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn known_hard_break_limits() {
        // Recorded, not hidden: a mark spanning a hard break splits per line.
        let rt = from_markdown("**one\\\ntwo**").unwrap();
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
        // The spanning strong becomes two per-line strongs on round-trip.
        assert!(
            rt != rt2,
            "if this ever round-trips, promote it out of the known-limits list"
        );
        assert_eq!(rt2.marks.len(), 2, "mark split across the hard break");
    }

    #[test]
    fn anchor_marks_omitted_but_text_survives() {
        let mut rt = from_markdown("comment target here").unwrap();
        rt.marks.push(Mark {
            start: 8,
            end: 14,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        rt.normalize();
        let md = to_markdown(&rt);
        // No anchor syntax in the projection, but the text round-trips.
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "comment target here");
        assert!(!md.contains("c1"));
    }
}
