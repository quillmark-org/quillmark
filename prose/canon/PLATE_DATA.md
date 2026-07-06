# Plate Data Injection

> **Implementation**: `crates/backends/typst/src/`

## TL;DR

Plates get document data through a backend-injected virtual Typst package, not a template engine. Data flows in two stages: `Quill::compile_data()` produces validated, zero-filled JSON in which content fields are canonical richtext corpus objects; `Backend::open()` generates the helper's `lib.typ`, lowering each corpus to Typst markup at codegen (and dates to `datetime(..)` constructors) — no per-field markdown re-parse.

## Overview

1. `Quill::compile_data()` coerces, validates, normalizes, and **zero-fills** the root-block fields — and each composable card's fields against its `card_kind` schema — into a plain JSON object: every absent schema field resolves to its authored value, else the schema `default:`, else its type-empty zero value. Content fields cross as canonical richtext corpus objects (coercion imports an authored markdown string to the corpus and re-canonicalizes an editor-supplied one). An incomplete document still renders (an absent or present-null field zero-fills; a `!must_fill` marker uses its suggested value or zero-fills); only a malformed one (a value that won't coerce/validate to its type) errors.
2. `Backend::open()` receives that JSON and generates the helper package. Content fields lower to Typst markup at codegen via `emit::emit_richtext`, dates become `datetime(..)` constructors; a direct `apply` path revalidates dates. There is no markdown-string transform.

### Data Shape

- Document-level metadata uses `$`-prefixed keys: `$quill` (quill ref string), `$body` (root prose body, a canonical richtext corpus object), `$cards` (array of card objects)
- Each card object carries `$kind` (discriminator), `$body` (card prose body, a corpus object), and the card's user fields flat
- User payload fields sit flat at the root next to the `$` keys; field names match `[a-z_][a-z0-9_]*` and therefore never collide with `$` metadata

## Typst Helper Package

The Typst backend injects a virtual package `@local/quillmark-helper:<version>` that exposes the JSON to plates and provides helpers.

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.title                  // plain field access
#data.at("$body")            // $body is automatically converted to content
#data.date                   // date fields are auto-converted to datetime
#for card in data.at("$cards") {
  if card.at("$kind") == "indorsement" {
    // ... per-kind handling using card.<field> and card.at("$body")
  }
}
```

The `$`-prefixed keys must be accessed via `.at("$...")` because Typst identifiers do not include `$`.

Helper contents (generated in `backends/typst/helper.rs` from `lib.typ.template`):

- `data`: a backend-generated Typst dictionary **literal** of all fields — no runtime processing, no `__meta__` sentinel. The backend classified and lowered every field at generation time, reading classification from the session's cached `SchemaMeta`.
- Content fields are Typst content, lowered from their corpus by `emit::emit_richtext`. Two schema shapes qualify (see `content_field_names`), both classified on `contentMediaType: application/quillmark-richtext+json`:
  - a scalar richtext field (`{type: object, contentMediaType: application/quillmark-richtext+json}`) — one corpus object, lowered in place.
  - `array<richtext>` (`{type: array, items: {type: object, contentMediaType: application/quillmark-richtext+json}}`) — each array element lowered individually.
  Each non-blank corpus is emitted as a markup **block** binding (`#let _qm_cN = [ .. ]`) that `data` references; a blank corpus stays an empty string literal.
- Date fields (`format: date-time`) are emitted as `datetime(year:, month:, day:)` constructors (date-only).
