# `@quillmark/wasm` 0.77.0 → 0.79.0

Migration guide for `@quillmark/wasm` consumers upgrading across the **card
model unification** (the *leaf rework*, PRs #567 and #579).

!!! warning "Skip 0.78.0"
    `@quillmark/wasm@0.78.0` shipped the leaf rework mid-flight and is being
    **yanked**. Do not pin to it. Upgrade directly from `0.77.0` to `0.79.0`.

## TL;DR

The `CARD` concept is gone. Cards are now a single uniform model addressed by
**kind**, and the on-the-wire discriminator key is `KIND`.

```diff
  // Markdown: card blocks are fenced code blocks, not `---` blocks
- ---
- CARD: products
- name: Widget
- ---
+ ```card products
+ name: Widget
+ ```

  // WASM API
- doc.setCardTag(0, "products");
+ doc.setCardKind(0, "products");

- const schema = quill.schema;  // schema.card_types
+ const schema = quill.schema;  // schema.cards

  // Backend template data
- card.CARD == "products"
+ card.KIND == "products"
```

The WASM API renames below (§2–§5) are **hard breaks** — there are no
compatibility aliases. The Markdown card syntax (§1) is a *soft* break: the
old `---/CARD: …/---` form still parses, with a deprecation warning.

---

## 1. Card syntax in Markdown

Cards are no longer `---`-delimited metadata blocks. A card is now a
[CommonMark fenced code block](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)
whose info string is `card <kind>`.

````diff
  ---
  QUILL: usaf_memo
  subject: Example
  ---

  Main body text.

- ---
- CARD: indorsement
- from: ORG/SYMBOL
- for: RECIPIENT/SYMBOL
- ---
+ ```card indorsement
+ from: ORG/SYMBOL
+ for: RECIPIENT/SYMBOL
+ ```

  Body of the endorsement.
````

Consequences for any Markdown your consumer generates or accepts:

- The info string must be exactly `card <kind>`. A missing kind token, an
  invalid kind token (`[a-z_][a-z0-9_]*`), or any extra info-string token is a
  **hard parse error**.
- Card fences obey CommonMark run-length closure — to embed a code block in a
  card body, open the card with a longer fence.
- A mid-document `---/…/---` block whose first body key is **not** `CARD:` is
  now a plain CommonMark thematic break, not a metadata fence. Only the
  top-of-document frontmatter block and legacy card blocks (below) are
  metadata fences.

### Legacy `---/CARD: …/---` blocks still parse

Existing documents do **not** have to be regenerated. A `---/…/---` block
whose first body key is `CARD:` is still parsed as a card (the `CARD:` value
becomes the kind) and emits a `parse::deprecated_card_syntax` warning. Passing
such a document through `doc.toMarkdown()` rewrites it to the canonical
`` ```card <kind> `` form, so a parse-then-emit round-trip migrates documents
for free:

```js
const doc = Document.fromMarkdown(legacyMarkdown);
// doc.warnings includes a `parse::deprecated_card_syntax` entry per block
const migrated = doc.toMarkdown();   // canonical ```card fences
```

Only the fence *shape* is deprecated — the `card` noun is unchanged. The
legacy path is retained for existing documents; its removal is
telemetry-driven, not pinned to a release.

See [Cards](../authoring/cards.md) for the full syntax.

## 2. `CARD` → `KIND` reserved name

The card discriminator key is renamed `CARD` → `KIND` everywhere:

| Surface | Before | After |
| --- | --- | --- |
| Reserved frontmatter names | `BODY`, `CARDS`, `QUILL`, `CARD` | `BODY`, `CARDS`, `QUILL`, `KIND` |
| Card record (`doc.cards[i]`) | `card.tag` carried the `CARD` value | `card.tag` carries the `KIND` value |
| Backend template data | `card.CARD` | `card.KIND` |

`EditError::ReservedName` now fires for `KIND` (and no longer for `CARD`).
Mutators that wrote or rejected `CARD` must be updated.

## 3. `Document.setCardTag` → `Document.setCardKind`

The structural mutator is renamed. The signature `(index, newKind)` and
behaviour (mutates only the sentinel; throws on out-of-range index or invalid
kind) are unchanged.

```diff
- doc.setCardTag(0, "indorsement");
+ doc.setCardKind(0, "indorsement");
```

All other `Document` mutators (`setField`, `removeField`, `pushCard`,
`insertCard`, `removeCard`, `moveCard`, `updateCardField`, `removeCardField`,
`updateCardBody`) are unchanged.

## 4. `QuillSchema.card_types` → `QuillSchema.cards`

The schema object returned by `quill.schema` renames its composable-card map:

```diff
  interface QuillSchema {
      main: QuillCardSchema;
-     card_types?: Record<string, QuillCardSchema>;
+     cards?: Record<string, QuillCardSchema>;
  }
```

```diff
  const schema = quill.schema;
- const cardSchema = schema.card_types?.["indorsement"];
+ const cardSchema = schema.cards?.["indorsement"];
```

`QuillSchema.main` is unchanged. The `cards` map is present only when the
quill declares at least one composable card kind.

## 5. `Quill.blankCard` argument

`quill.blankCard(kind)` takes a card **kind** name. The call is positional, so
JS callers need no change — but the value must be a declared inline card kind,
and the reserved name `main` is not a valid kind. The return type is
`FormCard | null` (`null` when the kind is not declared).

## 6. `Quill.yaml` schema layout

If your consumer ships or generates `Quill.yaml` files, the top-level layout
changed. The `main:` section and `card_types:` section are merged into a
single `cards:` map; `main` is the reserved entry-point key.

```diff
- main:
-   fields:
-     sender:
-       type: string
-
- card_types:
-   indorsement:
-     fields:
-       from:
-         type: string
+ cards:
+   main:
+     fields:
+       sender:
+         type: string
+   indorsement:
+     fields:
+       from:
+         type: string
```

Root-level `fields:` is still unsupported; the main schema now lives under
`cards.main.fields`. See the
[Quill.yaml Reference](../format-designer/quill-yaml-reference.md).

---

## New in 0.79.0

These are additive — no migration required, but worth adopting.

### `Diagnostic.path`

`Diagnostic` objects (in `err.diagnostics`, `doc.warnings`, render warnings)
now carry an optional `path` field — a document-model path anchor such as
`"cards.indorsement[0].signature_block"`. It is set on schema-validation
diagnostics and `undefined` otherwise. Use it to navigate a form UI straight
to the offending field.

```ts
interface Diagnostic {
  severity: "error" | "warning" | "note";
  code?: string;
  message: string;
  location?: Location;
  path?: string;        // new
  hint?: string;
  sourceChain?: string[];
}
```

### Multi-diagnostic render errors

Previously `quill.render` / `quill.open` forwarded every diagnostic only for
`CompilationFailed` errors; quill-config and schema-validation failures
collapsed to a single diagnostic. Now `err.diagnostics` carries **all**
diagnostics for those failures too:

```js
try {
  quill.render(doc);
} catch (err) {
  for (const d of err.diagnostics) {
    console.error(d.path ?? "<doc>", d.message);
  }
}
```

If your error handling read only `err.diagnostics[0]`, switch to iterating the
full array to surface every validation problem at once.

---

## Unchanged

- `new Quillmark()`, `engine.quill(tree)` (`Map`/`Record` of `Uint8Array`).
- `Document.fromMarkdown`, `doc.frontmatter`, `doc.body`, `doc.cards`,
  `doc.warnings`, `doc.quillRef`, `doc.toMarkdown()`.
- `quill.render(doc, opts?)`, `quill.open(doc)` → `session.render(opts)`,
  `session.pageCount`, `quill.backendId`.
- `quill.form(doc)`, `quill.blankMain()`.
- `RenderOptions` shape (`{ format?, ppi?, pages? }`) and `RenderResult` shape.
- npm package name and import path.
