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

`BODY` and `LEAVES` are reserved as **output-only** fields: the parser
populates them, and supplying them as input keys is a hard parse error.
`BODY` holds the document's Markdown body; `LEAVES` holds the ordered list
of leaf records. `QUILL` is also reserved — it's the sentinel that names the
quill format. YAML tags such as `!fill` cannot decorate `QUILL` or `KIND`.

## Rules Summary

- `QUILL` is required and must be the first body key in the frontmatter
  block.
- The frontmatter block sits at the top of the document (after optional
  blank lines).
- Mid-document `---/---` is a CommonMark thematic break, not a metadata
  fence.

## Leaves

Inline structured records — *leaves* — use a `` ```leaf `` fenced code block
with `KIND:` as the first body key. See [Leaves](leaves.md) for the full
syntax.

Frontmatter is coerced and validated against the schema declared in the
Quill's `Quill.yaml` (`main.fields`). See the
[Quill.yaml Reference](../format-designer/quill-yaml-reference.md) for field
types and constraints.
