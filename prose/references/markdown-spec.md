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
(raw HTML), §3.2 (a column-zero bare `~~~` block with a blank line above it
is a card-yaml block, not an ordinary fenced code block; an indented `~~~`
is not a card-yaml opener), and §3.2.1
(root-block `---` alias — a `---` at document start followed by a matching
`---` is interpreted as a YAML-frontmatter root block, not a thematic
break / setext underline). Additionally, this spec defines:

- **Structured data** — card-yaml blocks (§3).
- **Extensions** — strikethrough, pipe tables, and `<u>` for underline
  (§6.1).

A document containing no card-yaml blocks is ordinary CommonMark, parsed as
such.

## 2. The card-yaml Format

The card-yaml format isolates structured data from markdown prose.

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
~~~
$quill: example@0.1.0
$kind: main
from: "bob"
to: "alice"
~~~

This is the primary document container body text.

~~~
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

1. **Opening fence** — exactly `~~~` (three tildes, no info string; see §3.2).
   The `~~~card-yaml` info string is also accepted (non-canonical alias) on
   input.
2. **YAML payload** — a standard YAML mapping containing both system
   metadata (`$`-prefixed reserved keys; see §3.3) and the block's data
   fields (see §3.4).
3. **Closing fence** — exactly `~~~` (see §3.2).

The prose body begins immediately after the closing `~~~` fence and runs to
the next opening fence or EOF.

### 3.2 Delimiter and Info String

- **Delimiter.** Blocks open and close with a run of tildes. The canonical
  fence is exactly three tildes (`~~~`), and `toMarkdown` (§9) always emits
  three. An opener of four or more tildes is accepted (non-canonical) and
  re-emits as `~~~`; its closing fence must be at least as long as the opener,
  per CommonMark's fenced-code-block rule. (Exception: the root-block `---`
  alias in §3.2.1.)
- **Info string.** The canonical opening fence carries **no info string** — a
  bare `~~~`. The info string `~~~card-yaml` is accepted on input and parses
  identically, but is non-canonical: `toMarkdown` (§9) always emits the bare
  `~~~` form. No other info string opens a card-yaml block — a
  `~~~` fence carrying any other info string (e.g. a language) is an ordinary
  CommonMark fenced code block.
- **Escape hatch.** Because every column-zero `~~~` block is a card-yaml block,
  write a literal fenced *code* block in prose with a **backtick fence**
  (```` ``` ````). A `~~~` fence carrying a language info string is also an
  ordinary code block. There is no "longer tilde run" escape — more tildes
  still open a card.
- **Indentation.** Both fences are at column zero — **no leading spaces**.
  An indented opener (1–3 spaces) is *not* a card-yaml opener: it is
  delegated to CommonMark as an ordinary fenced code block, exactly like an
  opener that fails the blank-line rule below. The closing `~~~` must also
  be at column zero: the payload between the fences is YAML, where
  indentation is structural, so an indented `~~~` is payload (e.g. a line
  of a block scalar), never a closer. (This deliberately tightens
  CommonMark's closing-fence rule, which tolerates 1–3 leading spaces —
  that leniency exists for indented openers and list contexts, neither of
  which applies to card-yaml blocks, and honouring it would let a tilde
  fence inside a `|` block-scalar value silently truncate the block.)
- **Line endings.** `\n` and `\r\n` are equally accepted.
- **Blank-line rule.** A blank line is required immediately above every
  `~~~` opener, *except* when the opener is the very first line of the
  document. A `~~~` line without a blank line above it is **not** a card-yaml
  opener — it is treated as an ordinary CommonMark fenced code block.

### 3.2.1 Root-Block `---` Alias

The **root block only** may open with `---` and close with `---` instead of
`~~~` / `~~~`. A `---`-fenced root parses identically to a `~~~`-fenced root
with the same payload.

- **Accept, don't emit, don't advertise.** Parsers accept the `---` form so
  that LLMs trained on broader-internet YAML-frontmatter conventions are
  not penalised on a stylistic mismatch. `toMarkdown` (§9) always emits the
  canonical bare `~~~` shape; a `---`-authored document round-trips to the
  canonical form on first re-emit. Authoring surfaces (blueprints,
  FORMAT_RULES, examples) document only the canonical form.
- **Root only.** A `---` opener is recognised only when every line above
  it is blank (i.e. document start, modulo leading blank lines) and no
  prior block has been parsed. Any other `---` line is delegated to
  CommonMark as a thematic break or setext-heading underline.
- **Matched fences.** Within a single block, opener and closer must agree:
  a `---` opener requires a `---` closer, and a `~~~` opener (bare or
  `~~~card-yaml`) requires a `~~~` closer. Mixed forms (`---` … `~~~`,
  `~~~` … `---`) leave the opener unclosed, so it falls through to CommonMark
  (code block to EOF, or a thematic break for a lone `---`) rather than being
  recognised as a block.
- **Composable position.** A `---` line after the root block — when it
  pairs with a later `---` and has YAML-key content between — is a
  misplaced composable card and is rejected with a diagnostic that names
  the canonical `~~~` replacement (§10). Composable cards have no `---`
  alias.

### 3.3 System Metadata (`$`)

A block's YAML payload may contain **`$`-prefixed reserved keys** that carry
system metadata. The set is **closed**: only `$quill`, `$kind`, `$id`,
`$ext`, and `$seed` are accepted. Any other `$`-prefixed key is a parse error. These
keys are ordinary YAML — they are read by the same YAML parser that handles
the rest of the payload — but they are **extracted** from the user field
set after parsing; they are not part of the data model's field map (§3.4).

In the typed model, the `$` entries live as typed variants of the
unified payload-item list (`PayloadItem::Quill`, `PayloadItem::Kind`,
`PayloadItem::Id`, and `PayloadItem::Meta` keyed by `MetaKey::Ext` /
`MetaKey::Seed`), interleaved in
source order with user fields and YAML comments. They are surfaced through typed
accessors — `card.quill()`, `card.kind()`, `card.id()`, `card.ext()`,
`card.seed()` — which return `Option<…>`. On a successfully parsed document the root
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
  `main` is **reserved for the document root**: the root block's `$kind` is
  `main` by position. An explicit `$kind: main` is accepted (round-trips
  byte-equal); omitting it is also accepted and synthesised at parse time.
  A non-`main` `$kind` on the root is a parse error. No composable card may
  declare `$kind: main`.
- **`$id: <value>`** — an opaque, optional identifier. Plain metadata: no
  validation, no uniqueness requirement; carried through round-trip
  unchanged.
- **`$ext: <mapping>`** — an opaque, optional **mapping** reserved for
  out-of-band extension data (UI editor state, agent annotations, …).
  Required to be a YAML mapping (object); scalars and sequences are
  rejected. Contents are carried verbatim through Markdown and storage
  DTO round-trips, and **never** appear in the plate JSON consumed by
  backends (§5). Bespoke consumers namespace their state inside the
  map — e.g. `$ext.editor.title`, the canonical slot for a per-card
  display name (an editor-side rename).
  An empty `$ext: {}` is preserved as a distinct, explicit declaration.
- **`$seed: <mapping>`** — an optional **mapping keyed by composable
  card-kind**, present on the **root block only**; a composable block carrying
  `$seed` is a parse error, exactly like `$quill`. Each entry is a *sparse
  overlay* — the user fields (plus an optional reserved `$body` string) that a
  newly-added card of that kind starts with, layered over the quill's
  schema-`example:` seed (`overlay › example › absent`). Required to be a YAML
  mapping; scalars and sequences are rejected. Like `$ext` it carries verbatim
  through Markdown and storage DTO round-trips and **never** appears in the
  plate JSON consumed by backends; unlike `$ext` the seeding layer interprets
  it. Overlays are validated advisorily by
  `Quill::validate` and never gate render. An empty `$seed: {}` is preserved.

- `$` metadata entries may appear at any position within the payload, and
  may be interleaved with data fields. The emitter preserves source order
  (see §9); newly constructed metadata that does not have a source-order
  is emitted in the canonical key order `$quill`, `$kind`, `$id`, `$ext`, `$seed`.
- A duplicate `$key` within a single block is a parse error (a YAML mapping
  cannot carry two entries under the same key).
- An unknown `$key` (anything outside `{quill, kind, id, ext, seed}`) is a parse
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

- **Field names.** Every field name matches `/^[A-Za-z_][A-Za-z0-9_]*$/`. The
  pattern excludes `$`, so a data field name can never collide with any
  `$`-prefixed system key. Lowercase is the canonical, recommended convention,
  but uppercase ASCII letters are accepted and preserved verbatim; case is
  significant, so `title` and `Title` are distinct fields.
- **Whitespace-only payload.** A block whose payload (after metadata
  extraction) is only whitespace yields an empty field set.
- **YAML comments.** Both own-line comments (`# …` on their own line) and
  inline comments (`field: value  # note`) are supported on data fields and
  round-trip through `toMarkdown`. Comments inside nested YAML values
  (arrays, maps) are also preserved: the pre-scan captures each nested
  comment with a structural path and the emitter re-injects it at the
  matching position.
- **The `!must_fill` tag.** `!must_fill` marks a data field as a placeholder
  awaiting user input and round-trips through emit. It is what
  `QuillConfig::blueprint` stamps into every Unendorsed cell — the canonical
  authoring placeholder — and a marker that survives into a rendered document
  is surfaced by `Quill::validate` as the non-fatal `validation::must_fill`
  warning (it never gates render). It applies both to a
  top-level field and to a leaf nested inside an object or an array element
  (e.g. `addr.street`, `recipients[0].name`); nested markers are recorded on
  the value tree and survive markdown, live-wire, and storage round-trips.
  `!must_fill` may be applied to scalars (string, integer, float, bool, null)
  and sequences; it is rejected on a mapping (tag the leaves, not the
  container). `!must_fill` may not be applied to a `$` metadata key. The marker
  is preserved only in **block style** — `key: !must_fill` at any depth. A
  marker written inside a **flow collection** (`{…}` / `[…]`) or on a **bare
  sequence element** (`- !must_fill`) cannot be round-tripped and is reported
  with a `parse::fill_marker_unsupported_position` warning (the value is kept,
  the marker is not); markers under YAML **anchors/merge keys** are likewise
  not preserved. `!must_fill` is the only fill tag: every other custom tag —
  `!include`, `!env`, and the former `!fill` spelling — is dropped with a
  `parse::unsupported_yaml_tag` warning; the scalar value is kept but the tag
  does not round-trip.

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

Quill names match `/^[a-z_][a-z0-9_]*$/`. Resolution of partial selectors to
concrete versions is performed by the quill registry; this spec fixes only
the surface syntax accepted on the `$quill` line.

## 4. Block Detection

A single detector runs over the line stream. A `~~~` line — a bare `~~~`, or
`~~~card-yaml` — opens a card-yaml block **iff** all of the following hold:

**D0 — Column zero.** The `~~~` opener has no leading spaces.

**D1 — Blank line above.** The `~~~` line is line 1 of the document, or the
line immediately above it is blank.

**D2 — Closing fence.** A matching `~~~` line at **column zero** appears
later in the document. An indented `~~~` line is payload (§3.2), never a
closer.

A `~~~` line that fails D0 (an indented opener) or D1 is **not** a card-yaml
opener; it is delegated to CommonMark, where an indented `~~~` is still a
valid fenced code block.

A `---` line opens the **root block** instead **iff** all of the following
hold (see §3.2.1):

**R1 — Document start.** No prior block has been parsed and every line above
the `---` line is blank.

**R2 — Closing `---`.** A matching `---` line appears later in the document.

YAML content between recognised fence markers is opaque to detection — a
`~~~` line inside an open block is part of that block's payload, not a new
opener (though the canonical payload never produces such a line). In
particular, an *indented* `~~~` inside the payload — e.g. a tilde code fence
embedded in a `|` block-scalar value — is payload by the column-zero closer
rule (D2). A *column-zero* `~~~` can never be block-scalar content (YAML
requires scalar content to be indented past its key), so the closer is
unambiguous. The same opacity applies to `---` lines inside an open
`---`-fenced root block.

Failure of D0, D1, or D2 delegates the `~~~` line to CommonMark (an unclosed
`~~~` opener becomes a code block to EOF, with a non-fatal unclosed-fence
warning). A document with no closed root block fails with `MissingQuill`
(§10). A `---` that fails R1 falls through to CommonMark unless it forms a
paired block with YAML content, which is rejected as a misplaced composable
card (§10).

### 4.1 Worked Example

```
~~~
$quill: resume@1.0.0
$kind: main
title: CV
~~~

Main body text.

***

A thematic break in prose stays a thematic break.

~~~
$kind: profile
name: "Alice"
~~~

Profile body.
```

The first `~~~` is the root block (line 1, D1 satisfied). The second opens a
`profile` card (blank line above). The `***` is an ordinary CommonMark
thematic break — card-yaml does not reserve any thematic-break syntax.

## 5. Data Model

Parsing yields the `Document` model, which serialises via
`to_plate_json` into the following wire shape for backend templates.
All system-metadata keys are `$`-prefixed; user payload fields live flat
at the root and cannot collide with metadata because field names exclude
the `$` sigil.

```typescript
interface PlateJson {
  $quill: string;         // quill name@version, from the root block $quill
  $body: object;          // root-block body as canonical RichText-JSON corpus (text/lines/marks/islands), not a markdown string
  $cards: Card[];         // zero or more cards, in document order
  [field: string]: any;   // root-block payload fields, flat
}

interface Card {
  $kind: string;          // card kind, matches /^[a-z_][a-z0-9_]*$/
  $body: object;          // card body as canonical RichText-JSON corpus, not a markdown string
  [field: string]: any;   // card payload fields, flat
}
```

- `$cards` is always present, possibly empty.
- Root-block fields and card-field names may collide freely; each card is its
  own scope.
- Body text is preserved verbatim — whitespace, line endings, and inline
  CommonMark are untouched by the splitter.
- `$body` (root and per-card) and every `richtext` payload field cross as
  canonical RichText-JSON corpus objects (`{ text, lines, marks, islands }`);
  markdown is a lossless projection of the corpus, not the wire form.

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

### 6.3 Limited or Out of Scope

The following are parsed where CommonMark or pulldown-cmark already
handles them, but produce limited or no Quillmark-specific output; fuller
support may come in a future revision:

- Images (`![alt](src)`) — the markup *is* rendered by the Typst backend as
  `#image("src", alt: "alt")`, with the alt text preserved as the output's
  accessibility alternate text. What remains future work is asset-resolver
  integration: `src` is emitted verbatim and resolved by the backend's
  virtual filesystem, with no dedicated asset-resolution layer yet.
- Math (`$…$`, `$$…$$`), footnotes, task lists, definition lists — not
  supported. In markdown body text `$` is literal; inside a `~~~` card-yaml
  payload `$` is reserved as the prefix for system-metadata keys (§3.3).
- HTML comments — accepted syntactically, not rendered (see §6.2).
- `<br>`, `<br/>`, `<br />` — follow the raw-HTML rule (non-rendering);
  authors use CommonMark-native hard breaks (trailing two spaces plus
  newline, or trailing `\\` plus newline).

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
| Field count per block | 1000 |
| Card count per document | 1000 |

Markdown block nesting depth (100) is enforced at import time by the
markdown→corpus parser (`Document::parse`); the Typst backend re-checks
at render as a backstop for corpora built without importing.

## 9. Emission Contract

`toMarkdown` always emits the **canonical form** of every block:

```
~~~
<payload items in source order>
~~~
```

That is: a bare `~~~` opener, the YAML payload (typed `$` system
metadata, user data fields, and YAML comments interleaved in source
order), and a `~~~` closer. The root block must declare `$quill`;
canonical emission also writes `$kind: main` on the root, synthesising
it when the input omitted the line (see §3.3). Composable cards must
declare `$kind: <kind>`. A document round-trips to this canonical
shape — fence markers and YAML quoting are normalised; the `~~~card-yaml`
alias and the `---`-fenced root alias (§3.2.1) both re-emit as bare `~~~`.
`!must_fill` tags and YAML comments
(own-line and inline, including those adjacent to `$` lines) survive the
round-trip.

Programmatically constructed metadata that does not have a source-order
emits in the canonical key order `$quill`, `$kind`, `$id`, `$ext`, `$seed` — the
typed mutators (`set_quill` / `set_kind` / `set_id` / `set_ext` / `set_seed`)
insert at these positions.

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

- The document has no recognised root block (`MissingQuill`). This covers an
  unclosed root fence: an unclosed `~~~` opener or a `---` opener with no
  matching `---` closer is delegated to CommonMark (§4) rather than erroring
  on its own, but with no closed root block the document still fails here.
- A `---` line in composable position (after the root block) that pairs
  with a later `---` and holds YAML-key content between — composable
  cards must use `~~~` fences (§3.2.1).
- The root block missing its `$quill` entry.
- The root block declaring a non-`main` `$kind` (an omitted `$kind` on
  the root is accepted and synthesised; only an explicit non-`main`
  value is rejected).
- A composable (non-root) block declaring `$quill`, or declaring
  `$kind: main` (which is reserved for the document root).
- A duplicate `$key` within a single block (caught by the YAML parser as a
  duplicate mapping key).
- An unknown `$key` outside the closed set `{quill, kind, id, ext, seed}`.
- An invalid `$quill` reference.
- A `$` metadata key whose value type is incompatible with the key.
- A data-field name failing `/^[A-Za-z_][A-Za-z0-9_]*$/`.
- Invalid YAML inside any block payload.
- Any §8 limit exceeded.

## 11. References

- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/)
- [GitHub Flavored Markdown](https://github.github.com/gfm/) — pipe tables
  and strikethrough.
