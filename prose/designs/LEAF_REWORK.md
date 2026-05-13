# LEAF Rework — Quillmark Markdown Inline Records

> **Status**: Draft proposal
> **Targets**: future revision of [MARKDOWN.md](MARKDOWN.md), [CARDS.md](CARDS.md)
> **Supersedes**: ad-hoc design-vector discussion

## 1. Core insight

Today's syntax overloads the `---/---` fence to do two different jobs:

1. **Document-level frontmatter**, at the top of the file, naming the quill.
2. **Inline structured-data records** (`---/CARD: …/---`), embedded in prose.

That overload is the source of most of the parser's complexity and most of
the ecosystem-friction bugs:

- F1 must peek inside YAML to classify whether a `---/---` pair is metadata
  or a thematic break.
- `parse::near_miss_sentinel` exists *because* `Card:` looks like `CARD:`
  and the cost of misclassifying is silent failure.
- Prettier reformats mid-document `---/---` pairs into ordinary thematic
  breaks, mangling inline records.
- YAML scalar values can contain literal `---`, requiring careful matching
  rules.

The fix is to split the two roles at the syntax layer. Frontmatter — which
the entire markdown ecosystem already knows — stays as `---/---` at the top
of the file. Inline records move to a CommonMark fenced code block with the
info string `leaf`. The resulting asymmetry reflects a real, external
constraint (frontmatter is the universal top-of-file metadata convention;
inline `---/---` is the position Prettier and CommonMark contention attack)
rather than an arbitrary design choice.

## 2. Final design

### 2.1 Frontmatter (unchanged from today)

```markdown
---
QUILL: my_quill@1.0
title: Annual Report
---
```

- Position: line 1 of the document, or preceded by a blank line.
- Recognised by every markdown ecosystem tool (GitHub, Obsidian properties,
  Jekyll, MkDocs, Hugo, Docusaurus).
- Carries the `QUILL:` reference plus any document-level fields.

### 2.2 Leaves (inline records — new)

```markdown
```leaf
KIND: product
name: Widget
price: 19.99
```

Body prose for this leaf, terminating at the next leaf fence or EOF.
```

- A CommonMark fenced code block with info string `leaf`.
- Body inside the fence is YAML; first key `KIND:` discriminates the record
  type (matches `[a-z_][a-z0-9_]*`).
- Prose following the closing fence is the leaf's body content, up to the
  next leaf fence or EOF.

### 2.3 Worked example

```markdown
---
QUILL: catalog@1.0
title: Spring Catalog
---

# Introduction

Welcome to the spring catalog.

```leaf
KIND: product
name: Widget
price: 19.99
```

The Widget is our flagship product.

```leaf
KIND: product
name: Gadget
price: 29.99
```

The Gadget complements the Widget.
```

## 3. Design rationale

### 3.1 Why `leaf` info string and not `yaml`

`yaml` was considered for the IDE-tooling win (editors highlight YAML inside
` ```yaml ` blocks out of the box). Rejected because:

- **Lexical classification fails.** `KIND:` inside a `yaml` block can't be
  distinguished from an illustrative YAML code sample without peeking — the
  same F1 problem we're trying to eliminate.
- **Reserved body keys leak globally.** `KIND:` would become reserved
  inside *every* `yaml` block in the document, including illustrative ones
  in Quillmark's own documentation.
- **Prettier touches body content.** `yaml` is a known Prettier language;
  it normalizes quoting, indentation, and trailing commas. Authors expect
  data records to round-trip verbatim.

`leaf` is unfamiliar to existing tools, but:

- Prettier and other formatters pass unknown info strings through verbatim.
- Quillmark's own VSCode (and equivalent) extension can inject YAML grammar
  + the Quillmark schema into `leaf` fences via standard language-injection
  features — the same mechanism that lights up JS/CSS inside ` ```html `.
- LLMs and human readers see `leaf` as a structural signal rather than
  "generic config block."
- The reserved body key (`KIND:`) is scoped — it only matters inside
  `leaf` fences, so Quillmark documentation can show ` ```yaml ` examples
  freely.

### 3.2 Why `KIND:` as a body key and not an info-string token

A natural alternative was ` ```leaf product ` — second-token in the info
string carries the kind. Rejected because:

- **Asymmetry with frontmatter.** Frontmatter has no info-string slot; its
  discriminator `QUILL:` lives as a body key. Putting the leaf kind in the
  info string forces two different conventions for "what kind of fence is
  this." Keeping `KIND:` as a body key gives a single shared rule —
  *the first body key names the fence's role* — applied in both positions.
- **YAML semantics for the discriminator.** `KIND:` is a YAML scalar with
  full YAML machinery (comments, validation, schema integration). An
  info-string token is a lexer artifact.

### 3.3 Why this doesn't reintroduce F1

The classical F1 problem was *classification* via content peek: "is this
`---/---` pair metadata or a thematic break? must read the YAML to know."

With this design, classification is purely lexical:

- ` ```leaf ` opens — the parser commits to leaf parsing on the info string
  alone, before reading any body content.
- `---/---` at the top of the document opens — the parser commits to
  frontmatter on position alone (line 1 or preceded by blank line).

Reading `KIND:` from a leaf body is *routing* (which leaf-kind schema to
apply, where to file it in the `leaves.X[]` map), not classification. A
malformed leaf — one with the `leaf` info string but no `KIND:` first key
— is a hard error with a specific diagnostic, not a fence-classification
ambiguity.

### 3.4 Symmetries we get, asymmetries that remain

**Symmetric** (the shared rule):
- Outer marker opens a fence; inside, the **first body key names the
  fence's role** (`QUILL:` at top, `KIND:` inline).

**Asymmetric — fence shape** (externally forced):
- `---/---` at top, ` ```leaf ` inline. Different markers because top of
  file is bound to the universal frontmatter convention and mid-document
  needs to escape Prettier + thematic-break + YAML-scalar contention.

**Asymmetric — discriminator semantics** (intrinsic, can't be unified):
- `QUILL:` is a *template binding* (foreign-key reference to a quill).
- `KIND:` is a *record-type discriminator* within the already-bound
  template.
- Same syntactic shape; different semantic roles. Don't try to paper over
  this with shared naming — the proposal keeps the names distinct on
  purpose.

## 4. Vocabulary

The proposal renames `card` → `leaf` to align the user-facing concept
with the syntax:

- Code (Rust, bindings): `Card` → `Leaf`, `CardSchema` → `LeafSchema`,
  `card_types` → `leaf_kinds`.
- Quill.yaml schema: `card_types:` → `leaf_kinds:`.
- Templates: `cards.X[i].field` → `leaves.X[i].field`.
- Output data: `data.CARDS` → `data.LEAVES`.

This is a separable decision from the syntax change — the syntax could
ship with `CARDS`/`card_types` vocabulary preserved. The proposal couples
them because the info string `leaf` becomes the canonical user-facing
name, and divergent internal vocabulary would be a long-term confusion
tax. See §7 for migration cost.

## 5. Reserved keys

Scoped to the fence they appear in:

| Position | Reserved keys |
|---|---|
| Frontmatter body | `QUILL`, `BODY`, `LEAVES` |
| Leaf body | `KIND`, `BODY` |

`QUILL` is not reserved inside leaves; `KIND` is not reserved inside
frontmatter. Legacy `CARD`/`CARDS` accepted as aliases during the
deprecation window (§7).

## 6. Data model

```text
Document {
  QUILL: string             // template reference, from frontmatter
  BODY: string              // body prose between frontmatter and first leaf
  LEAVES: Leaf[]            // zero or more leaves, in document order
  [field: string]: any      // other frontmatter fields
}

Leaf {
  KIND: string              // matches /^[a-z_][a-z0-9_]*$/
  BODY: string              // leaf body prose
  [field: string]: any      // other leaf fields
}
```

Templates access leaves grouped by kind (`leaves.product[0].name`) and
the parser preserves document order within each kind, mirroring today's
`cards.product[i]` semantics — only the vocabulary changes.

## 7. Migration

Migration is mechanical but invasive. Honest scope:

**Document format** — dual-support during deprecation window.
- Legacy `---/CARD: foo/---` parses unchanged and emits a
  `parse::deprecated_leaf_syntax` warning.
- Round-trip through emit converts to the ` ```leaf / KIND: foo / ``` `
  form (lossless, comments preserved).
- After the deprecation window, the legacy form errors with a clear
  pointer to the migration tool.

**Quill.yaml** — rename `card_types:` → `leaf_kinds:`. Field shapes
below are unchanged.

**Templates** — `cards.X` → `leaves.X`. Mechanical sweep.

**Rust code** — `Card*` types rename to `Leaf*`; field names track.
Spot check at the time of this proposal: 22+ files in `crates/` reference
`CARD`/`CARDS`/`card_types`.

**Bindings** — Python and WASM both expose `CARDS` in their public APIs.
The migration is a breaking API change for binding consumers;
dual-emission with a deprecation cycle reduces the blast radius but does
not eliminate it.

**Backends** — the typst backend consumes `data.CARDS`. Backends maintained
in-tree migrate in lockstep with the rename; external backends consume
both names through the deprecation window.

**Diagnostics** — path anchors that today reference `CARDS[i].field`
re-map to `LEAVES[i].field`. The recent document-model path anchor work
(c.f. commit `78ec6ca`) is the surface area affected.

## 8. Parser behaviour

The fence-detection logic in `crates/core/src/document/fences.rs`
collapses significantly. Today the file implements F1 (content-peek
sentinel) + F2 (leading blank) + F3 (indent) + near-miss-sentinel
diagnostic + reserved-key disambiguation.

After this change:

- **Frontmatter detection** keeps F2 (top-of-file or preceded by blank)
  and F3 (zero-to-three space indent). The F1 sentinel check
  simplifies to "first body key is `QUILL:` — fail with a specific
  error if not." There is no `CARD`-vs-`QUILL` branching because
  `---/---` only ever means frontmatter now.
- **Leaf detection** delegates to CommonMark's existing fenced-code-block
  rules. Quillmark only inspects fenced code blocks whose info string's
  first token is `leaf`.
- **Near-miss-sentinel diagnostic** for inline records is gone. The
  closest analogue inside a `leaf` fence is "missing `KIND:` first key,"
  which is a hard error rather than a silent classification miss.

The `MetadataBlock` shape in `assemble.rs` unifies frontmatter and leaf
into a single `Block { sentinel: BlockKind, fields, body }` shape, with
`BlockKind = Main(QuillRef) | Leaf(kind)`.

## 9. What we don't claim

In the spirit of honest accounting:

- **GitHub/Obsidian preview legibility for leaves is *worse* than today**,
  not better. ` ```leaf ` renders as a grey code block; today's
  `---/CARD/---` rendered as a thematic break with YAML-as-paragraph. If
  raw-`.md`-on-GitHub readability is important to a use case, this
  design pessimises it. The mitigation is the VSCode/IDE plugin path,
  not GitHub.
- **Some IDEs lose YAML language support inside leaves** until the
  Quillmark extension is installed. The plugin is required to close the
  gap, not optional.
- **`F1/F2/F3 collapse` is partial.** F2 + F3 still apply at the top of
  file for frontmatter detection. CommonMark's fenced-code-block rules
  apply for leaf detection. Net rule count shifts from one custom system
  to (smaller custom system + already-implemented CommonMark rules) —
  net engineering win, but not a clean collapse.
- **Migration is real engineering work.** "Mechanical sweep" is accurate
  for the renames but does not include the binding-API breakage, backend
  contract updates, dual-parser maintenance window, or downstream
  template breakage. Plan for one minor-version cycle of dual support
  minimum.

## 10. What survives the design

- **YAML-scalar-`---` ambiguity** — gone. Code-fence closure uses run-length
  matching, not delimiter equality.
- **`parse::near_miss_sentinel`** for inline records — gone. Misspelt
  ` ```leef ` is just a code block with an unknown info string; misspelt
  `KIND:` inside a `leaf` fence is a specific schema diagnostic.
- **Prettier round-trip damage** for inline records — gone. Unknown
  info-string fenced code blocks are verbatim to Prettier.
- **Thematic-break collision** for inline records — gone. `---` is no
  longer overloaded mid-document.
- **YAML comments** — preserved in both positions (same YAML parser).
- **Frontmatter ecosystem interop** — fully preserved (GitHub, Obsidian,
  Jekyll, MkDocs, Hugo, Docusaurus).
- **Single shared mental model** — "first body key names this fence's
  role" applies at top and inline.

## 11. Open questions

1. **Vocabulary rename scope.** Adopt the full `card` → `leaf` rename
   (templates, bindings, backends) or scope it to syntax only and keep
   `CARDS`/`card_types` internally? §4 recommends the full rename; the
   alternative cuts migration cost by ~half at the price of a permanent
   syntax-vs-internals naming split.

2. **Deprecation window length.** One minor version (e.g. 0.78 → 0.79)
   or two? Longer window reduces user friction; shorter window reduces
   parser maintenance cost.

3. **`yaml`-info-string acceptance as a future alias.** Reject for now
   (per §3.1). Could be added later if the IDE-tooling gap proves harder
   to close than expected — but only as `yaml` with a mandatory second
   token (` ```yaml leaf `) to preserve lexical classification.

4. **`leaf` validation pattern.** Today's `[a-z_][a-z0-9_]*` is preserved
   for `KIND:` values; should the info-string second token (if ever
   added per (3)) follow the same rule, or accept a broader identifier
   shape?

## 12. References

- [MARKDOWN.md](MARKDOWN.md) — current specification (to be revised)
- [CARDS.md](CARDS.md) — current data model (to be revised)
- [SCHEMAS.md](SCHEMAS.md) — schema model, affected by §4 rename
- [CommonMark 0.31.2 §4.5](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)
  — fenced code block rules this design relies on
- `crates/core/src/document/fences.rs` — fence-detection implementation
  that simplifies per §8
