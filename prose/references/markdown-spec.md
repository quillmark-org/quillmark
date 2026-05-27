# Quillmark Markdown Specification

> **Status**: Authoritative specification
> **Base**: [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
> **Implementation**: `crates/core/src/document/`

Quillmark Markdown is a **strict superset of CommonMark** with one declared
deviation. It layers a structured-data system — the **card-yaml** format — on
top of ordinary markdown, and selects a small, stable set of GFM extensions.
This document is the authoritative syntax standard.

## 1. Superset Statement

Every valid CommonMark 0.31.2 document parses to the same block / inline
structure under this spec, *except* for the deviations declared in §6.2
(raw HTML) and §3.2.1 (root-block `---` alias — a `---` at document start
followed by a matching `---` is interpreted as a YAML-frontmatter root
block, not a thematic break / setext underline). Additionally, this spec
defines:

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

- **Root block** — the first block, identified purely by position. Its
  `$quill` metadata declares the quill that renders the document.
- **Subsequent blocks** — zero or more *cards*. Each declares a composable
  structured record.
- **Prose body** — the markdown content between one block's closing fence and
  the next block's opening fence (or EOF).

### 2.1 Worked Example

```
~~~card-yaml
$quill: example@0.1.0
$kind: main
from: "bob"
to: "alice"
~~~

This is the primary document container body text.

~~~card-yaml
$kind: endorsement
$id: rev-1
from: "charlie"
role: "reviewer"
clearance: "alpha"
~~~

I have reviewed the contents and officially endorse this flight plan.
```

The first block is the root block (by position); its `$quill` entry binds
the document to the `example` quill at version `0.1.0`. The second block is a
card whose `$kind` is `endorsement`. The text after each closing `~~~` fence
is that block's prose body.

## 3. card-yaml Blocks

### 3.1 Structural Rules

A card-yaml block has three parts, in order:

1. **Opening fence** — exactly `~~~card-yaml` (see §3.2). The info string
   alone identifies the block; no further declaration is needed.
2. **YAML payload** — a standard YAML mapping containing both system
   metadata (`$`-prefixed reserved keys; see §3.3) and the block's data
   fields (see §3.4).
3. **Closing fence** — exactly `~~~` (see §3.2).

The prose body begins immediately after the closing `~~~` fence and runs to
the next opening fence or EOF.

### 3.2 Delimiter and Info String

- **Delimiter.** Blocks open and close exclusively with `~~~` — three tildes,
  no more, no fewer. The closing fence is exactly `~~~` and carries no info
  string. (Exception: the root-block `---` alias in §3.2.1.)
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

### 3.2.1 Root-Block `---` Alias

The **root block only** may open with `---` and close with `---` instead of
`~~~card-yaml` / `~~~`. A `---`-fenced root parses identically to a
`~~~card-yaml`-fenced root with the same payload.

- **Accept, don't emit, don't advertise.** Parsers accept the `---` form so
  that LLMs trained on broader-internet YAML-frontmatter conventions are
  not penalised on a stylistic mismatch. `toMarkdown` (§9) always emits the
  canonical `~~~card-yaml` / `~~~` shape; a `---`-authored document
  round-trips to the canonical form on first re-emit. Authoring surfaces
  (blueprints, FORMAT_RULES, examples) document only the canonical form.
- **Root only.** A `---` opener is recognised only when every line above
  it is blank (i.e. document start, modulo leading blank lines) and no
  prior block has been parsed. Any other `---` line is delegated to
  CommonMark as a thematic break or setext-heading underline.
- **Matched fences.** Within a single block, opener and closer must agree:
  a `---` opener requires a `---` closer, and a `~~~card-yaml` opener
  requires a `~~~` closer. Mixed forms (`---` … `~~~`, `~~~card-yaml` …
  `---`) surface as the "never closed" parse error.
- **Composable position.** A `---` line after the root block — when it
  pairs with a later `---` and has YAML-key content between — is a
  misplaced composable card and is rejected with a diagnostic that names
  the canonical `~~~card-yaml` / `~~~` replacement (§10). Composable
  cards have no `---` alias.

### 3.3 System Metadata (`$`)

A block's YAML payload may contain **`$`-prefixed reserved keys** that carry
system metadata. The set is **closed**: only `$quill`, `$kind`, `$id`, and
`$ext` are accepted. Any other `$`-prefixed key is a parse error. These
keys are ordinary YAML — they are read by the same YAML parser that handles
the rest of the payload — but they are **extracted** from the user field
set after parsing; they are not part of the data model's field map (§3.4).

In the typed model, the `$` entries live as typed variants of the
unified payload-item list (`PayloadItem::Quill`, `PayloadItem::Kind`,
`PayloadItem::Id`, `PayloadItem::Ext`), interleaved in source order with
user fields and YAML comments. They are surfaced through typed
accessors — `card.quill()`, `card.kind()`, `card.id()`, `card.ext()` —
which return `Option<…>`. On a successfully parsed document the root
card always returns `Some(_)` for both `quill()` and `kind()` (with
`kind() == "main"`); composable cards return `None` for `quill()` and
`Some(_)` for `kind()` (any value other than `"main"`). The root's
`$kind: main` is synthesised when omitted in source (see §3.3 rules),
so the typed-accessor invariant holds regardless of whether the
author wrote the line.

- **`$quill: <name>@<version>`** — binds the document to a quill (see §3.5
  for the version-selector forms). The root block (the first block) must
  declare it; no other block may. The value is parsed into a typed quill
  reference as the block is read.
- **`$kind: <value>`** — identifies a card's kind. The value is
  name-validated at parse time and must match `[a-z_][a-z0-9_]*`. The kind
  `main` is **reserved for the document root**: the root block's kind is
  `main` by virtue of position. An explicit `$kind: main` on the root is
  accepted and round-trips byte-equal; omitting it is also accepted, in
  which case the parser synthesises the entry at the canonical position
  so the in-memory model is uniform. A non-`main` `$kind` on the root is
  a parse error, and no composable card may declare `$kind: main`.
- **`$id: <value>`** — an opaque, optional identifier. Plain metadata: no
  validation, no uniqueness requirement; carried through round-trip
  unchanged.
- **`$ext: <mapping>`** — an opaque, optional **mapping** reserved for
  out-of-band extension data (UI editor state, agent annotations, …).
  Required to be a YAML mapping (object); scalars and sequences are
  rejected. Contents are carried verbatim through Markdown and storage
  DTO round-trips, and **never** appear in the plate JSON consumed by
  backends (§5). Bespoke consumers namespace their state inside the
  map — e.g. `$ext.presentation.title` for an editor-side card rename.
  An empty `$ext: {}` is preserved as a distinct, explicit declaration.

Rules:

- The root block must declare `$quill: <ref>`. Its `$kind` is `main`
  by position: an explicit `$kind: main` is accepted, an omitted `$kind`
  is accepted (and synthesised on parse), and any other `$kind` value
  is a parse error. A composable (non-root) block must declare
  `$kind: <kind>` for some `<kind>` other than `main`, and must not
  declare `$quill`.
- `$` metadata entries may appear at any position within the payload, and
  may be interleaved with data fields. The emitter preserves source order
  (see §9); newly constructed metadata that does not have a source-order
  is emitted in the canonical key order `$quill`, `$kind`, `$id`, `$ext`.
- A duplicate `$key` within a single block is a parse error (a YAML mapping
  cannot carry two entries under the same key).
- An unknown `$key` (anything outside `{quill, kind, id, ext}`) is a parse
  error.
- An invalid `$quill` reference is a parse error.
- A `$`-prefixed key whose value type is wrong for the key (e.g. a sequence
  under `$quill`, a scalar under `$ext`) is a parse error.
- **YAML comments on `$` lines.** Inline trailing comments (`$quill: foo  #
  bound at build`) and adjacent own-line comments round-trip through the
  unified payload-item list — the same mechanism that preserves comments
  on data fields (§3.4). Both flavors survive parse → emit → parse.

### 3.4 Data Payload

User-defined fields sit in the same YAML payload as the `$` metadata keys
(§3.3); after metadata extraction, the remaining mapping entries are the
data payload.

- **Field names.** Every field name matches `/^[a-z_][a-z0-9_]*$/`. The
  pattern excludes `$` and uppercase, so a data field name can never collide
  with the metadata sigil (`$quill`, `$kind`, `$id`, `$ext`, `$body`,
  `$cards`) and
  is consistently lowercase across the wire format.
- **Whitespace-only payload.** A block whose payload (after metadata
  extraction) is only whitespace yields an empty field set.
- **YAML comments.** Both own-line comments (`# …` on their own line) and
  inline comments (`field: value  # note`) are supported on data fields and
  round-trip through `toMarkdown`. (Comments targeting `$` metadata lines
  are the one exception — see §3.3.) Comments inside nested YAML values
  (arrays, maps) are also preserved: the pre-scan captures each nested
  comment with a structural path and the emitter re-injects it at the
  matching position.
- **The `!fill` tag.** `!fill` is the single supported YAML tag; it marks a
  top-level data field as a placeholder awaiting user input and round-trips
  through emit. `!fill` may be applied to scalars (string, integer, float,
  bool, null) and sequences; it is rejected on mappings because Quillmark's
  schema has no top-level `type: object`. `!fill` may not be applied to a
  `$` metadata key. Any other custom tag (`!include`, `!env`, …) is
  dropped with a `parse::unsupported_yaml_tag` warning; the scalar value is
  kept but the tag does not round-trip.

### 3.5 Version Selectors

The `$quill` value is `<name>@<version>`, where `<version>` is one
of:

| Form | Meaning |
|---|---|
| `name@2.1.0` | exact version |
| `name@2.1` | latest `2.1.x` |
| `name@2` | latest `2.x.x` |
| `name@latest` | latest overall (explicit) |
| `name` | latest overall (default — `@version` omitted) |

Quill names match `/^[a-z][a-z0-9_]*$/`. Resolution of partial selectors to
concrete versions is performed by the quill registry; this spec fixes only
the surface syntax accepted on the `$quill` line.

## 4. Block Detection

A single detector runs over the line stream. A `~~~card-yaml` line opens a
card-yaml block **iff** both of the following hold:

**D1 — Blank line above.** The `~~~card-yaml` line is line 1 of the document,
or the line immediately above it is blank.

**D2 — Closing fence.** A matching `~~~` line appears later in the document.

A `---` line opens the **root block** instead **iff** all of the following
hold (see §3.2.1):

**R1 — Document start.** No prior block has been parsed and every line above
the `---` line is blank.

**R2 — Closing `---`.** A matching `---` line appears later in the document.

YAML content between recognised fence markers is opaque to detection — a
`~~~card-yaml` line inside an open block is part of that block's payload, not
a new opener (though the canonical payload never produces such a line). The
same applies to `---` lines inside an open `---`-fenced root block.

A `~~~card-yaml` line that fails D1 is delegated to CommonMark as an ordinary
fenced code block. A `~~~card-yaml` opener with no matching `~~~` closer
before EOF — and equivalently a `---` root opener with no matching `---`
closer — is a hard parse error (§10). A `---` line that fails R1 falls
through to CommonMark unless it pairs with a later `---` line that holds
YAML-key content between them, in which case it is rejected as a misplaced
composable card (§10).

### 4.1 Worked Example

```
~~~card-yaml
$quill: resume@1.0.0
$kind: main
title: CV
~~~

Main body text.

***

A thematic break in prose stays a thematic break.

~~~card-yaml
$kind: profile
name: "Alice"
~~~

Profile body.
```

The first `~~~card-yaml` is the root block (line 1, D1 satisfied). The second
opens a `profile` card (blank line above). The `***` is an ordinary
CommonMark thematic break — card-yaml does not reserve any thematic-break
syntax.

## 5. Data Model

Parsing yields the `Document` model, which serialises via
`to_plate_json` into the following wire shape for backend templates.
All system-metadata keys are `$`-prefixed; user payload fields live flat
at the root and cannot collide with metadata because field names exclude
the `$` sigil.

```typescript
interface PlateJson {
  $quill: string;         // quill name@version, from the root block $quill
  $body: string;          // prose body of the root block
  $cards: Card[];         // zero or more cards, in document order
  [field: string]: any;   // root-block payload fields, flat
}

interface Card {
  $kind: string;          // card kind, matches /^[a-z_][a-z0-9_]*$/
  $body: string;          // card prose body
  [field: string]: any;   // card payload fields, flat
}
```

- `$cards` is always present, possibly empty.
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
  supported. In markdown body text `$` is literal; inside a `~~~card-yaml`
  payload `$` is reserved as the prefix for system-metadata keys (§3.3).
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
<payload items in source order>
~~~
```

That is: a `~~~card-yaml` opener, the YAML payload (typed `$` system
metadata, user data fields, and YAML comments interleaved in source
order), and a `~~~` closer. The root block must declare `$quill`;
canonical emission also writes `$kind: main` on the root, synthesising
it when the input omitted the line (see §3.3). Composable cards must
declare `$kind: <kind>`. A document round-trips to this canonical
shape — fence markers and YAML quoting are normalised, including the
`---`-fenced root alias (§3.2.1), which re-emits as `~~~card-yaml` /
`~~~`. `!fill` tags and YAML comments (own-line and inline, including
those adjacent to `$` lines) survive the round-trip.

Programmatically constructed metadata that does not have a source-order
emits in the canonical key order `$quill`, `$kind`, `$id`, `$ext` — the
typed mutators (`set_quill` / `set_kind` / `set_id` / `set_ext`) insert
at these positions.

### 9.1 Canonical Idempotence

A document in canonical form round-trips byte-equal under both
`toMarkdown ∘ fromMarkdown` and `fromJson ∘ toJson`:

- **`toMarkdown(fromMarkdown(canonical)) == canonical`** — the canonical
  form is a parse-emit fixed point.
- **`toMarkdown(fromMarkdown(arbitrary)) == toMarkdown(fromMarkdown(
  toMarkdown(fromMarkdown(arbitrary))))`** — at most one round-trip
  canonicalises any valid input; further round-trips are no-ops.
- **`toJson(fromJson(toJson(x))) == toJson(x)`** for any in-memory
  `Document x` — JSON serialization is byte-deterministic within a schema
  version.
- **The Markdown and JSON forms agree:** `toMarkdown(fromJson(toJson(x)))
  == toMarkdown(x)` for every `Document x` produced by
  `fromMarkdown(arbitrary)`. The two persistence formats canonicalise to
  the same in-memory model.

Arbitrary (non-canonical) input parses successfully when it satisfies §1–8
and converges to the canonical form on the first emit. Type fidelity (a
quoted `"42"` survives as a string, an unquoted `42` survives as an
integer) is preserved, along with the source positions of `$` metadata
keys and YAML comments; fence-marker length and quoting style are not.
The canonical form is what consumers should content-hash, content-address,
or compare for equality.

## 10. Errors

Parse errors include:

- A `~~~card-yaml` opener with no matching `~~~` closer before EOF (or,
  equivalently, a `---` root opener with no matching `---` closer).
- A `---` line in composable position (after the root block) that pairs
  with a later `---` and holds YAML-key content between — composable
  cards must use `~~~card-yaml` / `~~~` (§3.2.1).
- Mixed fence markers within a single block — `---` opener with `~~~`
  closer or vice versa (surfaces as the "never closed" error).
- The root block missing its `$quill` entry.
- The root block declaring a non-`main` `$kind` (an omitted `$kind` on
  the root is accepted and synthesised; only an explicit non-`main`
  value is rejected).
- A composable (non-root) block declaring `$quill`, or declaring
  `$kind: main` (which is reserved for the document root).
- A duplicate `$key` within a single block (caught by the YAML parser as a
  duplicate mapping key).
- An unknown `$key` outside the closed set `{quill, kind, id, ext}`.
- An invalid `$quill` reference.
- A `$` metadata key whose value type is incompatible with the key.
- A data-field name failing `/^[a-z_][a-z0-9_]*$/`.
- Invalid YAML inside any block payload.
- Any §8 limit exceeded.

## 11. References

- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
- [GitHub Flavored Markdown](https://github.github.com/gfm/) — pipe tables
  and strikethrough.
