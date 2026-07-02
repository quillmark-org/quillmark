# Markdown Syntax

Quillmark Markdown is a **strict superset of [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)** with a small set of [GitHub Flavored Markdown](https://github.github.com/gfm/) extensions and **one declared deviation**. If you already know CommonMark, you only need to learn what is on this page.

For the authoritative grammar, block-detection rules, normalization, and limits, see the formal [Markdown specification](../reference/markdown-spec.md).

## Foundation

Body content (the prose after each [card-yaml block](card-yaml.md), including any [card](card-yaml.md#card-blocks)) is parsed as CommonMark 0.31.2. Headings, emphasis, links, images, lists, code blocks, blockquotes, thematic breaks, and inline code all behave exactly as the [CommonMark spec](https://spec.commonmark.org/0.31.2/) defines them. (Images render to a Typst `#image`; the `src` must resolve against the backend's file system.)

For the conventional syntax of these elements, refer to:

- [CommonMark spec](https://spec.commonmark.org/0.31.2/) — the base grammar.
- [GFM spec](https://github.github.com/gfm/) — pipe tables and strikethrough.

## Selected GFM extensions

Quillmark enables a small, stable subset of GFM:

| Feature | Syntax | Notes |
|---|---|---|
| Strikethrough | `~~text~~` | Standard GFM rules; word-bounded delimiter runs. |
| Pipe tables | `\| col \| col \|` with alignment row | Supports `:---`, `:---:`, `---:` alignment. |
| Underline | `<u>text</u>` | The single allow-listed raw-HTML tag (see [the deviation below](#raw-html-is-not-rendered-except-u)). |

Task lists, autolinks beyond CommonMark's, and other GFM features are **not** enabled.

## Deviation from CommonMark

### Raw HTML is not rendered, except `<u>`

CommonMark passes raw HTML through to the output. Quillmark recognises raw HTML syntactically (so it does not break paragraph structure) but **discards every tag**, with one exception: `<u>…</u>` renders as underline.

```markdown
<u>This is underlined</u>, even <u>across word boundaries</u>.
<span style="color: red">This span is dropped entirely.</span>
<!-- HTML comments are also dropped -->
```

Why: Typst (the rendering backend) has no HTML renderer, and arbitrary HTML passthrough would create injection risks for downstream tooling. `<u>` is allowed because no CommonMark-native syntax covers arbitrary-range underline.

Consequences:

- `<br>`, `<br/>`, `<br />` produce no output. Use a CommonMark hard break instead — two trailing spaces before a newline, or a trailing `\` before a newline.
- HTML entities and embedded SVG are dropped.
- HTML comments do not appear in output.

## Out of scope

The following are recognised by the parser (so they will not corrupt surrounding content) but produce no output:

- **Math** (`$…$`, `$$…$$`) — `$` is treated as a literal character.
- **Footnotes**, **task lists**, **definition lists** — not supported.

Some constructs (like link titles) are accepted by the parser but may be dropped during rendering when the active backend has no target for them. Those losses are backend-specific — see each backend's documentation.

## Structured data: card-yaml blocks

Quillmark carries structured data in [card-yaml blocks](card-yaml.md),
each followed by its Markdown prose body. The full block-detection rules —
fence syntax, the blank-line rule, and the backtick escape hatch for literal
code blocks — are in
[§4 of the spec](../reference/markdown-spec.md#4-block-detection).
