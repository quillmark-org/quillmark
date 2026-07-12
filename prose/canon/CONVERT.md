# RichText → Typst Lowering

> **Implementation**: `crates/backends/typst/src/emit.rs`

## TL;DR

The Typst backend lowers a richtext corpus (`RichText`) to Typst markup with
`emit_richtext`, which walks the corpus — lines, anchored marks, embedded
islands — and never re-parses markdown. Alongside the markup it records a
per-segment source map (`corpus ↔ generated` byte windows). This is the only
markup-producing path in the render engine. Markdown reaches the corpus once, at
ingest, in `quillmark-richtext::import`; the normative rules for *which* markdown
a corpus can hold live in [markdown-spec.md §6](../references/markdown-spec.md);
this page documents how the backend lowers the corpus it produces.

## Pipeline

```
emit_richtext(&RichText) -> Result<EmittedContent, EmitError>
  ├─ block walk    lines → headings, paragraphs, code fences, lists, quotes, islands
  ├─ mark sweep    anchored marks → nested #strong[…] / #emph[…] / #link(…)[…] / …
  └─ source map    per-segment (corpus ↔ gen) windows + one (corpus, gen) pair per run
```

`EmittedContent { markup: String, segments: Vec<SegmentMap> }`. The corpus is a
single Unicode-scalar-value (USV) `text` carrying `lines` (line attributes and
container nesting), `marks` (anchored `[start, end)` ranges), and `islands`
(tables/images at reserved slot chars). The walk is a terminator-model block
tree over `lines`; the inline pass sweeps `marks` and islands within each line.

A **segment** is a maximal run of lines joined by `Line::continues` — one
paragraph, one heading, one whole code fence, one island line. It is what
"paragraph-level" means against the corpus, and the unit a region keys on.

## Escape functions

Two escapers guard the two Typst contexts; both live in `emit`:

- **`escape_markup`** — text in markup context. Escapes (backslash first)
  `\ // ~ * _ ` `` ` `` ` # [ ] { } $ < > @`. Applied to plain text runs and to
  a table cell's text.
- **`escape_string`** — text inside a Typst string literal. Escapes
  `\ " \n \r \t` and other control characters as `\u{…}`. Applied to `#link` /
  `#image` URLs, code content, and code-fence language tags.

## Element mapping

| Corpus construct | Typst |
|---|---|
| `LineKind::Heading{level}` | `=` … `======` (`level` × `=`) |
| `LineKind::Para` | inline content; a hard break (a `continues` line join) emits `#linebreak()`, a soft break is a space (both settled at import) |
| `LineKind::Code{lang}` (code fence) | `#raw(block: true, lang: "…", "…")`; `lang:` emitted only when the language tag is non-empty |
| `LineKind::Rule` (thematic break) | `#line(length: 100%)` |
| `MarkKind::Strong` | `#strong[…]` |
| `MarkKind::Emph` | `#emph[…]` |
| `MarkKind::Underline` | `#underline[…]` |
| `MarkKind::Strike` | `#strike[…]` |
| `MarkKind::Code` | `#raw("…")` (inline) |
| `MarkKind::Link{url}` | `#link("url")[…]` (`escape_string` on the url) |
| `MarkKind::Anchor` / `Unknown` | nothing |
| `Container::ListItem` (bullet) | `- ` |
| `Container::ListItem` (ordered) | `+ ` auto-numbered; first item emits `N. ` when the list starts at `N ≠ 1` |
| `Container::Quote` | `#quote(block: true)[…]` |
| `image` island | `#image("url", alt: "…")`; `alt:` omitted when empty |
| `table` island | `#table(columns: N, align: (…), table.header(…), …)` |

Table alignment maps `none→auto`, `left`, `center`, `right`; the `align:`
argument is emitted only when at least one column is non-default. A table cell is
canonical `{text, marks}`, lowered through the same mark sweep as prose — a
formatted cell reaches `#strong[…]` / `#emph[…]` / `#raw(…)` / `#link(…)[…]`, not
an escaped source slice.

**Block quotes render** as `#quote(block: true)[…]` — the one lowering
divergence from a flat inline pass; a quote's inner blocks lower under the
block-level discipline.

Anchor and unknown marks emit nothing; unknown island types emit nothing
(parallel to the HTML rule at import). Content that import never admits into the
corpus — raw HTML other than `<u>`, HTML comments, `<br>`, math, footnotes, task
lists, definition lists (markdown-spec §6.3) — is simply absent here.

## Mark sweep

Marks anchor freely and may overlap (Peritext semantics from an editor); Typst
markup nests. The sweep opens marks by priority `(start, longer-span-first,
kind-ord)` and closes-and-reopens deeper survivors at each overlap boundary, so
free overlap lowers to properly nested markup — `strong[0,4)` over `emph[2,6)`
on `abcdef` becomes `#strong[ab#emph[cd]]#emph[ef]`, bracket-balanced. `code`
marks render atomically as `#raw("…")` (their content is a string literal, so no
inner mark applies).

## Source map

Each segment records a `SegmentMap`:

```rust
struct SegmentMap {
    corpus: Range<usize>,                                 // USV, the segment's corpus span
    gen:    Range<usize>,                                 // bytes into `markup`
    runs:   Vec<(Range<usize>, Range<usize>, EscapeCtx)>, // (corpus USV, gen bytes) per text run
}
enum EscapeCtx { Markup, StringLit }
```

A **run** is one plain-text stretch between marks, islands, and line breaks;
`gen` slices exactly `escape_markup(corpus_slice)` (or `escape_string` for code /
string-literal runs). Structural bytes — mark delimiters, container syntax,
`#linebreak()` — fall between runs, inside `gen` but under no run. This is the
only place a per-segment source map can be produced, because it is the only place
that both lowers the corpus and knows the resulting byte layout.

Per-character spans within a run are **recomputed**, not stored: a one-scan
treats the `//`→`\/\/` markup escape as a 2-char/4-byte cluster and every other
character as its own. The `escape_tripwire` test pins that scan against
`escape_markup` / `escape_string` byte-for-byte, so an escape-rule change fails
loud.

## Where markdown is parsed

The markdown engine (`pulldown-cmark`) appears exactly once in the workspace, in
`quillmark-richtext::import`, run at ingest. `import` normalizes, parses, and
lowers markdown into the corpus (markdown-spec §6 is its normative acceptance
surface); every downstream render walks the corpus. No render path parses
markdown.

## Codegen integration

`generate_lib_typ` (`helper.rs`) lowers each content field's corpus to a markup
**block** binding `#let _qm_cN = [ .. ]` via `emit_richtext`, then rebases the
emitter's segment map from block-relative to `lib.typ`-relative offsets, yielding
one `ContentMap { path, block, segments }` per content field. The generated
`data` dict references `_qm_cN`; a blank corpus stays an empty string literal.
The file parser parses each block once — no runtime `eval`, no `json()` blob.

## Gotchas

- **Backslash first.** `escape_markup` replaces `\` before any other character,
  or later escapes would be double-escaped.
- **All code is `#raw(...)`, not backtick markup.** Both inline code and code
  fences put content into a string literal where backtick runs are inert — no
  delimiter can collide, and `escape_string` covers the only specials (`"` / `\`).
  The function form makes inline-vs-block explicit via `block:`. A fence buffers
  its lines into one string joined by escaped `\n`.
- **Ordered-list start.** Typst's `+` marker always restarts at 1. A start
  number `≠ 1` is preserved by writing the explicit number on the first item
  (`5. …`); Typst then continues the following `+` items from there.
- **List markers.** Bullet items become Typst `-`; ordered items become Typst `+`
  (its enumeration marker), not `-`.
