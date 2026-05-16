# Quillmark Markdown

**Status:** Draft specification
**Editor:** Quillmark Team
**Base:** [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
**Pending rework:** [CARD_MODEL.md](../proposals/CARD_MODEL.md) renames the
inline-record vocabulary "leaf" → "card" (` ```card <kind> `). This spec
describes the current implementation.

Quillmark Markdown is a **strict superset of CommonMark** with one declared
deviation. It layers a structured-data system (YAML frontmatter + inline
*leaf* records) on top of ordinary markdown, and selects a small, stable set
of GFM extensions. This document is the authoritative syntax standard.

## 1. Superset Statement

Every valid CommonMark 0.31.2 document parses to the same block / inline
structure under this spec, *except* for the deviation declared in §6.2.
Additionally, this spec defines:

- **Structured data** — YAML frontmatter (`---/---` at top) and inline leaf
  records (`` ```leaf `` fenced code blocks) (§3).
- **Extensions** — strikethrough, pipe tables, and `<u>` for underline
  (§6.1).

Documents containing neither frontmatter nor leaves are ordinary
CommonMark, parsed as such.

## 2. Document Grammar

A document is a sequence of regions, in order:

```
Document = Frontmatter? Body (LeafFence LeafBody)*
```

- **Frontmatter** — at most one. A `---/---` pair at the top of the
  document, carrying `QUILL` plus any document-level fields (§3).
- **Body** — markdown content between the frontmatter close and the first
  leaf fence (or EOF).
- **Leaf fence + leaf body** — zero or more. Each leaf fence is a
  CommonMark fenced code block whose info string is `leaf <kind>`; its
  body declares a typed structured record. Markdown prose attached to the
  leaf follows the closing fence, up to the next leaf fence or EOF.

The two structures use *different* delimiters by design — `---/---` for
frontmatter (universal across the markdown ecosystem) and `` ```leaf `` for
inline records (CommonMark fenced code block, safe against Prettier and
thematic-break collisions).

## 3. Metadata Carriers

### 3.1 Frontmatter

A frontmatter block is a pair of `---` lines (with optional trailing
whitespace, 0–3 leading spaces of indentation). The first body key must be
`QUILL:`. The content between the fences is parsed as YAML.

- **Position.** Line 1 of the document, or preceded by a blank line.
- **Line endings.** `\n` and `\r\n` are equally accepted.
- **Reserved keys.** `BODY`, `LEAVES`, and `KIND` are **output-only** —
  the parser populates them on the parsed object and supplying them as
  input keys is a hard parse error. `QUILL` is the sentinel and must be
  the first body key.
- **YAML comments.** Own-line comments (`# …`) between the fence
  delimiters round-trip as first-class ordered items. Inline comments on
  value lines (`key: value  # note`) round-trip on the same line.
  Comments inside nested YAML values (arrays, maps) are preserved with
  structural paths and re-emitted at the matching position.
- **The `!fill` tag.** `!fill` marks a top-level field as a placeholder
  awaiting user input and round-trips through emit. It is permitted on
  scalars and sequences, rejected on mappings, and **forbidden on the
  sentinel key `QUILL`** (sentinels are routing keys, not data). Any
  other custom tag is dropped with a `parse::unsupported_yaml_tag`
  warning.

### 3.2 Leaves

A leaf is a [CommonMark fenced code block](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)
whose info string is exactly two whitespace-delimited tokens: `leaf`
followed by the **kind**. The body of the fence is parsed as YAML.

````markdown
```leaf product
name: Widget
price: 19.99
```

Body prose for this leaf, up to the next leaf or EOF.
````

- **Fence rules.** Inherit CommonMark §4.5 verbatim — opener and closer
  match by character and run length; closers carry no info string. To
  embed a fenced code block inside a leaf body, open the leaf with a
  longer fence (e.g. four backticks).
- **Indent.** 0–3 leading spaces are permitted, matching CommonMark.
- **Info string.** Exactly `leaf <kind>`. The kind matches
  `[a-z_][a-z0-9_]*`. A missing kind token, an invalid kind token, or any
  extra info-string token is a hard parse error (§4.2).
- **Reserved keys.** `BODY` and `KIND` are output-only inside a leaf —
  supplying either as an input body key is a hard parse error. `KIND` is
  populated from the info-string kind token. `QUILL` is not reserved
  inside leaves.
- **The `!fill` tag.** Same rules as frontmatter (§3.1). The kind lives in
  the info string, not the body, so `!fill` cannot reach it.

## 4. Fence Detection

### 4.1 Frontmatter detection

A `---` line opens a frontmatter block iff:

- **F2 — Position.** The line is at the top of the document (line 1, or
  preceded by blank lines only), or preceded by a blank line.
- **F3 — Indent.** The marker has 0–3 leading spaces. A line with four or
  more leading spaces (or any leading tab) is indented code per CommonMark
  §4.4, not a frontmatter marker.

The block extends from the opening `---` to the next `---` line. If the
content's first non-blank, non-comment key is `QUILL:`, the block is the
document frontmatter. Otherwise the `---/---` pair is delegated to
CommonMark and behaves as thematic breaks (the inner text is plain
paragraph content).

Only **one** frontmatter block is recognised — the first matching
`---/---` pair. Subsequent `---/---` pairs are CommonMark thematic breaks.

### 4.2 Leaf detection

A fenced code block is a leaf iff its info string's first token is `leaf`.
Detection is purely lexical: the parser commits to leaf-handling on that
first token alone, before reading any body content.

Once committed, the rest of the info string is *routing*: the second
token is the kind and selects the schema. A leaf info string that is not
exactly `leaf <kind>` — a missing kind token, an invalid kind token (one
not matching `[a-z_][a-z0-9_]*`), or any extra token — is a hard parse
error, not a fence-classification ambiguity. Likewise any malformed leaf
body (reserved-key collision, invalid YAML) is a hard error.

### 4.3 Worked example

````markdown
---
QUILL: catalog@1.0
title: Spring Catalog
---

# Introduction

Welcome to the spring catalog.

```leaf product
name: Widget
price: 19.99
```

The Widget is our flagship product.

```leaf product
name: Gadget
price: 29.99
```

The Gadget complements the Widget.
````

### 4.4 Failure modes

- **Frontmatter sentinel typo.** A top `---/quill: …/---` (lowercase) or
  similar near-miss emits a `parse::near_miss_sentinel` warning and is
  treated as thematic breaks. Parsing fails with `MissingQuillField` and a
  hint pointing at the actual key found.
- **Unknown info string.** ` ```leef ` is just a code block with an
  unknown language — silently passed through. Misspelt info strings are
  not near-miss diagnostics.
- **Missing kind token in a leaf.** A `` ```leaf `` fence with no kind
  token (or an invalid/extra token) is a hard parse error — the fence has
  been classified on the `leaf` token, so the diagnostic is specific.
- **Legacy `---/CARD: …/---` block.** Accepted in this release as a
  Release-N migration path: parsed as a leaf (the `CARD:` value becomes
  the kind), surfaces a `parse::deprecated_leaf_syntax` warning, and
  rewritten to ` ```leaf <kind> ` on `to_markdown()` round-trip. The
  legacy form will be a hard parse error in the next release.

## 5. Data Model

Parsing yields:

```typescript
interface Document {
  QUILL: string;          // template reference, from frontmatter
  BODY: string;           // body prose between frontmatter and first leaf
  LEAVES: Leaf[];         // zero or more leaves, in document order
  [field: string]: any;   // other frontmatter fields
}

interface Leaf {
  KIND: string;           // leaf kind, matches /^[a-z_][a-z0-9_]*$/
  BODY: string;           // leaf body prose
  [field: string]: any;   // other leaf fields
}
```

- `LEAVES` is always present, possibly empty.
- Templates may also access leaves grouped by kind via `leaves.<kind>[i]`
  (preserving document order within each kind).
- Frontmatter field names and leaf field names may collide freely; each
  leaf is its own scope.
- Body text is preserved verbatim — whitespace, line endings, and inline
  CommonMark are untouched by the splitter.

## 6. Markdown Content

Body regions (the document body and every leaf body) are rendered as
CommonMark 0.31.2 with the extensions and deviations below.

### 6.1 Extensions

| Feature | Syntax | Notes |
|---|---|---|
| Strikethrough | `~~text~~` | GFM rules: word-bounded delimiter runs only. |
| Pipe tables | GFM pipe-table syntax with alignment rows | Supports `:---`, `:---:`, `---:` alignment. |
| Underline (HTML) | `<u>text</u>` | The one allowlisted HTML tag (see §6.2). The only syntax for underline; handles intraword and arbitrary-range cases. |

### 6.2 Declared Deviation from CommonMark

**Raw HTML is accepted syntactically but produces no output, except
`<u>…</u>` which renders as underline.** The parser recognises HTML per
CommonMark §4.6 / §6.11, discards every event, and re-emits only the
`<u>` wrapper. Rationale: Typst has no HTML renderer, and arbitrary
passthrough would create an injection vector for downstream
HTML-producing tooling; `<u>` is the one exception because no
CommonMark-native syntax covers underline.

No other syntax deviates from CommonMark. Delimiter-run semantics for `*`,
`_`, `**`, `__`, and `~~` follow CommonMark and GFM exactly — in particular,
`__text__` renders as strong emphasis, identical to `**text**`.

### 6.3 Out of Scope

The following are parsed where CommonMark or pulldown-cmark already
handles them, but produce no Quillmark-specific output and may be
implemented in a future revision:

- Images (`![alt](src)`) — reserved for the asset-resolver integration;
  required for v1 of this spec.
- Math (`$…$`, `$$…$$`), footnotes, task lists, definition lists — not
  supported; `$` is literal.
- HTML comments — accepted syntactically, not rendered (see §6.2).
- `<br>`, `<br/>`, `<br />` — follow the raw-HTML rule (non-rendering);
  authors use CommonMark-native hard breaks (trailing two spaces plus
  newline, or trailing `\\` plus newline).

Backends MAY drop semantic data (e.g., link titles, image alt text)
that has no equivalent in their render target. Such losses are backend
concerns and are documented per backend, not in this spec.

## 7. Input Normalization

Before CommonMark parsing, each body region is normalized:

1. **Line-ending canonicalization.** `\r\n` and bare `\r` sequences are
   converted to `\n`. YAML scalars receive this treatment from the YAML
   parser itself; the body region does not, so this step ensures both
   layers agree.
2. **Bidi control stripping.** Remove U+061C, U+200E, U+200F,
   U+202A–U+202E, U+2066–U+2069. These invisible characters can
   desynchronize delimiter runs when copy-pasted from bidi-aware sources.
3. **HTML comment fence repair.** If `-->` is followed by non-whitespace
   text on the same line, insert a newline after `-->` so the trailing
   text reaches the paragraph parser instead of being consumed by the
   CommonMark HTML-block rule (type 2).

Normalization is applied identically to the document body and every leaf
body. It is not applied to YAML field values.

## 8. Limits

Conforming parsers MUST enforce these limits and MUST surface a parse
error when any is exceeded:

| Limit | Value |
|---|---|
| Document size | 10 MB |
| YAML size per fence | 1 MB |
| YAML nesting depth | 100 |
| Markdown block nesting depth | 100 |
| Field count per fence | 1000 |
| Leaf count per document | 1000 |

## 9. Errors

Parse errors include:

- Frontmatter started (top `---` with `QUILL:` first key) but no closing
  `---` before EOF.
- Frontmatter missing the `QUILL` key (no valid frontmatter found).
- Leaf fence opened but never closed.
- Leaf info string that is not exactly `leaf <kind>` — a missing kind
  token, a kind token failing the `/^[a-z_][a-z0-9_]*$/` pattern, or any
  extra info-string token.
- Use of an output-only reserved key (`BODY`, `LEAVES`, `KIND`) as a
  user-defined input field.
- `!fill` tag applied to the sentinel key `QUILL`.
- Invalid YAML inside any fence.
- Any §8 limit exceeded.

## 10. References

- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
- [GitHub Flavored Markdown](https://github.github.com/gfm/) (pipe tables,
  strikethrough)
- [`CARD_MODEL.md`](../proposals/CARD_MODEL.md) — pending proposal: unified
  "card" vocabulary, supersedes the leaf design this spec currently describes
