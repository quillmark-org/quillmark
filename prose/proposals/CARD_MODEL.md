# Card Model — Unified Card Vocabulary

> **Status**: Implemented
> **Supersedes**: `LEAF_REWORK.md`, `LEAF_KIND_INFOSTRING.md` — both deleted; this proposal consolidates and revises them.
> **Affects**: [MARKDOWN.md](../designs/MARKDOWN.md), [CARDS.md](../designs/CARDS.md), [SCHEMAS.md](../designs/SCHEMAS.md), [BLUEPRINT.md](../designs/BLUEPRINT.md), [QUILL.md](../designs/QUILL.md)

## 1. Core insight

A document's top-level metadata block and its inline structured records are
**the same kind of thing**. The current design names them apart — inline
records are "leaves", the top-level block is "frontmatter" — yet the
implementation already models both with one Rust type (`Leaf`) and one config
type (`LeafSchema`). The shared type serves the main block through
`Sentinel::Main`, which is why `Leaf::is_main()` exists: a `Leaf` that is not
a leaf.

The fix is **one noun**. Everything is a **card**. A document has exactly one
**main card** and zero or more **inline cards**. The main card *is* a card —
not a special non-card — and there is no separate "leaf" concept. Collapsing
to a single noun removes the umbrella-versus-member tension, makes `is_main()`
an honest predicate on `Card`, and deletes a concept instead of renaming one.

## 2. The card model

A document is composed of **cards**:

- **Main card** — exactly one. Carries the `QUILL` reference, the
  document-level fields, and the leading body prose. It is the document's
  entry point and has **no `KIND`**.
- **Inline cards** — zero or more, in document order. Each is opened by a
  `` ```card <kind> `` fenced code block, carries typed fields, and owns the
  body prose that follows it. Each has a `KIND`.

### 2.1 A card is head + body

Every card has two parts:

- **Head** — the fence and the YAML fields within it. For the main card the
  head is the `---/---` frontmatter; for an inline card it is the
  `` ```card <kind> `` fence and its YAML body.
- **Body** — the markdown prose *after* the head's closing fence, up to the
  next card or EOF.

The fence is the card's **head**, not the whole card. The closing `` ``` ``
(or closing `---`) terminates the *head*; the card continues through its
body. This holds identically in both positions — main card = `---` head +
leading body; inline card = `` ```card `` head + trailing body — which is the
point: one shape, named once.

## 3. Markdown syntax

### 3.1 Main card — unchanged

```markdown
---
QUILL: catalog@1.0
title: Spring Catalog
---
```

A `---/---` pair at the top of the document (line 1, or preceded by a blank
line) with `QUILL:` as the first body key. Unchanged from today: top-of-file
frontmatter is the universal markdown-ecosystem convention and stays put.

### 3.2 Inline card

````markdown
```card product
name: Widget
price: 19.99
```

Body prose for this card, up to the next card or EOF.
````

- A CommonMark fenced code block whose info string is exactly `card <kind>`.
- `<kind>` matches `[a-z_][a-z0-9_]*` and selects the schema.
- The fence body is YAML; the prose after the closing fence is the card body.
- Classification is purely lexical — the parser commits to card-handling on
  the `card` first token alone, before reading any body content. A missing
  kind token, an invalid kind token, or any extra info-string token is a hard
  parse error (a routing failure, not a classification ambiguity).

### 3.3 Worked example

`````markdown
---
QUILL: catalog@1.0
title: Spring Catalog
---

# Introduction

Welcome to the spring catalog.

```card product
name: Widget
price: 19.99
```

The Widget is our flagship product.

```card product
name: Gadget
price: 29.99
```
`````

## 4. Design rationale

### 4.1 One noun, not two

Earlier iterations searched for an umbrella term *over* "leaf" — so the main
could be "a card" while leaves stayed "leaves". That search was the symptom,
not the cure: the umbrella and the member are the same concept. With one word
there is no hierarchy to name — there is just **card**, with one distinguished
instance, the **main card**. "Is the main a leaf?" stops being a question; it
is a card, like every other card.

### 4.2 The info-string token is `card`, not a branded `quill`

`` ```quill <kind> `` would brand the syntax and is collision-free, but it
reintroduces an author-versus-internal vocabulary split: the token would say
`quill` while the type is `Card`, the data key is `CARDS`, and the config is
`cards`. The whole value of the simplification is **one vocabulary edge to
edge**, which requires the token to be `card`. A fenced-code info string is
infrastructure, not a marketing surface; branding belongs in the package
name, CLI, docs, and editor extension.

### 4.3 The shared `Card` type is load-bearing — keep it

An alternative was to drop the shared type: fold the main card's accessors
into `Document` and let `Card` mean only an inline card. Rejected — UI
consumers render the main card and the inline cards as **uniform card panels**
(fields plus a body editor). The shared `Card` type is what lets a UI iterate
`[main, ...cards]` uniformly, and the config layer already commits to this: a
single `CardSchema` describes both. Keep the shared type; the main card is the
card with the entry-point role.

### 4.4 "card" over "leaf"

Neither word is Quillmark-specific, so nothing brandable is lost. "Card" is
the better *signal*: it connotes a discrete unit of structured information (an
index card, a data card), whereas "leaf" connotes foliage or a tree node. For
a fence that holds a structured record, `card` is the more honest cue.

## 5. Quill.yaml — the `cards:` map

Replace the two top-level schema keys (`main:` and `leaf_kinds:`) with a
single `cards:` map — a flat namespace of card schemas, modeled on a program's
`main()` entry point:

```yaml
cards:
  main:                 # mandatory, reserved entry point — no KIND
    fields:
      title:
        type: string
  indorsement:          # an inline kind; the key is its KIND discriminator
    description: Chain of routing endorsements.
    ui:
      title: Routing Endorsement
    fields:
      from:
        type: string
      for:
        type: string
```

**Invariant.** `cards` is keyed by card name. The reserved name `main` is the
entry point: exactly one, mandatory, carries no `KIND`, never inline-placeable.
Every other key is an inline card kind whose key *is* its `KIND` discriminator.

This is one populated namespace — not a wrapper over two sub-keys, and not a
map that over-promises "every entry is a kind". Every entry is a *card*, and
the main card is one. It also drops a top-level key (one `cards:` instead of
`main:` + `leaf_kinds:`).

## 6. Reserved names

- **`main`** is reserved as a card-kind name. Defining an inline kind named
  `main` is a hard error.
- **`KIND`, `BODY`, `CARDS`** are output-only reserved field keys — the parser
  populates them; supplying any as an input field is a hard parse error.
- **`QUILL`** is the main card's sentinel: first body key of the frontmatter,
  may not carry a `!fill` tag.

## 7. Data model

```text
Document {
  QUILL: string         // template reference, from the main card
  BODY: string          // leading body prose, before the first inline card
  CARDS: Card[]          // inline cards, in document order
  [field]: any           // other main-card fields
}

Card {
  KIND: string          // inline-card kind, from the ```card <kind> info string
  BODY: string          // card body prose
  [field]: any           // other card fields
}
```

The shape is unchanged from the current `Leaf` model — only the vocabulary
moves. Templates address inline cards grouped by kind via `cards.<kind>[i]`,
preserving document order within each kind.

## 8. Vocabulary rename — all surfaces

| Surface | Current (`leaf`) | New (`card`) |
|---|---|---|
| Rust types | `Leaf`, `LeafSchema` | `Card`, `CardSchema` |
| Sentinel variant | `Sentinel::Leaf(kind)` | `Sentinel::Inline(kind)` |
| Data key | `LEAVES` | `CARDS` |
| Quill.yaml schema | `main:` + `leaf_kinds:` | single `cards:` map |
| Schema YAML output | `leaf_kinds` | `cards` |
| Markdown info string | `` ```leaf <kind> `` | `` ```card <kind> `` |
| Templates | `leaves.<kind>[i]` | `cards.<kind>[i]` |
| Runtime API | `Document::leaves()` | `Document::cards()` |
| Limits / error codes | `MAX_LEAF_COUNT`, `leaf_*` | `MAX_CARD_COUNT`, `card_*` |
| Diagnostic codes | `parse::deprecated_leaf_syntax`, `form::unknown_leaf_kind` | `parse::deprecated_card_syntax`, `form::unknown_card` |
| Design doc | `LEAVES.md` | `CARDS.md` |

`KIND`, `BODY`, and `QUILL` keep their names. `is_main()` survives as an
honest predicate on `Card`.

## 9. Legacy `---/CARD: …/---` path — retained

The first-party application is **in beta with real users** whose documents
contain `---/CARD: …/---` blocks. The legacy parser path is **kept** so those
documents keep parsing — back-compat for the document syntax is a deliberate
courtesy to beta users, not a transitional convenience.

Behaviour (unchanged in substance; vocabulary updated):

- A `---/---` block after the frontmatter whose first body key is `CARD:`
  parses as an inline card. The `CARD:` value supplies the kind.
- Parsing it emits a `parse::deprecated_card_syntax` warning.
- `Document::to_markdown()` rewrites it to the canonical `` ```card <kind> ``
  form on round-trip.

Reverting the vocabulary to "card" produces a happy alignment: the legacy
`CARD:` key and the new `card` model now **agree on the noun**. The deprecation
is purely about fence *shape* — `---/CARD: …/---` → `` ```card <kind> `` — no
longer a cross-vocabulary migration. The warning message should say so.

**Removal** is no longer pinned to a "Release N+1". Retire the legacy path only
when beta telemetry shows the `---/CARD:` form is no longer in active use.

## 10. API back-compat — open question

Retaining the legacy path protects users' **documents**. It does **not** by
itself protect a consumer's **code**: the §8 renames (`Leaf` → `Card`,
`LEAVES` → `CARDS`, `Document::leaves()` → `cards()`, the diagnostic-code
renames) are breaking changes to the binding API and the diagnostic surface,
and the beta app consumes those.

**Decision needed before implementation:** does back-compat extend to the API,
or only to the document syntax?

- *Document syntax only* — flip the API in one atomic change; the beta app
  updates its code in lockstep. Simplest; relies on the app being first-party
  and shipped together with the library.
- *API too* — ship deprecated aliases (`pub use Card as Leaf`, a `leaves()`
  method delegating to `cards()`, dual-emit old diagnostic codes) for one
  release. No flag-day for the app, at the cost of an alias layer to carry
  and later remove.

Recommendation: **document syntax only**. The app is first-party and ships
alongside the library, so an atomic API flip is cheaper than carrying aliases;
the legacy *document* path already covers the irreplaceable case — user
content that cannot be edited in lockstep. Confirm with the team.

## 11. What this proposal revises

- **`LEAF_REWORK.md` renamed `card` → `leaf`.** Reverted: the noun is `card`.
  The `leaf` rename created the `Leaf`-that-`is_main()` wart this proposal
  removes.
- **`LEAF_REWORK.md §7` scheduled the legacy path for deletion in "Release
  N+1".** Revised: the legacy path is retained for beta users; removal is
  telemetry-driven, not calendar-driven (§9).
- **Kept from `LEAF_KIND_INFOSTRING.md`:** the kind discriminator lives in the
  info string (`` ```card <kind> ``), not a body key — unchanged, good for
  LLM authoring.
- **New in this proposal:** the `cards:` Quill.yaml map (§5), which neither
  prior proposal addressed.

## 12. What this proposal does not claim

- **Third vocabulary pass.** This is `card` → `leaf` → `card`. Pre-release the
  churn is keystrokes, not risk, but the design docs have moved twice;
  implementation must update every doc in lockstep so no stale "leaf" survives.
- **Preview legibility is unchanged from the leaf rework.** An inline card
  still renders as a grey code block in GitHub/Obsidian, not a thematic break.
  The mitigation remains an editor extension, out of scope here.
- **`main` becomes a reserved kind name.** A document cannot have an inline
  card of kind `main`. This is the entry-point reserved-name tradeoff and is
  accepted.

## 13. Implementation checklist

Vocabulary rename (§8) across `crates/core`, `crates/quillmark`,
`crates/backends/typst`, `crates/bindings/{cli,python,wasm}`, fixtures, golden
files, and conformance probes. Specific structural work:

- `crates/core/src/document/` — `Leaf` → `Card`, `Sentinel::Leaf` →
  `Sentinel::Inline`, info-string token `leaf` → `card` in `fences.rs`.
- `crates/core/src/quill/config.rs`, `schema.rs`, `schema_yaml.rs` —
  `leaf_kinds:` + `main:` → the unified `cards:` map (§5), with `main`
  reserved-name enforcement.
- Emitter (`document/emit.rs`) — emit `` ```card <kind> ``; legacy round-trip
  target updated.
- Legacy path (`fences.rs`, `assemble.rs`) — retained; diagnostic code and
  message updated to `parse::deprecated_card_syntax` (§9).
- Resolve the §10 API back-compat question first — it determines whether an
  alias layer is part of the change.
- Docs: rename `LEAVES.md` → `CARDS.md`; update `MARKDOWN.md`, `SCHEMAS.md`,
  `BLUEPRINT.md`, `QUILL.md`, `INDEX.md`, and `docs/authoring/`.

## 14. References

- [MARKDOWN.md](../designs/MARKDOWN.md) — markdown specification
- [CARDS.md](../designs/CARDS.md) — data-model design (formerly `LEAVES.md`)
- [SCHEMAS.md](../designs/SCHEMAS.md) — `QuillConfig` schema model
- [CommonMark 0.31.2 §4.5](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)
