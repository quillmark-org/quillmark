# @quillmark/conformance

The frozen engine↔consumer **contract** for Quillmark, as executable fixtures.
One `conformance.json` both the engine (`borb-sh/quillmark`) and its pinned
consumers run in CI — the general cure for "green upstream tests, broken pinned
consumer." Published lockstep with `@quillmark/wasm` from the same commit; pin
both to the same version.

Freezing this set is what the 1.0 release *means*: post-1.0 a fixture diff is a
breaking change by definition; pre-1.0 the diffs are the pivot log. See
[`prose/canon/CONFORMANCE.md`](https://github.com/borb-sh/quillmark/blob/main/prose/canon/CONFORMANCE.md).

## What it asserts

- **`fieldStates()`** — per field, the resolved `{ value, source }` and
  declaration order.
- **`validate()`** — the diagnostic set (`severity`, `code`, `path`).
- **Mutator errors** — the `edit::*` `code` and `DocPath` a failed operation
  carries.
- **The `DocPath` grammar** — every address parses to the expected
  `DocPathSeg[]` and round-trips, geometry addresses included.

A fixture asserts a diagnostic's `code`, `path`, and `severity` — **never its
`message`**. A copyedit is not a formal break, so fixture diffs stay meaningful.

## Usage

Supply an adapter mapping the contract verbs to your integration layer (over
`@quillmark/wasm`), then run the suite:

```js
import { runConformance, contractVersion } from '@quillmark/conformance'
import { Quill, Document, parseDocPath, formatDocPath, contractVersion as engineVersion } from '@quillmark/wasm'

const adapter = {
  contractVersion: () => engineVersion(),
  buildQuill: (files) =>
    Quill.fromTree(new Map(Object.entries(files).map(([p, t]) => [p, new TextEncoder().encode(t)]))),
  parseDocument: (md) => Document.fromMarkdown(md),
  storeField: (doc, card, field, value) => doc.storeField({ card: card ?? undefined, field }, value),
  storeFill: (doc, card, field, value) => doc.storeFill({ card: card ?? undefined, field }, value),
  removeField: (doc, card, field) => doc.removeField({ card: card ?? undefined, field }),
  commitField: (quill, doc, card, field, value) =>
    quill.writer(doc)[card == null ? 'set' : 'card']?.(card ?? field, card == null ? value : undefined),
  insertCard: (doc, kind, index, body) => doc.insertCard({ kind, body: body ?? undefined }, index ?? undefined),
  removeCard: (doc, index) => doc.removeCard(index),
  validate: (quill, doc) => quill.validate(doc),
  fieldStates: (quill, doc) => quill.fieldStates(doc),
  parseDocPath,
  formatDocPath,
}

// In your test runner:
runConformance(adapter) // throws an aggregate error naming every failure
```

The verb bindings above are sketches — adapt them to your surface's exact
signatures. The `fixtures` / `paths` / `quills` arrays are also exported for
per-fixture test cases (`runFixture` / `runPath`).

## Contract version

`contractVersion` is semver'd over `{ diagnostic taxonomy, DocPath grammar,
fieldStates shape }`, independent of the package version. `runConformance`
asserts your engine's `contractVersion()` matches the frozen value before
running — a compatibility check at load time, not at bug-report time.
