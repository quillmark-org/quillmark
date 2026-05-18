# Quillmark Markdown

**Status:** Draft specification
**Editor:** Quillmark Team
**Base:** [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)

Quillmark Markdown is a **strict superset of CommonMark** with one declared
deviation. It layers a structured-data system — the **card-yaml** format — on
top of ordinary markdown, and selects a small, stable set of GFM extensions.
This document is the authoritative syntax standard.

## 1. Superset Statement

Every valid CommonMark 0.31.2 document parses to the same block / inline
structure under this spec, *except* for the deviation declared in §6.2.
Additionally, this spec defines:

- **Structured data** — card-yaml blocks (§3).
- **Extensions** — strikethrough, pipe tables, and `<u>` for underline
  (§6.1).

A document containing no card-yaml blocks is ordinary CommonMark, parsed as
such.

## 2. The card-yaml Format

The card-yaml format isolates structured metadata from markdown prose. By
keeping the data payload inside an explicitly delimited block, separate from
the unstructured body that follows it, the format keeps LLM generation stable
and prevents state corruption — a generator editing prose cannot accidentally
disturb the structured fields, and vice versa.

A document is a sequence of **blocks**. Each block is one card-yaml block
followed by its prose body:

```
Document = (CardYamlBlock ProseBody)+
```

- **Root block** — the first block, identified purely by position. Its `#@`
  header declares the quill that renders the document.
- **Subsequent blocks** — zero or more *cards*. Each declares a composable
  structured record.
- **Prose body** — the markdown content between one block's closing fence and
  the next block's opening fence (or EOF).

### 2.1 Worked Example

```
~~~card-yaml
#@quill: example@0.1.0
from: "bob"
to: "alice"
~~~

This is the primary document container body text.

~~~card-yaml
#@kind: endorsement
#@id: rev-1
from: "charlie"
role: "reviewer"
clearance: "alpha"
~~~

I have reviewed the contents and officially endorse this flight plan.
```

The first block is the root block (by position); its `#@quill` header binds
the document to the `example` quill at version `0.1.0`. The second block is a
card whose `#@kind` is `endorsement`. The text after each closing `~~~` fence
is that block's prose body.

## 3. card-yaml Blocks

### 3.1 Structural Rules

A card-yaml block has four parts, in order:

1. **Opening fence** — exactly `~~~card-yaml` (see §3.2). The info string
   alone identifies the block; no further declaration is needed.
2. **System metadata header** — an optional leading run of `#@key: value`
   lines inside the block (see §3.3). The root block must declare `#@quill`;
   all other `#@` entries are optional.
3. **Data payload** — standard YAML key/value pairs below the `#@` header
   (see §3.4).
4. **Closing fence** — exactly `~~~` (see §3.2).

The prose body begins immediately after the closing `~~~` fence and runs to
the next opening fence or EOF.

### 3.2 Delimiter and Info String

- **Delimiter.** Blocks open and close exclusively with `~~~` — three tildes,
  no more, no fewer. The closing fence is exactly `~~~` and carries no info
  string.
- **Info string.** The opening fence must be exactly `~~~card-yaml`. No other
  info string opens a card-yaml block.
- **Indentation.** The `~~~card-yaml` opener and its closing `~~~` are at
  column zero — no leading spaces.
- **Line endings.** `\n` and `\r\n` are equally accepted.
- **Blank-line rule.** A blank line is required immediately above every
  `~~~card-yaml` opener, *except* when the opener is the very first line of
  the document. A `~~~card-yaml` line without a blank line above it is **not**
  a card-yaml opener — it is treated as an ordinary CommonMark fenced code
  block. Requiring the blank line keeps prose-body round-tripping stable and
  prevents a card-yaml block from being absorbed into a preceding paragraph.

### 3.3 System Metadata (`#@`)

A block may begin with a **system metadata header** — an optional leading run
of `#@key: value` lines. These lines carry no parser semantics beyond the
rules below; they are simply kept out of the YAML payload's user field set
(§3.4). The `#@` header is not part of the data model's field map.

- **`#@quill: <name>@<version>`** — binds the document to a quill (see §3.5
  for the version-selector forms). This is the **only required `#@` entry**,
  and it must be declared by the **root block** (the first block). It may
  appear on any block.
- **`#@kind: <value>`** — optional metadata identifying a card's kind. There
  is no reserved kind; `#@kind` is plain metadata.
- **`#@id: <value>`** — an opaque, optional identifier. It is plain metadata:
  no validation, no uniqueness requirement; it is carried through the
  round-trip unchanged.
- Any other `#@key` is accepted and carried as system metadata.

Rules:

- The `#@` header is optional on every block *except* the root block, which
  must declare `#@quill`.
- `#@` header lines may appear in any order. The emitter preserves their
  source order (see §9).
- A duplicate `#@key` within a single block is a parse error.
- A malformed `#@` line (not of the form `#@key: value`) is a parse error.
- `<value>` for `#@kind` matches `/^[a-z_][a-z0-9_]*$/`.
- **Comments are not supported on a `#@` header line itself.** Unlike the
  payload (§3.4), a `#@` line carries no trailing `#` comment.

### 3.4 Data Payload

Standard YAML key/value pairs sit directly below the `#@` metadata header.

- **Field names.** Every field name matches `/^[a-z_][a-z0-9_]*$/`.
- **Reserved names.** `QUILL`, `CARD`, `BODY`, and `CARDS` are reserved and
  may not be used as field names.
- **Whitespace-only payload.** A block whose payload is only whitespace
  yields an empty field set.
- **YAML comments.** Both own-line comments (`# …` on their own line) and
  inline comments (`field: value  # note`) are supported in the payload and
  round-trip through `toMarkdown`. (A `#@` header line is the one
  exception — see §3.3.) Comments inside nested YAML values (arrays, maps)
  are also preserved: the pre-scan captures each nested comment with a
  structural path and the emitter re-injects it at the matching position.
- **The `!fill` tag.** `!fill` is the single supported YAML tag; it marks a
  top-level field as a placeholder awaiting user input and round-trips
  through emit. `!fill` may be applied to scalars (string, integer, float,
  bool, null) and sequences; it is rejected on mappings because Quillmark's
  schema has no top-level `type: object`. Any other custom tag (`!include`,
  `!env`, …) is dropped with a `parse::unsupported_yaml_tag` warning; the
  scalar value is kept but the tag does not round-trip.

### 3.5 Version Selectors

The `#@quill` value is `<name>@<version>`, where `<version>` is one
of:

| Form | Meaning |
|---|---|
| `name@2.1.0` | exact version |
| `name@2.1` | latest `2.1.x` |
| `name@2` | latest `2.x.x` |
| `name@latest` | latest overall (explicit) |
| `name` | latest overall (default — `@version` omitted) |

Quill names match `/^[a-z][a-z0-9_]*$/`. See [VERSIONING.md](./VERSIONING.md)
for resolution semantics.

## 4. Block Detection

A single detector runs over the line stream. A `~~~card-yaml` line opens a
card-yaml block **iff** both of the following hold:

**D1 — Blank line above.** The `~~~card-yaml` line is line 1 of the document,
or the line immediately above it is blank.

**D2 — Closing fence.** A matching `~~~` line appears later in the document.

YAML content between recognised fence markers is opaque to detection — a
`~~~card-yaml` line inside an open block is part of that block's payload, not
a new opener (though the canonical payload never produces such a line).

A `~~~card-yaml` line that fails D1 is delegated to CommonMark as an ordinary
fenced code block. A `~~~card-yaml` opener with no matching `~~~` closer
before EOF is a hard parse error (§9).

### 4.1 Worked Example

```
~~~card-yaml
#@quill: resume@1.0.0
title: CV
~~~

Main body text.

***

A thematic break in prose stays a thematic break.

~~~card-yaml
#@kind: profile
name: "Alice"
~~~

Profile body.
```

The first `~~~card-yaml` is the root block (line 1, D1 satisfied). The second
opens a `profile` card (blank line above). The `***` is an ordinary
CommonMark thematic break — card-yaml does not reserve any thematic-break
syntax.

## 5. Data Model

Parsing yields:

```typescript
interface Document {
  QUILL: string;          // quill name@version, from the root block #@quill
  BODY: string;           // prose body of the root block
  CARDS: Card[];          // zero or more cards, in document order
  [field: string]: any;   // other root-block payload fields
}

interface Card {
  CARD: string;           // card kind, matches /^[a-z_][a-z0-9_]*$/
  BODY: string;           // card prose body
  [field: string]: any;   // other card payload fields
}
```

- `CARDS` is always present, possibly empty.
- Root-block fields and card-field names may collide freely; each card is its
  own scope.
- Body text is preserved verbatim — whitespace, line endings, and inline
  CommonMark are untouched by the splitter.

## 6. Markdown Content

Body regions (the root body and every card body) are rendered as CommonMark
0.31.2 with the extensions and deviations below.

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
   layers agree. Authors editing on Windows or pasting from sources that
   emit CR-bearing line terminators otherwise leave bare `\r` bytes in
   the body, which some backends render as visible garbage.
2. **Bidi control stripping.** Remove U+061C, U+200E, U+200F,
   U+202A–U+202E, U+2066–U+2069. These invisible characters can
   desynchronize delimiter runs when copy-pasted from bidi-aware sources.
3. **HTML comment fence repair.** If `-->` is followed by non-whitespace
   text on the same line, insert a newline after `-->` so the trailing
   text reaches the paragraph parser instead of being consumed by the
   CommonMark HTML-block rule (type 2).

Normalization is applied identically to the root body and every card
body. It is not applied to YAML payload values.

## 8. Limits

Conforming parsers MUST enforce these limits and MUST surface a parse
error when any is exceeded:

| Limit | Value |
|---|---|
| Document size | 10 MB |
| YAML payload size per block | 1 MB |
| YAML nesting depth | 100 |
| Markdown block nesting depth | 100 |
| Field count per block | 1000 |
| Card count per document | 1000 |

## 9. Emission Contract

`toMarkdown` always emits the **canonical form** of every block:

```
~~~card-yaml
<#@ header lines, in order>
<payload>
~~~
```

That is: a `~~~card-yaml` opener, the `#@` system-metadata lines (in their
source order), the YAML payload, and a `~~~` closer. The root block's `#@`
header includes `#@quill`; other blocks emit whatever `#@` entries they
declared, or none. A document round-trips to this canonical shape — fence
markers, key ordering, and YAML quoting are normalised. `!fill` tags and
payload comments (own-line and inline) survive the round-trip; the `#@`
header lines are emitted without a comment.

## 10. Errors

Parse errors include:

- A `~~~card-yaml` opener with no matching `~~~` closer before EOF.
- The root block missing its `#@quill` entry.
- A malformed `#@` header line (not of the form `#@key: value`).
- A duplicate `#@key` within a single block.
- A `#@kind` value failing `/^[a-z_][a-z0-9_]*$/`.
- A field name failing `/^[a-z_][a-z0-9_]*$/`.
- Use of a reserved name (`QUILL`, `CARD`, `BODY`, `CARDS`) as a field name.
- Invalid YAML inside any block payload.
- Any §8 limit exceeded.

## 11. References

- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
- [GitHub Flavored Markdown](https://github.github.com/gfm/) (pipe tables,
  strikethrough)
- [`VERSIONING.md`](./VERSIONING.md) — quill version-selector resolution
- [`CARDS.md`](./CARDS.md) — downstream card-handling semantics
