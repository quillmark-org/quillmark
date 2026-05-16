# Markdown Syntax

Quillmark Markdown is a **strict superset of [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)** with a small set of [GitHub Flavored Markdown](https://github.github.com/gfm/) extensions and **one declared deviation**. If you already know CommonMark, you only need to learn what is on this page.

For the authoritative grammar, fence-detection rules, normalization, and limits, see the formal specification: [prose/designs/MARKDOWN.md](https://github.com/nibsbin/quillmark/blob/main/prose/designs/MARKDOWN.md).

## Foundation

Body content (the prose between frontmatter and any [card](cards.md), and inside each card) is parsed as CommonMark 0.31.2. Headings, emphasis, links, lists, code blocks, blockquotes, thematic breaks, and inline code all behave exactly as the [CommonMark spec](https://spec.commonmark.org/0.31.2/) defines them.

For the conventional syntax of these elements, refer to:

- [CommonMark spec](https://spec.commonmark.org/0.31.2/) — the base grammar.
- [CommonMark tutorial](https://commonmark.org/help/) — a 10-minute walk-through.
- [GFM spec](https://github.github.com/gfm/) — pipe tables and strikethrough.

The rest of this page covers only what Quillmark adds, removes, or interprets differently.

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

The following are recognised by the parser (so they will not corrupt surrounding content) but produce no output in the current version:

- **Images** (`![alt](src)`) — reserved for the asset-resolver integration; planned for v1.
- **Math** (`$…$`, `$$…$$`) — `$` is treated as a literal character.
- **Footnotes**, **task lists**, **definition lists** — not supported.

Some constructs (like link titles) are accepted by the parser but may be dropped during rendering when the active backend has no target for them. Those losses are backend-specific — see each backend's documentation.

## The `---` marker delimits frontmatter only

Quillmark uses `---` to delimit [frontmatter](yaml-frontmatter.md) at the top
of a document. Mid-document `---` is a CommonMark thematic break — it no
longer opens a metadata fence. Inline structured records use a different
syntax; see [Cards](cards.md).

## Next steps

- [YAML Frontmatter](yaml-frontmatter.md)
- [Cards](cards.md)
