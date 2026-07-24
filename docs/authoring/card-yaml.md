# card-yaml Blocks

Quillmark documents carry structured metadata in **card-yaml blocks** —
explicitly delimited blocks that isolate YAML data from the surrounding
Markdown prose. The first such block (the *root block*) names the format used
to render the document; later blocks are composable [cards](#card-blocks).

```
~~~
$quill: my_format
$kind: main
title: My Document
author: Jane Doe
date: 2025-01-15
tags: ["important", "draft"]
~~~

# Document content starts here
```

## Block Structure

A card-yaml block has three parts, in order:

1. **Opening fence** — a bare `~~~` (three tildes, no info string). No leading
   indentation. The `~~~card-yaml` info string is also accepted on
   input as a non-canonical alias; it parses identically and re-emits as a bare
   `~~~`.
2. **YAML payload** — a standard YAML mapping. The reserved keys `$quill`,
   `$kind`, `$id`, `$ext`, and `$seed` carry system metadata (see below); every
   other key is a user-defined data field.
3. **Closing fence** — a tilde run at least as long as the opener. The canonical opener and closer are both `~~~`; a longer opener (e.g. `~~~~`) requires an equally long closer.

The unstructured Markdown body begins immediately after the closing `~~~`
fence and runs to the next opening fence or the end of the document.

A blank line is required immediately above every `~~~` opener,
*except* when the opener is the very first line of the document. A
`~~~` line without a blank line above it is **not** an opener — it is
treated as an ordinary code block.

Because every column-zero `~~~` block is a card-yaml block, writing a literal
fenced code block in prose requires the escape hatch: use a **backtick fence**
(or a `~~~` fence carrying a language info string, e.g. `~~~rust`). Adding more
tildes does not escape — a `~~~~` block is still a card (its closer must just
be at least as long). A `~~~` fence whose info string is anything other than
`card-yaml` stays an ordinary code block.

## System Metadata (`$`)

The block's YAML payload may contain up to five reserved `$`-prefixed keys.
After parsing, these keys are extracted from the user field set and exposed
on the block's typed metadata.

- **`$quill: <name>@<version>`** names the format used to render the
  document. The root block (the first block, identified by position) **must
  declare `$quill`** — it is the only required `$` entry. If the root block
  is missing `$quill`, parsing fails.
- **`$kind: <kind>`** identifies a card's kind. The root block's kind is
  `main` by position; `$kind: main` may be omitted or declared explicitly —
  any other value is a parse error. Every composable card must declare a kind
  matching `[a-z_][a-z0-9_]*` other than `main`.
- **`$id: <value>`** is an opaque, optional identifier — the durable card
  handle, carried through the round-trip. Unique per document: the first
  card carrying a given `$id` keeps it, and a later duplicate (or an empty
  `$id`) is dropped with a warning.
- **`$ext: <mapping>`** is an opaque YAML mapping reserved for out-of-band
  extension data — UI editor state, agent annotations, anything bespoke to a
  consumer that should not reach the rendered output. Round-trips through
  Markdown and the storage DTO; **never** appears in the plate JSON consumed
  by backends. The value must be a mapping (scalars and sequences are
  parse errors); an empty `$ext: {}` is preserved as a distinct, explicit
  declaration. Consumers namespace inside the map (`$ext.editor`, `$ext.agent`,
  …) to avoid collisions; `$ext.editor.title` is the canonical slot for a
  per-card display name (an editor-side rename).
- **`$seed: <mapping>`** is a **root-only** mapping of per-kind seed overlays,
  keyed by card-kind. Like `$ext` it round-trips through Markdown and storage
  but **never** appears in the plate JSON; the seeding layer interprets it (see
  [CARDS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/CARDS.md)
  "Per-kind Seed Overlays"). A composable card carrying `$seed` is rejected.

`$` metadata entries may appear anywhere in the block's payload (the
canonical emission puts them first, in the order `$quill`, `$kind`,
`$id`, `$ext`, `$seed`). Any other `$`-prefixed key is a parse error — the set
is closed.

### Version Selectors

Pin a specific version with `@version` syntax on the `$quill` line:

```
~~~
$quill: my_format@2.1
$kind: main
title: Document Title
~~~
```

A bare name selects the latest version; `@latest`, `@2`, `@2.1`, and `@2.1.0`
pin progressively tighter. The [Quill Versioning](../quills/versioning.md#how-authors-select-versions)
page owns the full selector semantics.

Quill names must match `[a-z_][a-z0-9_]*` (lowercase letters, digits, and
underscores; must start with a lowercase letter or underscore).

## Payload Data Types

The data payload (everything in the YAML mapping except the `$`-prefixed
metadata keys) is standard YAML.

**Strings:**
```yaml
title: Simple String
quoted: "String with special chars: $%^"
multiline: |
  This is a
  multiline string
```

**Numbers:**
```yaml
count: 42
price: 19.99
```

**Booleans:**
```yaml
published: true
draft: false
```

**Arrays:**
```yaml
tags: ["tech", "tutorial"]
# or
authors:
  - Alice
  - Bob
```

**Objects:**
```yaml
author:
  name: John Doe
  email: john@example.com
```

Object-valued fields must be schematized in `Quill.yaml` with `type: object`
and a `properties:` map; array-valued fields with `type: array` and an
`items:` element schema (e.g. `items: { type: string }`, or `items: { type:
object, properties: … }` for a list of objects). Nesting beyond one level is
not supported. See
[Quill.yaml Reference: Field Types](../quills/quill-yaml-reference.md#field-types).

Field names must match `[A-Za-z_][A-Za-z0-9_]*`. Lowercase is the canonical,
recommended convention, but uppercase is accepted and preserved verbatim (case
is significant). Only `$`-prefixed keys are reserved for system metadata.

## Comments

YAML comments are supported in the payload and round-trip through
`toMarkdown` — both own-line comments and inline comments:

```yaml
# An own-line comment.
title: My Document  # an inline comment
```

Comments adjacent to `$` metadata keys — own-line or inline — round-trip
identically to comments on data fields.

## Placeholder Fields (`!must_fill`)

A field tagged `!must_fill` marks the value as a placeholder awaiting input.
The tag round-trips through parsing and emit, so editors and bindings can
detect and update placeholders without losing them.

```yaml
recipient: !must_fill
department: !must_fill Department Here
tags: !must_fill []
addr:
  street: !must_fill        # nested leaf, inside an object
  city: Anytown
recipients:
  - name: !must_fill        # nested leaf, inside an array element
    role: lead
```

`!must_fill` is valid on scalars (string, number, bool, null) and sequences,
both at the top level and on leaves nested inside objects and array elements.
It is rejected on mappings (tag the leaves, not the container). `!must_fill`
is the only placeholder tag; every other custom YAML tag (`!include`, `!env`,
`!fill`) is dropped with a warning and the value kept.

Use **block style** for placeholders. A marker written inside a flow
collection (`addr: {street: !must_fill}`), on a bare sequence element
(`- !must_fill`), or under a YAML anchor/merge key is **not** preserved — the
flow and bare-element cases emit a `parse::fill_marker_unsupported_position`
warning so the loss is never silent.

## Card Blocks

Every block after the root is a *card* — a composable, repeatable record. A card
declares `$kind: <kind>` (matching `[a-z_][a-z0-9_]*`, never `main`) alongside its
data fields; the Markdown after its closing `~~~` fence is the card's body.

```
~~~
$quill: my_quill@1.0
$kind: main
title: Main Document
~~~

# Introduction

Some content here.

~~~
$kind: products
name: Widget
price: 19.99
~~~

Widget description.

~~~
$kind: products
name: Gadget
price: 29.99
~~~

Gadget description.
```

Each card is collected into the plate JSON's `$cards` array. Its body Markdown —
everything between that card's closing `~~~` fence and the next block's opener
(or document end) — is carried as the card's `$body` value.

Card kinds and their field schemas are declared in `Quill.yaml` under
`card_kinds`; see the
[Quill.yaml Reference](../quills/quill-yaml-reference.md#card_kinds-section).

## Emission

`toMarkdown` always emits the canonical block form — a bare `~~~`
opener, the `$` metadata lines in the canonical order `$quill`, `$kind`,
`$id`, `$ext`, `$seed`, the remaining data fields, and a `~~~` closer. The root
block emits `$quill` and `$kind: main` plus any `$id` / `$ext` / `$seed` it
declared (`$seed` is root-only); composable cards emit `$kind: <kind>` plus
any `$id` / `$ext` they declared. Fence markers,
key ordering, and YAML quoting are normalised; `!must_fill` tags and YAML
comments (own-line and inline trailing, including those adjacent to `$`
lines) survive the round-trip.

The payload is coerced and validated against the schema declared in the
Quill's `Quill.yaml` (`main.fields`). See the
[Quill.yaml Reference](../quills/quill-yaml-reference.md) for field
types and constraints.
