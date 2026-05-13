# Leaves

Quillmark supports inline structured-data records — *leaves* — for repeated
sections like product cards, indorsement chains, or experience entries. Each
leaf is a [CommonMark fenced code block](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)
with the info string `leaf` and a YAML body whose first key is `KIND:`.

## Leaf syntax

````markdown
---
QUILL: my_quill@1.0
title: Main Document
---

# Introduction

Some content here.

```leaf
KIND: products
name: Widget
price: 19.99
```

Widget description goes here, as Markdown prose, up to the next leaf or EOF.

```leaf
KIND: products
name: Gadget
price: 29.99
```

Gadget description.
````

Leaves with the same `KIND` are collected into an ordered array under
`leaves.<kind>`. The two `products` leaves above land at
`leaves.products[0]` and `leaves.products[1]` (template-side) and as two
entries in `data.LEAVES` (backend wire shape).

## Fence closure and nesting

Leaf fences follow CommonMark's run-length closure rules: the closer must
have at least as many backticks as the opener. To embed a fenced code block
inside a leaf body, open the leaf with a longer fence:

`````markdown
````leaf
KIND: example
caption: Hello world in Python
````

```python
print("hello")
```

More body prose for this leaf.
`````

Indented fences (0–3 leading spaces) are permitted, matching CommonMark.

## Rules

- `KIND:` must be the first key in the leaf body. Its value matches the
  pattern `[a-z_][a-z0-9_]*`.
- `BODY` is reserved inside a leaf — the parser populates it with the prose
  attached to the leaf, and supplying it as an input key is a hard error.
- `QUILL` is *not* reserved inside leaves — it's only meaningful in
  frontmatter.
- YAML tags such as `!fill` cannot decorate the sentinel keys (`QUILL`,
  `KIND`).
- Misspelt info strings (e.g. ` ```leef `) are just unknown-language code
  blocks; the parser ignores them. Misspelt `KIND:` inside a `leaf` fence
  is a schema-level error.

## Leaf body

Each leaf gets a `BODY` field containing the Markdown prose between the
leaf's closing fence and the next leaf's opening fence (or end of file).
The body is verbatim — no further fence detection happens inside it.
