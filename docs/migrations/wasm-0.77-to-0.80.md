# `@quillmark/wasm` 0.77.0 → 0.80.0

Migration notes for `@quillmark/wasm` consumers upgrading across the
**card-syntax** release.

!!! warning "Skip 0.78.0 and 0.79.0"
    `@quillmark/wasm@0.78.0` and `@quillmark/wasm@0.79.0` were experimental
    pre-releases and are being yanked. Upgrade straight from `0.77.0` to
    `0.80.0`.

    The card model evolved through two experimental iterations before
    stabilizing:

    - **`0.78.0`** introduced "leaf" terminology. It was never published for
      general use and the leaf concept does not survive into `0.80.0`.
    - **`0.79.0`** introduced the "card" model but spelled the schema's
      composable-card section `card_types`. That spelling is **officially
      deprecated**.

    `0.80.0` is the first stable release of the card model. Its composable
    cards are described by **card kinds**, and the schema section is named
    `card_kinds`. Treat both `0.78.0` and `0.79.0` as skip-this versions.

## TL;DR

The `Quillmark`, `Quill`, `Document`, and `RenderSession` class APIs are
**unchanged**. Three things move:

1. The canonical Markdown **card syntax** is a fenced code block
   (`` ```card <kind> ``). The legacy `---`/`CARD:` fence still parses, but
   `doc.toMarkdown()` now emits the fenced form.
2. The quill schema returned by `Quill.schema` names its composable-card
   section `card_kinds` (a map keyed by **card kind** name).
3. `Diagnostic` objects gain an optional `path` field, and validation /
   quill-config render errors now surface **every** diagnostic instead of
   just the first.

```diff
   const doc = Document.fromMarkdown(markdown);   // still accepts legacy CARD: fences
- // doc.toMarkdown() emitted `---`/`CARD:` card fences
+ // doc.toMarkdown() now emits ```card <kind> fenced blocks

- const kinds = quill.schema.card_types;          // experimental 0.79.0 spelling
+ const kinds = quill.schema.card_kinds;          // stable 0.80.0 spelling

  try {
    quill.render(doc, { format: "pdf" });
  } catch (err) {
-   // QuillConfig / ValidationFailed errors exposed only err.diagnostics[0]
+   // QuillConfig / ValidationFailed errors now expose every diagnostic
    for (const d of err.diagnostics) console.error(d.path ?? "", d.message);
  }
```

---

## 1. Card Markdown syntax

In `0.80.0` the canonical card block is a fenced code block whose info string
is `card <kind>`. The block content is the card's YAML; the Markdown after the
closing fence is the card body. `<kind>` is the **card kind** — the on-the-wire
`CARD` discriminator.

```diff
  ---
  QUILL: my_quill
  title: Main Document
  ---

  Some content here.

- ---
- CARD: products
+ ```card products
  name: Widget
  price: 19.99
- ---
+ ```

  Widget description.
```

The card kind still matches `[a-z_][a-z0-9_]*`, and `QUILL`, `CARD`, `BODY`,
and `CARDS` remain reserved field names. A `card` fenced block must have a
blank line above it to be recognized as a card.

### Input — no action required

`Document.fromMarkdown` still accepts the legacy `---`/`CARD:` fence. Both
syntaxes parse identically, so existing stored Markdown keeps working without
changes.

### Output — review round-trip consumers

`doc.toMarkdown()` always emits the **canonical fenced form**. Any legacy
`CARD:` fence in the source is rewritten to a `` ```card `` block on the next
`toMarkdown()` call.

This is the one behavior change most likely to affect you:

- **Snapshot / golden-file tests** that compare `toMarkdown()` output against
  a fixed string need their fixtures regenerated.
- **Persistence layers** that store `toMarkdown()` output will start writing
  fenced cards. This is safe — the result re-parses identically — but the
  on-disk bytes change.
- **Diffing the original source against `toMarkdown()`** to detect edits will
  report a spurious diff for documents that contained legacy card fences. Use
  `doc.equals(other)` for semantic comparison instead of string comparison.

The `Document` card mutators (`pushCard`, `insertCard`, `updateCardField`,
etc.) and the `CardInput` shape (`{ tag, fields?, body? }`) are unchanged.

## 2. Quill schema uses `card_kinds`

The schema returned by `Quill.schema` describes the main card and any number
of named **card kinds**. The card-kinds section is keyed `card_kinds`:

```ts
interface QuillSchema {
  main: QuillCardSchema;
  /** Present only when the quill declares at least one named card kind. */
  card_kinds?: Record<string, QuillCardSchema>;
}
```

If you read this section under the experimental `0.79.0` name `card_types`,
rename the access:

```diff
- const kinds = quill.schema.card_types;
+ const kinds = quill.schema.card_kinds;
```

Quills themselves declare composable cards under a `card_kinds:` section in
`Quill.yaml`. The experimental `card_types:` spelling from `0.79.0` is no
longer accepted — any bundled quill must use `card_kinds:`.

## 3. `Diagnostic` gains a `path` field

`Diagnostic` objects — returned in `err.diagnostics`, `quill.form().diagnostics`,
and `session.warnings` — now carry an optional `path` string:

```ts
interface Diagnostic {
  severity: "error" | "warning" | "note";
  message: string;
  path?: string;        // new in 0.80.0
  location?: Location;
  hint?: string;
  // ...
}
```

`path` is a document-model anchor (for example
`cards.indorsement[0].signature_block`) and is set on schema-validation
diagnostics; it is `undefined` otherwise. This is purely additive — no code
change is required, but you can now point a user at the offending field
without parsing the message string.

## 4. Validation errors carry all diagnostics

Before `0.80.0`, only the `CompilationFailed` render error forwarded its full
diagnostic list to JS. The `QuillConfig` and `ValidationFailed` variants fell
through a path that kept just the first diagnostic, so `err.diagnostics` had
length 1 even when several fields failed validation.

In `0.80.0` all three variants forward every diagnostic. If your error
handling assumed `err.diagnostics.length === 1` for validation or
quill-config failures, that assumption is no longer correct — iterate the
array instead:

```diff
  try {
    quill.render(doc, { format: "pdf" });
  } catch (err) {
-   showError(err.diagnostics[0].message);
+   for (const d of err.diagnostics) {
+     showError(d.path ? `${d.path}: ${d.message}` : d.message);
+   }
  }
```

`err.message` is still a single string — an aggregate
`"<N> error(s): <first.message>"` summary when there are multiple
diagnostics.

---

## Checklist

- [ ] Bump `@quillmark/wasm` from `0.77.0` directly to `0.80.0` (skip the
      experimental `0.78.0` and `0.79.0`).
- [ ] Regenerate any snapshot/golden fixtures that compare `toMarkdown()` output.
- [ ] Replace string comparisons of `toMarkdown()` output with `doc.equals`.
- [ ] Rename any `quill.schema.card_types` access to `quill.schema.card_kinds`.
- [ ] Ensure bundled quills declare composable cards under `card_kinds:` (not
      the deprecated `card_types:`).
- [ ] Update error handling to iterate `err.diagnostics` rather than reading
      only `err.diagnostics[0]`.
- [ ] Optionally surface `diagnostic.path` in validation UIs.
