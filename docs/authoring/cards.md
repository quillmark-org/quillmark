# Cards

Quillmark supports inline structured-data records — *cards* — for repeated
sections like product cards, indorsement chains, or experience entries. Each
card is a [CommonMark fenced code block](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)
whose info string is `card <kind>` and which carries a YAML body.

## Card syntax

````markdown
---
QUILL: my_quill@1.0
title: Main Document
---

# Introduction

Some content here.

```card products
name: Widget
price: 19.99
```

Widget description goes here, as Markdown prose, up to the next card or EOF.

```card products
name: Gadget
price: 29.99
```

Gadget description.
````

Cards with the same `KIND` are collected into an ordered array under
`cards.<kind>`. The two `products` cards above land at
`cards.products[0]` and `cards.products[1]` (template-side) and as two
entries in `data.CARDS` (backend wire shape).

## Fence closure and nesting

Card fences follow CommonMark's run-length closure rules: the closer must
have at least as many backticks as the opener. To embed a fenced code block
inside a card body, open the card with a longer fence:

`````markdown
````card example
caption: Hello world in Python
````

```python
print("hello")
```

More body prose for this card.
`````

Indented fences (0–3 leading spaces) are permitted, matching CommonMark.

## Rules

- The info string must be exactly `card <kind>`. The kind matches the
  pattern `[a-z_][a-z0-9_]*`. A missing kind token, an invalid kind
  token, or any extra info-string token is a hard parse error.
- `BODY` and `KIND` are reserved inside a card — the parser populates them
  (`BODY` with the attached prose, `KIND` from the info-string kind
  token), and supplying either as an input body key is a hard error.
- `QUILL` is *not* reserved inside cards — it's only meaningful in
  frontmatter.
- YAML tags such as `!fill` cannot decorate the `QUILL` sentinel key.
- Misspelt info strings (e.g. ` ```caard `) are just unknown-language code
  blocks; the parser ignores them. A `` ```card `` fence with a missing or
  malformed kind token, however, is a hard parse error.

## Card body

Each card gets a `BODY` field containing the Markdown prose between the
card's closing fence and the next card's opening fence (or end of file).
The body is verbatim — no further fence detection happens inside it.
