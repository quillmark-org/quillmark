# YAML Frontmatter

Quillmark documents begin with a YAML frontmatter block delimited by `---` markers. The `QUILL` key is required and names the format used to render the document.

```markdown
---
QUILL: my_format
title: My Document
author: Jane Doe
date: 2025-01-15
tags: ["important", "draft"]
---

# Document content starts here
```

## QUILL Key

`QUILL` must appear in the first (global) frontmatter block. If it is missing, parsing fails.

```markdown
---
QUILL: my_format
title: Document Title
---
```

### Version Selectors

Pin a specific version with `@version` syntax:

```markdown
---
QUILL: my_format@2.1
title: Document Title
---
```

| Syntax | Meaning |
|--------|---------|
| `format` | Latest version (default) |
| `format@latest` | Latest version (explicit) |
| `format@2` | Latest 2.x.x |
| `format@2.1` | Latest 2.1.x |
| `format@2.1.0` | Exact version 2.1.0 |

Quill names must match `[a-z][a-z0-9_]*` (lowercase letters, digits, and underscores; must start with a lowercase letter).

## Frontmatter Data Types

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

Object-valued fields must be schematized in `Quill.yaml` with `type: object` and a `properties:` map. Nesting beyond one level is not supported. See [Quill.yaml Reference: Field Types](../format-designer/quill-yaml-reference.md#field-types).

## Placeholder Fields (`!fill`)

A top-level field tagged `!fill` marks the value as a placeholder awaiting input. The tag round-trips through parsing and emit, so editors and bindings can detect and update placeholders without losing them.

```yaml
recipient: !fill
department: !fill Department Here
tags: !fill []
```

`!fill` is valid on scalars (string, number, bool, null) and sequences. It is rejected on mappings. Other custom YAML tags (`!include`, `!env`, …) are dropped with a warning.

## Reserved Field Names

`BODY` and `CARDS` are reserved and cannot be used in frontmatter — the parser rejects documents that include them. `BODY` holds the document's Markdown body; `CARDS` holds the array of card blocks.

## Rules Summary

- `QUILL` can only appear in the first (global) metadata block.
- `QUILL` and `CARD` cannot both appear in the same block.
- A document can have only one global metadata block.
- All metadata blocks after the first must carry a `CARD` directive (see below).

## Card Blocks

Additional `---`-delimited blocks embedded in the document body are **card blocks**. Each must declare `CARD: <type>`, where `<type>` matches `[a-z_][a-z0-9_]*`. All card blocks are collected into the `CARDS` array available to templates.

```markdown
---
CARD: indorsement
from: ORG/SYMBOL
for: ORG2/SYMBOL
---

Indorsement body text here.
```

See [Cards](cards.md) for details on card syntax and usage.

Frontmatter is coerced and validated against the schema declared in the Quill's `Quill.yaml` (`main.fields`). See the [Quill.yaml Reference](../format-designer/quill-yaml-reference.md) for field types and constraints.
