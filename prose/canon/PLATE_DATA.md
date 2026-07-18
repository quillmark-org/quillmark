# Plate Data Injection

> **Implementation**: `crates/backends/typst/src/`

## TL;DR

Plates get document data through a backend-injected virtual Typst package, not a template engine. Data flows in two stages: `Quill::compile_data()` produces validated, zero-filled JSON in which content fields are canonical richtext content objects; `Backend::open()` generates the helper's `lib.typ`, lowering each content to Typst markup at codegen (and dates to `datetime(..)` constructors) — no per-field markdown re-parse.

## Overview

1. `Quill::compile_data()` coerces, validates, normalizes, and **zero-fills** the root-block fields — and each composable card's fields against its `card_kind` schema — into a plain JSON object: every absent schema field resolves to its authored value, else the schema `default:`, else its type-empty zero value. Content fields cross as canonical richtext content objects (coercion imports an authored markdown string to the content and re-canonicalizes an editor-supplied one). An incomplete document still renders (an absent or present-null field zero-fills; a `!must_fill` marker uses its suggested value or zero-fills); only a malformed one (a value that won't coerce/validate to its type) errors.
2. `Backend::open()` receives that JSON and generates the helper package. Content fields lower to Typst markup at codegen via `emit::emit_richtext`, dates become `datetime(..)` constructors; a direct `apply` path revalidates dates. There is no markdown-string transform.

### Data Shape

- Document-level metadata uses `$`-prefixed keys: `$quill` (quill ref string), `$body` (root prose body, a canonical richtext content object), `$cards` (array of card objects)
- Each card object carries `$kind` (discriminator), `$body` (card prose body, a content object), and the card's user fields flat
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
- Content fields are Typst content, lowered from their content by `emit::emit_richtext`. Two schema shapes qualify (see `content_field_names`), both classified on `contentMediaType: application/quillmark-content+json`:
  - a scalar richtext field (`{type: object, contentMediaType: application/quillmark-content+json}`) — one content object, lowered in place.
  - `array<richtext>` (`{type: array, items: {type: object, contentMediaType: application/quillmark-content+json}}`) — each array element lowered individually.
  Each non-blank content is emitted as a markup **block** binding (`#let _qm_cN = [ .. ]`) that `data` references; a blank content stays an empty string literal. A `richtext(inline)` field (`quillmark:inline: true`, classified by `inline_field_names`) instead lowers via `emit::emit_richtext_inline` to **pure inline** markup — the single `Para`'s content with no block terminator, so no `parbreak` — so the value composes in an inline slot (`par(..)`, a grid cell) without Typst's "parbreak may not occur inside of a paragraph" warning. A `plaintext` field rides the *same* media type (plus an editor-only `quillmark:plain: true`), so `content_field_names` classifies it identically and it lowers through this exact path — the codec differs only at authoring/coercion (literal `from_plaintext`), never at codegen.
- Date fields (`format: date-time`) are emitted as `datetime(year:, month:, day:)` constructors (date-only).
- `plaintext(field)`: the sanctioned content→`str` coercion. Where `data.<field>` is Typst **content**, `plaintext(field)` returns the content field's plain text — the content text with island slots stripped and marks dropped (the same projection pdfform lowers a richtext field to). It reads a generated `_qm-plaintext` literal keyed by schema address (`subject`, `refs.2`, `$cards.<kind>.<n>.<field>`); `""` for a blank field or an address with no content content. Use it when a plate/package needs a string (string ops, an `assert(type(item) == str)` consumer) for any content field (`richtext` or `plaintext`). Note the name collision: this Typst helper is distinct from the `plaintext` **field type** — the helper projects *any* content to a `str`, while the field type declares a field's content plain from the start.
