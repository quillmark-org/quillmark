# Plate Data Injection

> **Implementation**: `crates/backends/typst/src/`

## TL;DR

Plates get document data through a backend-injected virtual Typst package, not a template engine. Data flows in two stages: `Quill::compile_data()` produces validated, zero-filled JSON; `Backend::open()` applies backend-specific field transforms (markdown→markup, date parsing) before compilation.

## Overview

1. `Quill::compile_data()` coerces, validates, normalizes, and **zero-fills** the root-block fields — and each composable card's fields against its `card_kind` schema — into a plain JSON object: every absent schema field resolves to its authored value, else the schema `default:`, else its type-empty zero value. An incomplete document still renders (an absent or present-null field zero-fills; a `!must_fill` marker uses its suggested value or zero-fills); only a malformed one (a value that won't coerce/validate to its type) errors.
2. `Backend::open()` receives that JSON and performs backend-specific field transformations (markdown strings → Typst markup, date parsing) before compilation.

### Data Shape

- Document-level metadata uses `$`-prefixed keys: `$quill` (quill ref string), `$body` (root prose body), `$cards` (array of card objects)
- Each card object carries `$kind` (discriminator), `$body` (card prose body), and the card's user fields flat
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

- `data`: a backend-generated Typst dictionary **literal** of all fields — no runtime processing, no `__meta__` sentinel. The backend classified and transformed every field at generation time.
- Content fields are Typst content. Two schema shapes qualify (see `content_field_names`):
  - `contentMediaType: text/markdown` — a single markdown string converted in place.
  - `markdown[]` (`{type: array, items: {contentMediaType: text/markdown}}`) — each array element converted individually.
  Each non-empty converted value is emitted as a markup **block** binding (`#let _qm_cN = [ .. ]`) that `data` references; empty values stay plain strings.
- Date fields (`format: date-time`) are emitted as `datetime(year:, month:, day:)` constructors (date-only).
