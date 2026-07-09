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
        segs.push(Segment {
            start: pos,
            end: pos,
        });
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
            let mut inline = render_inline(ctx, range.start, false);
            // A trailing `#` run reads as an ATX closing sequence on re-import
            // (`# a #` → heading text "a", dropping the `#`). Escape the last `#`
            // so the run no longer reaches end-of-line as a bare hash sequence —
            // one escaped hash defeats the whole closer, and `\#` re-imports as a
            // literal `#`.
            if inline.ends_with('#') {
                inline.pop();
                inline.push_str("\\#");
            }
            out.push_str(&inline);
        }
        LineKind::Para => {
            // Join continuation lines with a backslash hard break.
            let parts: Vec<String> = range.map(|i| render_inline(ctx, i, true)).collect();
            out.push_str(&parts.join("\\\n"));
        }
        LineKind::Rule => out.push_str("---"),
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
    // Cells are canonical `{text, marks}`; reconstruct each cell's markdown from
    // its structure (the prose mark→syntax rendering, plus `|`→`\|`), so nothing
    // re-parses markdown and `import(export(table))` is a fixed point.
    // header row
    out.push_str("| ");
    if let Some(h) = header {
        out.push_str(&h.iter().map(render_cell_md).collect::<Vec<_>>().join(" | "));
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
                out.push_str(&r.iter().map(render_cell_md).collect::<Vec<_>>().join(" | "));
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
        if lead_digits > 0 && lead_digits < n && matches!(chars[lead_digits], '.' | ')') {
            Some(lead_digits)
        } else {
            None
        }
    } else {
        None
    };

    render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        escape_punct_at,
        escape_leading_block,
        false, // prose text does not escape `|`
        |pos_local| {
            let global = line_start + pos_local;
            let before = ctx
                .rt
                .text
                .chars()
                .take(global)
                .filter(|c| *c == ISLAND_SLOT)
                .count();
            ctx.rt.islands.get(before).map(|isl| {
                let mut tmp = String::new();
                emit_island(isl, &mut tmp);
                tmp
            })
        },
    )
}

/// Render marks over a standalone char slice to markdown: the projection's mark
/// boundary sweep, shared by prose lines and table cells. `code_ranges`/`fmt`/
/// `links` are the marks clipped to `chars` (local offsets); `escape_pipe` adds
/// `|`→`\|` for cells; `island_markup_at` renders an island slot (prose) or
/// yields `None` (cells carry no slot).
///
/// The model permits free (Peritext-style) overlap — an editor's `apply_mark_ops`
/// can produce `strong[0,4)` + `strike[2,6)` — but markdown syntax nests. The
/// sweep closes every mark ending at a boundary and reopens the deeper survivors,
/// so a partial overlap lowers to balanced markdown (`**ab~~cd~~**~~ef~~`), which
/// re-imports to the same corpus for marks with *distinct* delimiters. Two
/// preconditions the sweep can't express in reopened markdown are clipped away
/// first:
///
/// - **Atomic spans** (`code`/`link`) can't carry a partial wrap, and the sweep's
///   cursor jumps their interior — a wrap edge hiding inside would be missed and
///   left unbalanced. [`clip_fmt_to_atomic`] pulls such edges to the span's
///   boundary (the #846 shape, in markdown).
/// - **`*`/`**` (emph/strong) share a delimiter character**, so a reopened
///   `*` abutting a `**` merges into an ambiguous `***` run that CommonMark
///   re-segments wrong — this overlap is *unrepresentable*, so
///   [`clip_asterisk_overlap`] nests the two by truncation (a documented codec
///   limit: `strong`+`emph` overlap keeps the text but loses the crossing tail).
#[allow(clippy::too_many_arguments)]
fn render_marked_core(
    chars: &[char],
    code_ranges: &[(usize, usize)],
    fmt: &[(usize, usize, &MarkKind)],
    links: &[(usize, usize, &str)],
    escape_punct_at: Option<usize>,
    escape_leading_block: bool,
    escape_pipe: bool,
    island_markup_at: impl Fn(usize) -> Option<String>,
) -> String {
    let n = chars.len();
    let mut out = String::new();

    // Clip the wrapping marks so the sweep only ever sees a representable shape.
    let mut fmt: Vec<(usize, usize, &MarkKind)> = fmt.to_vec();
    let mut atomics: Vec<(usize, usize)> = code_ranges.to_vec();
    atomics.extend(links.iter().map(|(s, e, _)| (*s, *e)));
    clip_fmt_to_atomic(&mut fmt, &atomics);
    clip_asterisk_overlap(&mut fmt);

    // Indices into `fmt` for the marks currently open, outermost first. Storing
    // the index (not `(end, kind)`) keeps each open mark's identity, so a
    // reopened mark re-emits its OWN delimiter.
    let mut stack: Vec<usize> = Vec::new();
    let mut pos = 0usize;
    while pos <= n {
        // Close every mark ending at `pos` (innermost first) and reopen the
        // deeper survivors that do not — free overlap → proper nesting.
        if let Some(idx) = stack.iter().position(|&fi| fmt[fi].1 == pos) {
            let mut reopen: Vec<usize> = Vec::new();
            while stack.len() > idx {
                let fi = stack.pop().unwrap();
                out.push_str(&delim_close(fmt[fi].2));
                if fmt[fi].1 != pos {
                    reopen.push(fi);
                }
            }
            for fi in reopen.into_iter().rev() {
                out.push_str(&delim_open(fmt[fi].2));
                stack.push(fi);
            }
        }
        // Open formatting marks starting here, longest span (outer) first —
        // BEFORE any atomic run, so a formatting mark that begins at the same
        // position as inline code/link still wraps it (`**` + code → `**`code`…**`,
        // not a dropped strong).
        let mut opening: Vec<usize> = (0..fmt.len()).filter(|&fi| fmt[fi].0 == pos).collect();
        opening.sort_by(|&a, &b| fmt[b].1.cmp(&fmt[a].1));
        for fi in opening {
            out.push_str(&delim_open(fmt[fi].2));
            stack.push(fi);
        }
        // A link is emitted atomically as [text](url); its display text carries
        // plain content (nested marks in link text are out of scope for phase 1).
        if let Some(&(ls, le, url)) = links.iter().find(|(s, _, _)| *s == pos) {
            out.push('[');
            out.push_str(&escape_run(&chars[ls..le], escape_pipe));
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
                if let Some(markup) = island_markup_at(pos) {
                    out.push_str(&markup);
                }
            } else if Some(pos) == escape_punct_at {
                out.push('\\');
                out.push(c);
            } else {
                out.push_str(&escape_char(
                    c,
                    pos == 0 && escape_leading_block,
                    escape_pipe,
                ));
            }
        }
        pos += 1;
    }
    // Drain any mark still open at end of sweep. Clipping keeps every wrap `end`
    // reachable, so this normally drains nothing; it is the final close guard.
    while let Some(fi) = stack.pop() {
        out.push_str(&delim_close(fmt[fi].2));
    }
    out
}

/// Clip wrapping marks so none crosses the interior of an atomic span (`code` or
/// a `link`'s text). A wrap edge landing strictly inside a `[cs, ce)` span is
/// pulled to that span's boundary (`start`→`ce`, `end`→`cs`); a wrap swallowed
/// whole collapses and drops. An atomic span can't carry partial styling, and
/// the sweep's cursor jumps its interior start→end, so a wrap `end` hiding inside
/// would be missed and left unbalanced (the #846 shape). A wrap that strictly
/// *contains* a span keeps both edges, so the span still nests inside it.
fn clip_fmt_to_atomic(fmt: &mut Vec<(usize, usize, &MarkKind)>, atomics: &[(usize, usize)]) {
    for &(cs, ce) in atomics {
        for m in fmt.iter_mut() {
            if cs < m.0 && m.0 < ce {
                m.0 = ce;
            }
            if cs < m.1 && m.1 < ce {
                m.1 = cs;
            }
        }
    }
    fmt.retain(|m| m.0 < m.1);
}

/// Nest `strong`/`emph` marks that partially overlap, by truncating the
/// later-opening one to its enclosing sibling's end. Both render as runs of the
/// same character (`**`/`*`), so a reopened `*` abutting a `**` would merge into
/// an ambiguous `***` — this overlap is unrepresentable in CommonMark. Truncation
/// keeps the text and the nested portion of both marks, dropping only the
/// crossing tail (a documented codec limit). Marks with distinct delimiters
/// (`strike`, `underline`, `link`) are left to the sweep's close-and-reopen,
/// which round-trips them exactly. No-op when the marks already nest.
fn clip_asterisk_overlap(fmt: &mut [(usize, usize, &MarkKind)]) {
    let is_ast = |k: &MarkKind| matches!(k, MarkKind::Strong | MarkKind::Emph);
    // Asterisk-family marks, outermost first (start asc, then longer span first).
    let mut idx: Vec<usize> = (0..fmt.len()).filter(|&i| is_ast(fmt[i].2)).collect();
    idx.sort_by(|&a, &b| fmt[a].0.cmp(&fmt[b].0).then(fmt[b].1.cmp(&fmt[a].1)));
    // Ends of the enclosing ancestors still open at the current mark's start.
    let mut open_ends: Vec<usize> = Vec::new();
    for &i in &idx {
        let (s, mut e, _) = fmt[i];
        while open_ends.last().is_some_and(|&end| end <= s) {
            open_ends.pop();
        }
        if let Some(&parent_end) = open_ends.last() {
            if parent_end < e {
                e = parent_end;
                fmt[i].1 = e;
            }
        }
        open_ends.push(e);
    }
}

/// Reconstruct a table cell's markdown from its `{text, marks}`: the same mark
/// sweep as prose (`render_marked_core`) with `|`→`\|` escaping so the cell
/// survives re-import through `pulldown`'s pipe splitting. A cell is flat inline
/// — no islands, no leading-block escape — so `import(export(table))` is a fixed
/// point.
fn render_cell_md(v: &serde_json::Value) -> String {
    let (text, marks) = crate::serial::parse_cell(v);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut code_ranges: Vec<(usize, usize)> = Vec::new();
    let mut fmt: Vec<(usize, usize, &MarkKind)> = Vec::new();
    let mut links: Vec<(usize, usize, &str)> = Vec::new();
    for m in &marks {
        if m.start >= n {
            continue;
        }
        let s = m.start;
        let e = m.end.min(n);
        if s >= e {
            continue;
        }
        match &m.kind {
            MarkKind::Anchor { .. } => {}
            MarkKind::Code => code_ranges.push((s, e)),
            MarkKind::Link { url } => links.push((s, e, url)),
            _ => fmt.push((s, e, &m.kind)),
        }
    }
    render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        None,
        false,
        true,
        |_| None,
    )
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

fn escape_run(chars: &[char], escape_pipe: bool) -> String {
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        s.push_str(&escape_char(*c, i == 0, escape_pipe));
    }
    s
}

/// Escape a char so it re-imports as literal text. `leading` also escapes
/// block-starter chars that would otherwise open a heading/list/quote;
/// `escape_pipe` adds `|`→`\|`, so a table cell survives `pulldown`'s pipe split.
fn escape_char(c: char, leading: bool, escape_pipe: bool) -> String {
    match c {
        '\\' => "\\\\".into(),
        '*' => "\\*".into(),
        '_' => "\\_".into(),
        '`' => "\\`".into(),
        '[' => "\\[".into(),
        ']' => "\\]".into(),
        '<' => "\\<".into(),
        '~' => "\\~".into(),
        // `&` starts a CommonMark entity/numeric reference (`&amp;`, `&#38;`),
        // decoded on re-import — an unescaped `&` in `&word;`-shaped text would
        // silently collapse to the entity's character. Always escaped (a bare `&`
        // is harmless, but detecting "would form an entity" is not worth the
        // fragility); `\&` re-imports as a literal `&`.
        '&' => "\\&".into(),
        '|' if escape_pipe => "\\|".into(),
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
    use crate::model::{Line, Mark};

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

    /// A single-paragraph corpus over `text` with hand-placed `marks` — the
    /// free-overlap shapes an editor's `apply_mark_ops` produces but markdown
    /// import never does. Normalized + validated before use.
    fn marked(text: &str, marks: Vec<Mark>) -> RichText {
        let mut rt = RichText {
            text: text.to_string(),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: vec![],
                continues: false,
            }],
            marks,
            islands: vec![],
        };
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()), "corpus invariants");
        rt
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
    fn thematic_break() {
        round_trips("one\n\n***\n\ntwo");
    }

    #[test]
    fn thematic_break_canonicalizes_to_dashes() {
        // `***`/`___` and `---` all import to the same `Rule` line, so export
        // re-emits the canonical `---` whatever the source delimiter was.
        for src in ["***", "___", "- - -"] {
            let rt = from_markdown(&format!("one\n\n{src}\n\ntwo")).unwrap();
            let md = to_markdown(&rt);
            assert!(md.contains("\n\n---\n\n"), "source: {src}, got: {md:?}");
        }
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
    fn table_with_formatted_cells_round_trips() {
        // Option A: cells carry {text, marks}; export reconstructs their markdown
        // from structure, so the corpus is a fixed point across formatted cells.
        round_trips("| Name | Note |\n| --- | --- |\n| **bold** | _italic_ |");
        round_trips("| A |\n| --- |\n| **b** and _i_ `c` [d](https://e.com) ~~e~~ |");
        round_trips("| A |\n| --- |\n| <u>under</u> |");
        // A literal pipe inside a cell survives via `\|` re-escaping on export.
        round_trips("| A |\n| --- |\n| a \\| b |");
    }

    #[test]
    fn formatted_cell_marks_are_structured_not_reparsed() {
        // The cell stores marks, not a markdown slice: a strong cell's island
        // props carry a `strong` mark over the cell-local range, and export
        // renders it back to `**bold**` from that structure.
        let rt = from_markdown("| H |\n| --- |\n| **bold** |").unwrap();
        let cell = &rt.islands[0].props["rows"][0][0];
        assert_eq!(cell["text"], "bold");
        assert_eq!(cell["marks"][0]["type"], "strong");
        assert_eq!(cell["marks"][0]["start"], 0);
        assert_eq!(cell["marks"][0]["end"], 4);
        assert!(to_markdown(&rt).contains("**bold**"));
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

    // ---------------------------------------------------------------------
    // Issue #848: markdown export fixed-point violations.
    // ---------------------------------------------------------------------

    /// The exact #848 repro: `strong[0,4)` + `emph[2,6)` over "abcdef" used to
    /// export as `**ab*cdef*`, which re-imports as LITERAL `**abcdef` text — a
    /// silent content change. It must now export balanced markdown that preserves
    /// the text. The two marks share the `*` delimiter character, so their
    /// overlap is unrepresentable in CommonMark (a reopened `*` abutting `**`
    /// merges into `***`); the export nests them by truncation, a documented
    /// codec limit — text intact, the crossing tail of the inner mark dropped.
    #[test]
    fn overlapping_asterisk_marks_stay_text_safe() {
        let rt = marked(
            "abcdef",
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 2,
                    end: 6,
                    kind: MarkKind::Emph,
                },
            ],
        );
        let md = to_markdown(&rt);
        assert_eq!(md, "**ab*cd***ef\n", "balanced, no literal `**` leak");
        let rt2 = from_markdown(&md).unwrap();
        // Text is preserved exactly — the corruption the issue reported is gone.
        assert_eq!(rt2.text, "abcdef");
        // Documented limit: same-delimiter overlap degrades to its nested subset.
        assert_eq!(
            rt2.marks,
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong
                },
                Mark {
                    start: 2,
                    end: 4,
                    kind: MarkKind::Emph
                },
            ]
        );
    }

    /// Overlap between marks with *distinct* delimiters round-trips exactly:
    /// the close-and-reopen sweep lowers `strong[0,4)` + `strike[2,6)` to
    /// `**ab~~cd~~**~~ef~~`, which re-imports to the same overlapping corpus.
    #[test]
    fn overlapping_distinct_delim_marks_round_trip_exactly() {
        for (k1, k2) in [
            (MarkKind::Strong, MarkKind::Strike),
            (MarkKind::Strike, MarkKind::Strong),
            (MarkKind::Emph, MarkKind::Strike),
            (MarkKind::Underline, MarkKind::Emph),
            (MarkKind::Strong, MarkKind::Underline),
        ] {
            let rt = marked(
                "abcdef",
                vec![
                    Mark {
                        start: 0,
                        end: 4,
                        kind: k1.clone(),
                    },
                    Mark {
                        start: 2,
                        end: 6,
                        kind: k2.clone(),
                    },
                ],
            );
            let md = to_markdown(&rt);
            let rt2 = from_markdown(&md).unwrap();
            assert_eq!(rt, rt2, "{k1:?}+{k2:?} overlap not a fixed point: {md:?}");
        }
    }

    /// A formatting mark partially overlapping an atomic `code` span can't wrap
    /// the code's interior; the wrap clips to the text outside so the markdown
    /// stays balanced (the #846 shape, here in the markdown emitter).
    #[test]
    fn wrap_over_code_stays_balanced() {
        let rt = marked(
            "abcdef",
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 2,
                    end: 6,
                    kind: MarkKind::Code,
                },
            ],
        );
        let md = to_markdown(&rt);
        assert_eq!(md, "**ab**`cdef`\n");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "abcdef");
    }

    /// Issue #848 part 2: a literal `&` (or an entity-shaped `&amp;`) must not
    /// re-import as the decoded entity. `from_markdown("\\&amp;")` yields corpus
    /// text "&amp;"; exporting it unescaped as `&amp;` used to re-import as "&".
    #[test]
    fn ampersand_and_entities_round_trip() {
        // A bare `&` and an entity-shaped run both survive.
        round_trips("a & b");
        round_trips("copyright \\&copy; sign");
        // The pinned repro: literal "&amp;" text.
        let rt = from_markdown("\\&amp;").unwrap();
        assert_eq!(rt.text, "&amp;");
        let md = to_markdown(&rt);
        assert!(md.contains("\\&"), "the `&` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "&amp;", "entity-shaped text must not decode");
        assert_eq!(rt, rt2);
    }

    /// Issue #848 part 3: heading text ending in a `#` run must not re-import as
    /// an ATX closing sequence. `from_markdown("# a \\#")` yields heading text
    /// "a #"; exporting it as `# a #` used to re-import as "a", dropping the `#`.
    #[test]
    fn heading_trailing_hash_round_trips() {
        let rt = from_markdown("# a \\#").unwrap();
        assert_eq!(rt.text, "a #");
        let md = to_markdown(&rt);
        assert!(md.contains("\\#"), "trailing `#` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "a #", "trailing `#` must survive");
        assert_eq!(rt, rt2);
        // A multi-`#` trailing run and a no-space `#` both round-trip.
        round_trips("# heading \\#\\#");
        round_trips("## title\\#");
    }
}
