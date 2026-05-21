# Cards

Quillmark supports composable, repeatable metadata blocks called *cards*. A
card is a `~~~card-yaml` block that declares a typed structured record, paired
with the Markdown prose that follows it.

## Card Block Syntax

A card is a `~~~card-yaml` block whose YAML payload declares `$kind: <kind>`
alongside its data fields. The Markdown after the closing `~~~` fence is the
card's body.

```
~~~card-yaml
$quill: my_quill@1.0
$kind: main
title: Main Document
~~~

# Introduction

Some content here.

~~~card-yaml
$kind: products
name: Widget
price: 19.99
~~~

Widget description.

~~~card-yaml
$kind: products
name: Gadget
price: 29.99
~~~

Gadget description.
```

All card blocks are collected into the `CARDS` array.

## Structural Rules

- A card block opens with exactly `~~~card-yaml` and closes with exactly `~~~`
  (three tildes).
- A composable card block must declare a `$kind: <kind>` entry naming the
  card kind. The kind must match `[a-z_][a-z0-9_]*` and must not be `main`
  (reserved for the document root). Invalid examples: `BadCard`, `my-card`,
  `2nd_card`, `main`.
- `QUILL`, `CARD`, `BODY`, and `CARDS` are reserved and cannot be used as field
  names inside a card.
- A blank line is required immediately above every `~~~card-yaml` opener
  (unless the block is the very first line of the document). A `~~~card-yaml`
  line without a blank line above it is treated as an ordinary code block.
- YAML comments adjacent to `$`-prefixed metadata keys are accepted on parse
  but dropped from the canonical form. Comments on data fields round-trip
  normally.

The document is positional: the **first** `~~~card-yaml` block is the root
block, and it must declare a `$quill: <name>@<version>` metadata line. Every
subsequent block is a card.

## Card Body Content

Each card includes a `BODY` field containing the Markdown between that card's
closing `~~~` fence and the next block's opening fence (or document end).

## Emission

Round-tripping a document through `toMarkdown` always emits the canonical
`~~~card-yaml` / `$`-prefixed metadata lines first / remaining data fields /
`~~~` form. Fence markers, key ordering, and YAML quoting are normalised.
`!fill` tags and data-field comments survive the round-trip.
