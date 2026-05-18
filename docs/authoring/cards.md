# Cards

Quillmark supports composable, repeatable metadata blocks called *cards*. A
card is a `~~~card-yaml` block that declares a typed structured record, paired
with the Markdown prose that follows it.

## Card Block Syntax

A card is a `~~~card-yaml` block, optionally led by a `#@kind: <kind>`
metadata line. The block's YAML payload sits below the `#@` header; the
Markdown after the closing `~~~` fence is the card's body.

```
~~~card-yaml
#@quill: my_quill@1.0
title: Main Document
~~~

# Introduction

Some content here.

~~~card-yaml
#@kind: products
name: Widget
price: 19.99
~~~

Widget description.

~~~card-yaml
#@kind: products
name: Gadget
price: 29.99
~~~

Gadget description.
```

All card blocks are collected into the `CARDS` array.

## Structural Rules

- A card block opens with exactly `~~~card-yaml` and closes with exactly `~~~`
  (three tildes).
- A card block may begin with a `#@kind: <kind>` metadata line naming the card
  kind.
- The card kind (`#@kind` value) must match `[a-z_][a-z0-9_]*`. Invalid
  examples: `BadCard`, `my-card`, `2nd_card`.
- `QUILL`, `CARD`, `BODY`, and `CARDS` are reserved and cannot be used as field
  names inside a card.
- A blank line is required immediately above every `~~~card-yaml` opener
  (unless the block is the very first line of the document). A `~~~card-yaml`
  line without a blank line above it is treated as an ordinary code block.
- Comments are **not** supported on a `#@` header line itself. YAML
  comments are supported in the payload below it.

The document is positional: the **first** `~~~card-yaml` block is the root
block, and it must declare a `#@quill: <name>@<version>` metadata line. Every
subsequent block is a card.

## Card Body Content

Each card includes a `BODY` field containing the Markdown between that card's
closing `~~~` fence and the next block's opening fence (or document end).

## Emission

Round-tripping a document through `toMarkdown` always emits the canonical
`~~~card-yaml` / `#@` metadata header / payload / `~~~` form. Fence markers,
key ordering, and YAML quoting are normalised. `!fill` tags and payload
comments survive the round-trip.
