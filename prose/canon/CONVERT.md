# Content → Typst Lowering

> **Implementation**: `crates/backends/typst/src/` (the `emit` module)

## TL;DR

The Typst backend lowers a `Content` value to Typst markup with
`emit_content`, which walks the content — lines, anchored marks, embedded
islands — and never re-parses markdown. Alongside the markup it records a
per-segment source map (`content ↔ generated` byte windows). This is the only
markup-producing path in the render engine. Markdown reaches the content once, at
ingest, in `quillmark-content::import`; the normative rules for *which* markdown
a content can hold live in [markdown-spec.md §6](../references/markdown-spec.md);
this page documents how the backend lowers the content it produces.

## Pipeline

```
emit_content(&Content) -> Result<Emission, EmitError>
  ├─ block walk    lines → headings, paragraphs, code fences, lists, quotes, islands
  ├─ mark sweep    anchored marks → nested #strong[…] / #emph[…] / #link(…)[…] / …
  └─ source map    per-segment (content ↔ gen) windows + one (content, gen) pair per run
```

`Emission { markup: String, segments: Vec<SegmentMap> }`. The content is a
single Unicode-scalar-value (USV) `text` carrying `lines` (line attributes and
container nesting), `marks` (anchored `[start, end)` ranges), and `islands`
(tables/images at reserved slot chars). The walk is a terminator-model block
tree over `lines`; the inline pass sweeps `marks` and islands within each line.

A **segment** is a maximal run of lines joined by `Line::continues` — one
paragraph, one heading, one whole code fence, one island line. It is what
"paragraph-level" means against the content, and the unit a region keys on.

## Escape functions

Two escapers guard the two Typst contexts; both live in `emit`:

- **`escape_markup`** — text in markup context. Escapes (backslash first)
  `\ // ~ * _ ` `` ` `` ` # [ ] { } $ < > @`. Applied to plain text runs and to
  a table cell's text.
- **`escape_string`** — text inside a Typst string literal. Escapes
  `\ " \n \r \t` and other control characters as `\u{…}`. Applied to `#link` /
  `#image` URLs, code content, and code-fence language tags.

## Element mapping

| Content construct | Typst |
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
content — raw HTML other than `<u>`, HTML comments, `<br>`, math, footnotes, task
lists, definition lists (markdown-spec §6.3) — is simply absent here.

### Island props

An island's `props` is a per-type canonical object — the shape this lowering
reads and the shape the WASM boundary pins:

- **`table`** → `{ header: Cell[], rows: Cell[][], aligns: Align[] }`. `header`
  and each row hold `Cell = { text, marks }` (marks lowered through the sweep
  above); `aligns` is one `none | left | center | right` per column. Import
  normalizes to a single column count — header, every row, and `aligns` padded
  to the widest — so `columns:` and `align:` agree.
- **`image`** → `{ url, alt }`; `alt` is the empty string when the source omits it.

The `KnownIslandType` dispatch (`crates/content/src/island.rs`) owns these
shapes engine-side; the WASM surface pins them as `TableProps` / `ImageProps` /
`TableCell` and types `ContentIsland.props` per the open `type`
(`crates/bindings/wasm/src/engine.rs`). An island of any other type keeps opaque
`props` and lowers to nothing, as above.

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
    content: Range<usize>,                                 // USV, the segment's content span
    gen:    Range<usize>,                                 // bytes into `markup`
    runs:   Vec<(Range<usize>, Range<usize>, EscapeCtx)>, // (content USV, gen bytes) per text run
}
enum EscapeCtx { Markup, StringLit }
```

A **run** is one plain-text stretch between marks, islands, and line breaks;
`gen` slices exactly `escape_markup(content_slice)` (or `escape_string` for code /
string-literal runs). Structural bytes — mark delimiters, container syntax,
`#linebreak()` — fall between runs, inside `gen` but under no run. This is the
only place a per-segment source map can be produced, because it is the only place
that both lowers the content and knows the resulting byte layout.

Per-character spans within a run are **recomputed**, not stored: a one-scan
treats the `//`→`\/\/` markup escape as a 2-char/4-byte cluster and every other
character as its own. The `escape_tripwire` test pins that scan against
`escape_markup` / `escape_string` byte-for-byte, so an escape-rule change fails
loud.

## Where markdown is parsed

The markdown engine (`pulldown-cmark`) appears exactly once in the workspace, in
`quillmark-content::import`, run at ingest. `import` normalizes, parses, and
lowers markdown into the content (markdown-spec §6 is its normative acceptance
surface); every downstream render walks the content. No render path parses
markdown.

## Codegen integration

`generate_lib_typ` (`helper.rs`) lowers each content field's content to a markup
**block** binding `#let _qm_cN = [ .. ]` via `emit_content`, then rebases the
emitter's segment map from block-relative to `lib.typ`-relative offsets, yielding
one `ContentMap { path, block, segments }` per content field. The generated
`data` dict references `_qm_cN`; a blank content stays an empty string literal.
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
