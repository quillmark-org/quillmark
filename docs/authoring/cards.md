# Cards

Quillmark supports composable, repeatable metadata blocks called *cards*. A
card is a `~~~` block that declares a typed structured record, paired
with the Markdown prose that follows it.

## Card Block Syntax

A card is a `~~~` block whose YAML payload declares `$kind: <kind>`
alongside its data fields. The Markdown after the closing `~~~` fence is the
card's body.

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

All card blocks are collected into the plate JSON's `$cards` array.

## Structural Rules

- A card block opens with a bare `~~~` (or longer tilde run) and closes with a
  tilde run at least as long as the opener. The canonical form is `~~~` / `~~~`.
  The legacy `~~~card-yaml` opener is still accepted on input but is
  non-canonical and re-emits as a bare `~~~`.
- A composable card block must declare a `$kind: <kind>` entry naming the
  card kind. The kind must match `[a-z_][a-z0-9_]*` and must not be `main`
  (reserved for the document root). Invalid examples: `BadCard`, `my-card`,
  `2nd_card`, `main`.
- Field names must match `[A-Za-z_][A-Za-z0-9_]*`. Lowercase is the canonical
  convention, but uppercase is accepted and preserved verbatim; only
  `$`-prefixed keys are reserved for system metadata and cannot be used as
  user fields.
- A blank line is required immediately above every `~~~` opener
  (unless the block is the very first line of the document). A `~~~`
  line without a blank line above it is treated as an ordinary code block.
  To write a literal fenced code block in prose, use a backtick fence (or a
  `~~~` fence with a language info string); a `~~~~` block is still a card.
- YAML comments round-trip through the canonical form. Own-line comments
  and inline trailing comments are preserved on both `$` metadata lines
  and data-field lines.

The document is positional: the **first** `~~~` block is the root
block, and it must declare a `$quill: <name>@<version>` metadata line. Every
subsequent block is a card.

## Card Body Content

Each card carries a `$body` value on the plate JSON containing the
Markdown between that card's closing `~~~` fence and the next block's
opening fence (or document end).

## Out-of-band Metadata (`$ext`)

A card may declare `$ext: <mapping>` — an opaque YAML map reserved for
state that belongs with the card but should not reach the rendered
output. The map round-trips through Markdown and the storage DTO but is
stripped from the plate JSON before backends see it, so template renders
are unaffected. Consumers namespace inside the map (`$ext.editor`,
`$ext.agent`, …) to avoid collisions when more than one tool carries
state on the same card. `$ext.editor.title` is the canonical slot for a
per-card display name (an editor-side rename).

```
~~~
$kind: indorsement
$ext:
  editor:
    title: "Cmdr's response"
from: ORG/SYMBOL
~~~
```

## Emission

See [card-yaml Blocks § Emission](card-yaml.md#emission).
