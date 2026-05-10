# WASM Migration Guide

> **Historical.** Migration guide for `@quillmark/wasm` consumers crossing the
> 0.54 → 0.58 boundary (the **Canonical Document Model** refactor, commit
> `f8c7ee3`, PR #444). Kept for consumers still upgrading from pre-0.58
> versions. For the current API surface, see the docs site and the
> `@quillmark/wasm` package README.

Previous in-tree migration notes lived in `MIGRATION.md`, which was deleted as
part of this refactor. This document replaces them for the WASM surface only.

## TL;DR

```diff
- import { ParsedDocument, Quillmark } from "@quillmark/wasm";
+ import { Document, Quillmark } from "@quillmark/wasm";

  const engine = new Quillmark();
  const quill  = engine.quill(tree);

- const parsed = ParsedDocument.fromMarkdown(md);
- const title  = parsed.fields.title;
- const body   = parsed.fields.BODY;
- const cards  = parsed.fields.CARDS;
+ const doc    = Document.fromMarkdown(md);
+ const title  = doc.frontmatter.title;   // no QUILL / BODY / CARDS
+ const body   = doc.body;                // string (never undefined)
+ const cards  = doc.cards;               // array<{tag, fields, body}>

- const result = quill.render(parsed, { format: "pdf" });
+ const result = quill.render(doc,    { format: "pdf" });
```

There is **no compatibility alias**. `ParsedDocument` is gone from the exports
(`crates/bindings/wasm/src/lib.rs:36`). Consumers must rename.

---

## 1. `ParsedDocument` → `Document`

### Rename

| Before | After |
| --- | --- |
| `import { ParsedDocument } from "@quillmark/wasm"` | `import { Document } from "@quillmark/wasm"` |
| `ParsedDocument.fromMarkdown(md)` | `Document.fromMarkdown(md)` |

### Shape change: `fields` → `frontmatter` + `body` + `cards`

The old `ParsedDocument.fields` was a single flat object that included the
reserved keys `BODY` and (when present) `CARDS` alongside user frontmatter. The
new `Document` splits these into typed accessors:

| Before (flat `fields`)         | After (typed getters)                                   |
| ------------------------------ | ------------------------------------------------------- |
| `parsed.fields.title`          | `doc.frontmatter.title`                                 |
| `parsed.fields.BODY`           | `doc.body` — always a string (empty when absent)        |
| `parsed.fields.CARDS`          | `doc.cards` — always an array (empty when absent)       |
| `parsed.fields.QUILL`          | not in `frontmatter`; use `doc.quillRef`                |
| `parsed.quillRef`              | `doc.quillRef` (unchanged)                              |
| `parsed.warnings`              | `doc.warnings` (unchanged)                              |

`doc.frontmatter` **never** contains `QUILL`, `BODY`, or `CARDS`. Checking for
those keys in `frontmatter` always yields `undefined`.

### Shape change: `doc.cards[i]`

Each element is `{ tag: string, fields: Record<string, unknown>, body: string }`.
The `tag` reflects the card's `CARD:` sentinel value, not a reserved `CARD` key
inside `fields`.

```js
doc.cards[0].tag       // "note"
doc.cards[0].fields    // { foo: "bar" }   — no CARD key
doc.cards[0].body      // "Card body..."   — string, may be ""
```

### `Document` is now an opaque WASM handle, not a serialized plain object

This is the **subtlest** behavioural change and the one most likely to bite.

`ParsedDocument` used to round-trip through `serde-wasm-bindgen` as a plain JS
object — you could spread it, `JSON.stringify` it, and pass the same value to
`quill.render` multiple times. `Document` is a real `#[wasm_bindgen]` class
(`crates/bindings/wasm/src/engine.rs:54`). That has two consequences:

**a. Reading fields goes through getters.** `doc.frontmatter`, `doc.body`,
   `doc.cards`, `doc.warnings`, `doc.quillRef` are all getters that allocate
   and deserialize on every access. If you read them in hot loops, cache the
   value locally.

**b. `quill.render(doc)` and `quill.open(doc)` borrow the handle.**
   Both take `&Document`, so the JS reference remains usable after the call.
   Render the same parse as many times and as many formats as you like:
   ```js
   const parsed = Document.fromMarkdown(md);
   const pdf = quill.render(parsed, { format: "pdf" });
   const svg = quill.render(parsed, { format: "svg" });
   ```

   The `opts` argument is optional. Omitting it uses the quill's default
   output format (as declared in `Quill.yaml`):
   ```js
   const result = quill.render(parsed);  // default format
   ```

   Use `quill.open(doc)` when you want a single compilation that serves
   multiple page-selective renders:
   ```js
   const session = quill.open(parsed);
   const page1 = session.render({ format: "png", pages: [0], ppi: 300 });
   const all   = session.render({ format: "pdf" });
   ```

---

## 2. New editor surface on `Document`

`Document` now supports in-place mutation. Every mutator enforces the parser's
invariants and throws `EditError` (as a JS `Error` whose message starts with
`[EditError::<Variant>]`) on violations:

| Method                                        | Purpose                                    |
| --------------------------------------------- | ------------------------------------------ |
| `setField(name, value)`                       | Insert or replace a frontmatter field      |
| `removeField(name)`                           | Remove a frontmatter field (returns it)    |
| `setQuillRef(refString)`                      | Replace the `QUILL` reference              |
| `replaceBody(body)`                           | Replace the global Markdown body           |
| `pushCard({ tag, fields?, body? })`           | Append a card                              |
| `insertCard(index, { tag, fields?, body? })`  | Insert at `0..=cards.length`               |
| `removeCard(index)`                           | Remove and return the card (or `undefined`)|
| `moveCard(from, to)`                          | Reorder                                    |
| `setCardTag(index, newTag)`                   | Rename a card's tag in place               |
| `updateCardField(index, name, value)`         | Convenience: edit a card's field           |
| `removeCardField(index, name)`                | Remove a card's frontmatter field          |
| `updateCardBody(index, body)`                 | Convenience: replace a card's body         |

`EditError` variants surfaced to JS: `ReservedName`, `InvalidFieldName`,
`InvalidTagName`, `IndexOutOfRange`. Reserved frontmatter field names are
`BODY`, `CARDS`, `QUILL`, `CARD`. Field names must match `[a-z_][a-z0-9_]*`
(NFC); tag names must match the tag grammar from the parser.

`removeField` and `removeCardField` validate `name` symmetrically with
`setField` / `updateCardField`: passing a reserved or syntactically invalid
name throws (`ReservedName` / `InvalidFieldName`) rather than silently
returning `undefined`. Absence of an otherwise-valid name returns `undefined`.

`setCardTag` is a structural primitive: it mutates only the sentinel, leaving
the card's frontmatter and body untouched. Schema-aware migration (clearing
orphan fields, applying new defaults) is the caller's responsibility.

Mutators never modify `doc.warnings`; warnings remain a frozen record of the
original parse.

```js
const doc = Document.fromMarkdown(md);
doc.setField("title", "New title");
doc.pushCard({ tag: "note", fields: { author: "Alice" }, body: "Hello" });

try {
  doc.setField("BODY", "x");              // throws
} catch (e) {
  // e.message starts with "[EditError::ReservedName] ..."
}
```

---

## 3. New emitter: `doc.toMarkdown()`

`doc.toMarkdown()` returns canonical Quillmark Markdown. It is type-fidelity
round-trip safe:

```js
const doc2 = Document.fromMarkdown(doc.toMarkdown());
// doc2 equals doc by value AND by type variant.
```

This is the fix for the YAML "Norway" / numeric-string / date-string bug
family: strings are always double-quoted on emission, so `"on"`, `"off"`,
`"01234"`, `"2024-01-15"`, `"null"` all survive as strings through the
round-trip.

Use this when a form editor mutates a parsed document and needs to persist
back to `.md` on disk.

---

## 4. New: `quill.form(doc)` and blank constructors

Schema-aware projection for form editors. `quill.form(doc)` returns a plain
JSON-ready object (not a class) with the shape:

```ts
{
  main:  { schema: {...}, values: Record<string, FieldSource> },
  cards: Array<{ tag: string, schema: ..., values: ..., diagnostics: [...] }>,
  diagnostics: Diagnostic[],
}
```

Each `FieldSource` carries the value plus a discriminator
(`Document | Default | Missing`). It is a **snapshot** — subsequent mutations
on `doc` require calling `form` again.

This takes `&Document`, so the handle survives the call.

For "new document" flows there is no parsed `Document` yet, so use the blank
constructors:

- `quill.blankMain()` — returns a `{ schema, values }` projection for the
  main card with every field defaulted (or `Missing`).
- `quill.blankCard(tag)` — same shape for a named card-type, or `undefined`
  when the tag is not declared in the quill.

These return the same per-card object shape as entries in `form(doc).cards`
/ `form(doc).main`, so a UI that renders one can render the others.

---

## 5. Render options — `assets` field removed

`RenderOptions` shape on the wire (all fields optional):

```ts
{ format?: "pdf"|"svg"|"png"|"txt", ppi?: number, pages?: number[] }
```

The entire options object is optional. `quill.render(doc)` and
`session.render()` both accept `undefined` and fall back to the quill's
default output format.

Dynamic asset injection was removed from the pipeline in this refactor.
`RenderOptions.assets` was **deleted** from the WASM surface — it is no longer
part of the TypeScript type and passing it is now a type error at compile
time (or an unknown-property warning in plain JS).

**Migration:** move any assets or fonts you were injecting through
`RenderOptions.assets` into the quill tree you pass to `engine.quill(tree)`:

```diff
  const tree = new Map();
  tree.set("Quill.yaml", quillYamlBytes);
  tree.set("plate.typ", plateBytes);
+ tree.set("assets/logo.png", logoBytes);
+ tree.set("assets/fonts/MyFont-Regular.ttf", fontBytes);
  const quill = engine.quill(tree);
```

Assets and fonts travel through the file tree only.

---

## 6. Quick reference: full before/after

```js
// ── Before ────────────────────────────────────────────────────────────────
import { ParsedDocument, Quillmark } from "@quillmark/wasm";

const engine = new Quillmark();
const quill  = engine.quill(tree);

const parsed = ParsedDocument.fromMarkdown(md);
console.log(parsed.fields.title, parsed.fields.BODY);

const r1 = quill.render(parsed, { format: "pdf" });
const r2 = quill.render(parsed, { format: "svg" }); // was fine


// ── After ─────────────────────────────────────────────────────────────────
import { Document, Quillmark } from "@quillmark/wasm";

const engine = new Quillmark();
const quill  = engine.quill(tree);

const doc = Document.fromMarkdown(md);
console.log(doc.frontmatter.title, doc.body);

// Render the same document multiple times
const r1 = quill.render(doc, { format: "pdf" });
const r2 = quill.render(doc, { format: "svg" });

// Or open a session for page-selective output
const session = quill.open(doc);
const rPdf = session.render({ format: "pdf" });
const rPng = session.render({ format: "png", ppi: 300, pages: [0, 2] });
```

---

## Parse-time requirements that bite silently

These are intentional behaviors but surface as errors consumers did not see in
0.54. Listed here so migrators do not re-discover them from stack traces.

### `Document.fromMarkdown` now requires `QUILL:` in frontmatter

A top-level `QUILL: <name>` is a **parse-time** requirement on every input
document. In 0.54, missing-QUILL surfaced at render time; in 0.58+ it fails
inside `Document.fromMarkdown` with an `InvalidStructure` diagnostic whose
message is `Missing required QUILL field. Add `QUILL: <name>` to the
frontmatter`.

Empty / whitespace-only inputs surface a dedicated message instead:
`Empty markdown input cannot be parsed as a Quillmark Document. Provide at
least a QUILL frontmatter field: `QUILL: <name>`.`.

Fix: add `QUILL: <name>` to the frontmatter of every document you parse. Test
fixtures in particular rot silently — a fixture that used to render will now
throw on parse.

### `Quill.yaml` requires a nested `quill:` section

Flat top-level keys (`name:`, `backend:`, `description:` at the root) are not
supported and will not be. Every field lives under the top-level `quill:`
mapping (lowercase — previously capitalized `Quill:`, see note below):

```yaml
quill:
  name: my_quill           # required, snake_case
  backend: typst           # required
  description: My quill    # required, non-empty
  version: 0.1.0           # required, semver
  author: Alice            # optional, defaults to "Unknown"
```

`name`, `backend`, `description`, and `version` are all required — only
`author` has a default (`"Unknown"`). See
`crates/core/src/quill/config.rs:615-672` for the full parse.

> **Key name is `quill:` (lowercase).** Pre-0.58 drafts used `Quill:` with a
> capital Q to match the filename; the published 0.58 line uses lowercase
> `quill:` for YAML-idiom consistency. Capitalized `Quill:` is not accepted —
> rename the section key in each `Quill.yaml` on migration.

---

## Unchanged

The following are behaviorally unchanged by this refactor:

- `new Quillmark()` constructor.
- `engine.quill(tree)` where `tree` is `Map<string, Uint8Array>` or
  `Record<string, Uint8Array>` (plain object — normalized at the boundary).
- `quill.open(doc)` → `session.pageCount` + `session.render(opts)`.
- `quill.backendId` getter.
- `RenderResult` shape: `{ artifacts, warnings, outputFormat, renderTimeMs }`.
- `Diagnostic` shape: `{ severity, code?, message, location?, hint?, sourceChain? }`.
  `severity` is a lowercase string: `"error"`, `"warning"`, or `"note"`.
  `sourceChain` is absent (not serialised) when empty.
- QUILL-ref mismatch behaviour: `quill.render(doc)` with a mismatched
  `doc.quillRef` still emits a `quill::ref_mismatch` warning, not an error.
- npm package name and import path.

---

## Leftovers cleaned up alongside this migration

This migration pass also resolved stale references to the removed APIs:

- **`RenderOptions.assets`** — deleted from `crates/bindings/wasm/src/types.rs`.
  The TypeScript type no longer exposes it. Inject assets through the quill
  tree.
- **`quill.renderWithOptions`** — this overload never existed on the WASM
  surface; the Rust `render_with_options` helper it mirrored was collapsed into
  `render`. `quill.render(doc, opts?)` is the single entry point for both
  default and custom render options.
- **`docs/format-designer/typst-backend.md`** — Python and JS code snippets
  rerouted from `workflow.render(parsed, …)` to `quill.render(doc, …)`.
- **`prose/schema-rework/`** — deleted. The plan's success criteria (delete
  `schema.rs`, drop `jsonschema` crate, remove `SchemaProjection`, expose
  `FormProjection` through bindings) all landed in the Document refactor, so
  the planning directory followed the same pattern as the 30+ other landed
  plans purged by the refactor.
- **`crates/bindings/python/examples/workflow_demo.py`** → renamed to
  `quill_demo.py`. Docstring updated to drop the removed `Workflow`
  terminology.
