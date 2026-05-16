# Cards

Quillmark supports inline metadata blocks for repeated structures called
*cards*.

## Card Block Syntax

A card is a fenced code block whose info string is `card <kind>`. The block's
content is the card's YAML data; the markdown after the closing fence is the
card's body.

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

Widget description.

```card products
name: Gadget
price: 29.99
```

Gadget description.
````

All card blocks are collected into the `CARDS` array.

## Legacy Syntax

The older metadata-fence syntax — a `CARD:` key inside a `---`/`---` pair — is
still accepted on input:

```markdown
---
CARD: products
name: Widget
price: 19.99
---

Widget description.
```

Both syntaxes parse identically. Round-tripping a document through
`toMarkdown` always emits the canonical ```` ```card ```` form, so legacy
fences are rewritten to fenced cards on the next save.

## Rules

- The card kind (`card <kind>`) must match `[a-z_][a-z0-9_]*`.
- `QUILL`, `CARD`, `BODY`, and `CARDS` are reserved and cannot be used as
  field names inside a card.
- A `card` fenced block must have a blank line above it to be recognized as a
  card; without one it is treated as an ordinary code block.
- `---` is reserved for metadata delimiters and cannot be used as a thematic
  break in body content.
- Invalid card-kind examples: `BadCard`, `my-card`, `2nd_card`.

## Card Body Content

Each card includes a `BODY` field containing the Markdown between that card's
closing fence and the next card (or document end).
