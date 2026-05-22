# card-yaml Blocks

Quillmark documents carry structured metadata in **card-yaml blocks** —
explicitly delimited blocks that isolate YAML data from the surrounding
Markdown prose. The first such block (the *root block*) names the format used
to render the document; later blocks are composable [cards](cards.md).

```
~~~card-yaml
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

1. **Opening fence** — exactly `~~~card-yaml` (three tildes plus the info
   string). No leading indentation. The info string alone identifies a
   metadata block.
2. **YAML payload** — a standard YAML mapping. The reserved keys `$quill`,
   `$kind`, and `$id` carry system metadata (see below); every other key is
   a user-defined data field.
3. **Closing fence** — exactly `~~~`.

The unstructured Markdown body begins immediately after the closing `~~~`
fence and runs to the next opening fence or the end of the document.

A blank line is required immediately above every `~~~card-yaml` opener,
*except* when the opener is the very first line of the document. A
`~~~card-yaml` line without a blank line above it is **not** an opener — it is
treated as an ordinary code block.

## System Metadata (`$`)

The block's YAML payload may contain three reserved `$`-prefixed keys.
After parsing, these keys are extracted from the user field set and exposed
on the block's typed metadata.

- **`$quill: <name>@<version>`** names the format used to render the
  document. The root block (the first block, identified by position) **must
  declare `$quill`** — it is the only required `$` entry. If the root block
  is missing `$quill`, parsing fails.
- **`$kind: <kind>`** identifies a card's kind. The root block must declare
  `$kind: main`; every composable card must declare a kind matching
  `[a-z_][a-z0-9_]*` other than `main`.
- **`$id: <value>`** is an opaque, optional identifier — plain metadata with
  no validation or uniqueness requirement, carried through the round-trip.

`$` metadata entries may appear anywhere in the block's payload (the
canonical emission puts them first, in the order `$quill`, `$kind`,
`$id`). Any other `$`-prefixed key is a parse error — the set is closed.

### Version Selectors

Pin a specific version with `@version` syntax on the `$quill` line:

```
~~~card-yaml
$quill: my_format@2.1
$kind: main
title: Document Title
~~~
```

| Syntax | Meaning |
|--------|---------|
| `format` | Latest version (default) |
| `format@latest` | Latest version (explicit) |
| `format@2` | Latest 2.x.x |
| `format@2.1` | Latest 2.1.x |
| `format@2.1.0` | Exact version 2.1.0 |

Quill names must match `[a-z][a-z0-9_]*` (lowercase letters, digits, and
underscores; must start with a lowercase letter).

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
and a `properties:` map. Nesting beyond one level is not supported. See
[Quill.yaml Reference: Field Types](../format-designer/quill-yaml-reference.md#field-types).

Field names must match `[a-z_][a-z0-9_]*`.

## Comments

YAML comments are supported in the payload and round-trip through
`toMarkdown` — both own-line comments and inline comments:

```yaml
# An own-line comment.
title: My Document  # an inline comment
```

Comments adjacent to a `$` metadata key (whether inline or own-line) are
accepted by the YAML parser but **not preserved** through emit — the
canonical form drops them.

## Placeholder Fields (`!fill`)

A top-level field tagged `!fill` marks the value as a placeholder awaiting
input. The tag round-trips through parsing and emit, so editors and bindings
can detect and update placeholders without losing them.

```yaml
recipient: !fill
department: !fill Department Here
tags: !fill []
```

`!fill` is valid on scalars (string, number, bool, null) and sequences. It is
rejected on mappings. Other custom YAML tags (`!include`, `!env`, …) are
dropped with a warning.

## Field-name Rules

User field names match `[a-z_][a-z0-9_]*`. Uppercase, hyphens, and the
`$` sigil are not allowed — the `$` prefix is reserved for system
metadata that the engine projects onto the plate JSON (`$quill`,
`$kind`, `$id`, `$body`, `$cards`). Keeping user fields lowercase
guarantees they can never shadow that metadata.

## Card Blocks

Every card-yaml block after the root block is a **card**. It must declare a
`$kind: <kind>` entry, where `<kind>` matches `[a-z_][a-z0-9_]*` and is not
`main`. All card blocks are collected into the `$cards` array on the
plate JSON available to templates.

```
~~~card-yaml
$kind: endorsement
from: ORG/SYMBOL
for: ORG2/SYMBOL
~~~

Endorsement body text here.
```

See [Cards](cards.md) for details on card syntax and usage.

## Emission

`toMarkdown` always emits the canonical block form — a `~~~card-yaml` opener,
the `$` metadata lines in the canonical order `$quill`, `$kind`, `$id`, the
remaining data fields, and a `~~~` closer. The root block emits both
`$quill` and `$kind: main`; composable cards emit `$kind: <kind>` plus any
`$id` they declared. Fence markers, key ordering, and YAML quoting are
normalised; `!fill` tags and data-field comments survive the round-trip.

The payload is coerced and validated against the schema declared in the
Quill's `Quill.yaml` (`main.fields`). See the
[Quill.yaml Reference](../format-designer/quill-yaml-reference.md) for field
types and constraints.
