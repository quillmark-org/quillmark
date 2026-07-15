# Quillmark WASM

WebAssembly bindings for Quillmark.

Maintained by [TTQ](https://tonguetoquill.com).

## Overview

Use Quillmark in browsers/Node.js with explicit in-memory trees (`Map<string, Uint8Array>` / `Record<string, Uint8Array>`).

The package exposes **one import surface**:

- `@quillmark/wasm` (the root) — the **canonical API**: `Quill`, `Document`, and
  an `Engine` that renders them.

`Quill` and `Document` are re-exported verbatim from the internal Typst-less
core build, so editor/validation code (`Quill.fromTree`,
`Document.fromMarkdown`) loads only that small core binary — no backend is
loaded until you render. The `Engine` hides everything else: each backend
(`typst`, `pdfform`) is a separate, private WASM binary with its own linear
memory, lazily loaded on the first render. The Engine clones a `Quill` /
`Document` into the backend's memory as data and frees the clones — you never
hold a backend object or cross a memory boundary yourself.

## Build

```bash
bash scripts/build-wasm.sh
```

The script builds three variants — the core (no backend), the Typst backend
(default features), and the Typst-free pdfform backend (`pdfform` feature) —
each with `--target bundler` and `--weak-refs` enabled (see
[Lifecycle](#lifecycle)).

## Test

```bash
bash scripts/build-wasm.sh
cd crates/bindings/wasm
npm install
npm test
```

## Usage

```ts
import { Document, Quill, Engine } from "@quillmark/wasm";

const quill = Quill.fromTree(tree);   // no engine needed: build + validate
const engine = new Engine();          // loads a backend lazily on first render

const markdown = `~~~
$quill: my_quill
$kind: main
title: My Document
~~~

# Hello`;

const parsed = Document.fromMarkdown(markdown);
const result = await engine.render(quill, parsed, { format: "pdf" });
```

## API

### `new Engine(options?)`
Create the render dispatcher. Routes each quill to its backend by
`quill.backendId`, lazily loads that backend binary, and renders — cloning the
quill/document into the backend's memory and freeing the clones internally.
`render`, `open`, `supportedFormats`, and `supportsCanvas` are **async** (the
first call may load a backend). Pass `{ backends }` to register or override
backend descriptors. Each entry is a descriptor
(`{ [backendId]: { load, formats, canvas } }`) where `load` is the lazy thunk
returning the backend module and `formats`/`canvas` are the **required** static
capability manifest. A malformed descriptor throws at `new Engine(...)`, naming
the backend id.

**Capability probes are always free.** `supportedFormats` and `supportsCanvas`
depend only on `quill.backendId`, and answer from the descriptor's required
`formats`/`canvas` manifest — never loading the multi-MB backend binary and
never cloning the quill. Use them as non-failing pre-render probes.

### `Quill.fromTree(tree)`
Build + validate a `Quill` from an in-memory tree. Pure — the declared backend
is resolved at render time, not here. Loads no backend binary.

### `new Document(quillRef)`
A blank document: a main card carrying only `$quill`, an empty body, and no
composable cards — the programmatic blank canvas. Absent fields resolve at
render time (schema `default`, else type-empty zero), so nothing the caller
did not set reaches the output. Build it up with `setFields` / `pushCard`.
For an example-filled starter use `quill.seedDocument()`. Throws on an
invalid quill reference.

### `Document.fromMarkdown(markdown)`
Parse markdown to a parsed document. Throws a JS `Error` (with `.diagnostics`
attached, see [Errors](#errors)) on any parse failure, including a missing
root `$quill` metadata line, malformed YAML, and inputs over the 10 MB
`parse::input_too_large` limit.

### `doc.toMarkdown()`
Emit canonical Quillmark Markdown. Type-fidelity round-trip safe:
`Document.fromMarkdown(doc.toMarkdown())` returns a document equal to `doc`
under [`doc.equals`](#docequalsother). The output is **not** guaranteed
byte-equal to the original source — YAML quoting, key ordering, and
whitespace are normalised. Use `equals` (not string comparison) to test
semantic equality.

### `doc.toJson()`
Serialize the document to a versioned storage DTO — a JSON **string**
carrying a `schema` version. Use this (not `toMarkdown`) to persist a
document across a process restart or crate upgrade: the wire format is
frozen per `schema` version, whereas Markdown syntax evolves. Parse-time
`warnings` are not part of the DTO.

The string is produced inside the module by `serde_json`; the JS `JSON`
global is not involved. It is standard JSON text, so callers may
`JSON.parse` it to inspect it — but it is intended as an opaque blob you
persist and hand back.

`toJson()` is **deterministic**: a `Document` that is `equals` to another
serializes to a byte-identical string — across repeated calls, and across
any crate upgrade that keeps the same `schema` version (every release does until
the `Document` model changes; see [Storage compatibility](#storage-compatibility-across-versions)).
Field order is fixed and object key order is preserved, so content hashes
and string-equality dirty-checks over the output are stable.

### `Document.fromJson(json)`
Reconstruct a `Document` from a storage DTO string produced by `toJson`.
Round-trips losslessly:

```ts
const stored = doc.toJson();        // persist this string
const restored = Document.fromJson(stored);
restored.equals(doc);               // true
```

Throws a JS `Error` on malformed JSON, an unknown `schema` version, or a
malformed payload. The restored document has no parse-time `warnings`.

### `Document.tryFromJson(json)`
Like `fromJson`, but returns `undefined` instead of throwing when `json` is
not a valid storage DTO. Use it to branch on format without a heuristic or
`try`/`catch` as control flow:

```ts
// "JSON canonical, Markdown fallback" — no exceptions, no string sniffing
const doc = Document.tryFromJson(content) ?? Document.fromMarkdown(content);
```

`undefined` means only "not a storage DTO"; `fromMarkdown` still throws on
genuinely malformed Markdown.

### Storage compatibility across versions

The `schema` value (`quillmark/document@0.93.0`) is the **model version**,
not the running crate version. It is a hand-set constant, bumped only when
the `Document` model itself changes — so every `0.93.x` patch release reads
and writes that same value.

- **Upgrading is safe.** A newer build always reads documents written by an
  older one. Each schema version's wire format is frozen and never changes;
  when the model does change, the new build ships a migration that converts
  old payloads on `fromJson`. A document you commit as your canonical
  on-disk format keeps loading across crate upgrades — there is no need to
  pin old wasm to read old data.
- **Downgrading is not.** `fromJson` rejects an *unknown* (i.e. newer)
  `schema` version rather than guessing at a format it predates. Don't feed
  documents written by a newer build back into an older one.

To detect a version mismatch before parsing, use the static accessors:

```ts
const v = Document.schemaVersionOf(blob); // undefined | string
if (v && v !== Document.currentSchemaVersion()) {
  // payload is from a build with a different model version
}
```

`schemaVersionOf` does not validate the payload — it only reads the
`schema` field, returning `undefined` for non-JSON, non-objects, or
payloads that don't carry one. Use it to distinguish "wrong version" from
"corrupt" when `fromJson` throws.

In short: persist the `toJson` string, upgrade freely, never downgrade. The
full design — including how migrations are added — is in
`prose/canon/DOCUMENT_STORAGE.md`.

### `doc.equals(other)`
Structural equality between two `Document` handles. Compares `main` and
`cards` by value; parse-time `warnings` are intentionally excluded.

Use this to debounce upstream prop updates: keep the last parsed `Document`
and compare instead of re-parsing on every keystroke.

### `doc.cardCount`
O(1) getter for the number of composable cards (excluding the main card).
Use this to validate indices before calling card mutators (`removeCard`,
`setCardField`, etc.) without allocating the full `cards` array.

### `quill.validate(doc)`

Returns `Diagnostic[]` — the document validated against the quill schema,
without invoking the backend. An empty array means the document is valid.
Each diagnostic carries the canonical `validation::*` `code`, `path`, and
`hint`. Includes the non-fatal `validation::must_fill` warning for each
`!must_fill` marker left in the document (render zero-fills these rather
than failing), so filter by `severity`/`code` for blockers vs. hints:

```ts
const diagnostics = quill.validate(Document.fromMarkdown(markdown));
const errors = diagnostics.filter(d => d.severity === "error");
```

To render a form editor, read field definitions from `quill.schema` (walk
`fields` in key order — declaration order is display order) and the authored
values from the `Document` payload — there is no separate form-view projection.

### `quill.seedDocument()`

Returns a starter `Document` seeded from the schema: each field's `example:`
is committed and every other field is left absent (the render layer fills
`default:` → type-empty zero). Illustration-first — a field with both an
`example` and a `default` renders its example. Use as the initial state for a
"new document" editor.

```ts
const doc = quill.seedDocument();
const markdown = doc.toMarkdown();
```

For per-card seeding, `quill.seedMain()` returns just the `$kind: main` card
and `quill.seedCard(kind)` returns a starter composable card (or `undefined`
if the kind is not declared). Both return the read `Card` shape of
`doc.main` / `doc.cards`, which `doc.pushCard` / `doc.insertCard` accept
directly:

```ts
doc.pushCard(quill.seedCard("note"));                 // seed → push
doc.pushCard(Document.makeCard("note", { x: 1 }));    // build from a flat map
doc.pushCard({ kind: "note", body: "Plain **markdown**." });  // bare inline
```

Reads and writes are two aligned shapes. A read `Card` always has `body:
RichText` (canonical corpus, never a raw string) — no narrowing, no guessing
whether the body was normalized. The write shape `CardInput` widens `body` to
`RichText | string` (a markdown string imports to the corpus) and makes every
field but `kind` optional. Every `Card` is a valid `CardInput`, so `pushCard` /
`insertCard` still take exactly what `cards` / `removeCard` / `seedCard` return.
Build a fresh card from a flat field map with
`Document.makeCard(kind, fields?, body?)`.

Batch mutation: `doc.setFields({...})` / `doc.setCardFields(index, {...})`
apply a whole object atomically — on any invalid field nothing is applied and
the thrown error carries one diagnostic per offending field (`path` = field
name).

### Typed writes: `commit*` is the default, `set*` is the quill-free primitive

A `Document` holds only a `$quill` *reference*, not the resolved schema, so it
mutates through two layers:

- **`commit*` — the schema-bound default whenever a quill is in hand.**
  `doc.commitField(quill, name, value)` / `doc.commitFields(quill, {...})` (and
  the `commitCard*` twins) resolve each field's schema `type`, coerce the value
  to its canonical form (`"3"` → `3`, a markdown string → a richtext corpus),
  and **fail now** on a mismatch instead of at render. A name the schema does
  not declare throws `UnknownField` rather than falling to the opaque store — on
  the typed path an undeclared name is a typo, not a fallback. The batch form is
  all-or-nothing: an undeclared name aborts the whole write and its per-field
  diagnostics name every offending field, so a whole-form submit surfaces every
  typo `setFields` would silently absorb.

- **`set*` — the deliberate quill-free primitive.** `doc.setField(name, value)`
  / `doc.setFields({...})` (and the `setCard*` twins) validate only the field
  name/depth/kind and store the value verbatim, no quill required. Reach for it
  on purpose when you *want* the opaque store: quill-agnostic storage/migration
  infra that has no bundle and must write regardless of a drifted schema;
  store-now-validate-later editors holding in-progress input that `commit`
  would reject; or verbatim passthrough of fields the schema doesn't own. It is
  the lower layer, not a lighter `commit` — a typo'd field name stores silently
  and only surfaces at `quill.validate` / render.

Per-keystroke cost is the same either way (both mutate the in-memory `Document`
in place; no seam is crossed), so steering to `commit*` buys the type check for
free.

#### `DocumentWriter` / `CardWriter` — bind the quill once

The `commit*` verbs take the `quill` handle per call (the document carries no
schema). When you hold both a quill and a document — a form editor, an MCP
writer — bind them once with the writer sugar and issue bare verbs:

```ts
import { DocumentWriter } from "@quillmark/wasm";

const ed = new DocumentWriter(quill, doc);          // JS twin of Rust `quill.writer(doc)`
ed.set("subject", "Q3 results");                    // strict-committed to the schema type
ed.setAll({ qty: "3", subject: "Q3" });             // all-or-nothing batch
ed.set("titel", "x");                               // throws UnknownField — a typo, not a fallback
ed.card(2).set("body", "**note**");                 // composable card, resolved by its $kind
```

`DocumentWriter` / `CardWriter` are pure JS holding references to your existing
`quill` and `doc` — no WASM handle of their own, nothing to `free()`. `card(i)`
is lazy: it never throws; an out-of-range index throws `IndexOutOfRange` at the
write.

### `engine.render(quill, parsed, opts?)` vs. `engine.open(quill, parsed)`

Use **`engine.render`** for one-shot exports (PDF/SVG/PNG) — compiles, emits
artifacts, done. Use **`LiveSession`** (returned by `engine.open`) for
reactive previews: the session is a persistent compiler. `paint` / `render` /
`regions` / `fieldAt` read its current compile without recompiling, and `apply(doc)`
recompiles in place on each edit, returning a `ChangeSet` whose `dirtyPages`
tells you which pages to repaint (`dirty ∩ visible`). Apply is transactional —
on throw, every read keeps serving the last-good compile. Don't open a session
per export, and don't re-open per edit — `apply` instead.

### `engine.render(quill, parsed, opts?)`
Render a pre-parsed `Document` against `quill`. Throws an
`engine::backend_not_found` error if no registered backend matches the quill's
declared backend.

### `engine.open(quill, parsed)` + `session.render(opts?)`
Open once, render all or selected pages (`opts.pages`).

The session also exposes `pageCount`, `backendId`, `supportsCanvas`,
`warnings` (non-fatal diagnostics of the current compile — set at `open`,
refreshed by each committed `apply`),
`apply(doc)` for in-place recompiles, `pageSize(page)`, and
`paint(ctx, page, opts?)` for canvas previews. See below.

A document that compiles to zero pages still produces a valid session
(`pageCount === 0`); `paint(ctx, 0)` and `pageSize(0)` then throw
`page index 0 out of range (pageCount=0)`. Branch on `pageCount === 0` to
render a "no pages to preview" UI without relying on the throw.

### Canvas Preview

`session.paint(ctx, page, opts?)` rasterizes a page directly into a
`CanvasRenderingContext2D` (main thread) or
`OffscreenCanvasRenderingContext2D` (Worker), skipping PNG/SVG byte
round-trips.

The painter owns `canvas.width` / `canvas.height` — it sizes the backing
store itself. Consumers own `canvas.style.*` (or the layout system that
sets them) and read `layoutWidth` / `layoutHeight` from the returned
`PaintResult`.

```ts
const result = session.paint(canvas.getContext("2d"), 0, {
  layoutScale: 1,                            // layout px per pt (page geometry unit)
  densityScale: window.devicePixelRatio,     // backing-store density
});

canvas.style.width  = `${result.layoutWidth}px`;
canvas.style.height = `${result.layoutHeight}px`;
```

- `layoutScale` (default 1) sets the canvas's display-box size:
  `layoutWidth = widthPt * layoutScale`. For on-screen canvases this is
  CSS pixels per point. Defaults to 1 (one CSS pixel per pt).
- `densityScale` (default 1) is the backing-store density multiplier.
  Fold `window.devicePixelRatio`, in-app zoom, and `visualViewport.scale`
  (pinch-zoom) into a single value here. Pass `devicePixelRatio` for
  crisp output on high-DPI displays.
- The effective rasterization scale is `layoutScale * densityScale`. If
  that would exceed the safe maximum (16384 px per side), `densityScale`
  is clamped proportionally; `result.clamped` reports it and
  `result.effectiveDensityScale` is the density actually applied. A
  clamped page renders soft at the same `canvas.style` size.
- `paint` writes the whole backing store with `putImageData`, which
  ignores the 2D context transform, `globalAlpha`, and clip. Give each
  visible page its own `` element — you cannot composite two pages,
  a sub-rect, or a context transform through `paint`.
- `paint` is always a full repaint — setting the backing-store width /
  height clears it. No `clearRect` required. Each call re-rasterizes from
  scratch (no per-page raster cache), so keep a page's canvas alive while
  it stays near the viewport rather than pooling one canvas across pages:
  an idle canvas retains its pixels for free, whereas reusing a canvas on
  scroll re-runs a full render.
- `pageCount` and `pageSize(page)` are stable for the session's
  lifetime (immutable snapshot) — cache them.
- Worker support: pass an `OffscreenCanvasRenderingContext2D` and the
  same call signature works. `layoutWidth` / `layoutHeight` are
  informational in that mode (no CSS layout box); fold everything into
  `densityScale`. Loading the WASM module inside a Worker is the host's
  responsibility.
- Backend support: gated by `supportsCanvas`. Probe upfront with
  `engine.supportsCanvas(quill)` (or `session.supportsCanvas`) before mounting
  a canvas-based UI; the throw on `paint` / `pageSize` remains the
  enforcement contract and includes the resolved `backendId` for
  debugging.

### Schema model

A field's *cell* is inferred from whether its schema declares a `default:`:

- **Unendorsed** (no `default:`) — `quill.blueprint` renders the
  `!must_fill` marker in the value cell (carrying the field's `example` as a
  suggested value when one exists). An absent Unendorsed field zero-fills
  silently. A `!must_fill` marker left in the document is non-fatal: it emits
  the `validation::must_fill` warning and still renders. Partial documents
  are accepted; `engine.render(quill, doc)` only throws for malformed
  input.
- **Endorsed** (with `default:`) — `quill.blueprint` renders the
  default value with a type-only `# <type>` annotation (shippable as-is),
  and the default is used when the document omits the field.

`QuillFieldSchema` has no `required` axis. A `!must_fill` marker left in the
document emits the non-fatal `validation::must_fill` warning.

### Errors

Every method that can fail throws a **`QuillmarkError`** — a JS `Error` with
`.diagnostics` attached. The type and a guard are exported from the root:

```ts
import { isQuillmarkError, type QuillmarkError } from "@quillmark/wasm";

try {
  const result = await engine.render(quill, doc);
} catch (e) {
  if (isQuillmarkError(e)) {
    for (const d of e.diagnostics) console.error(d.severity, d.message);
  } else {
    throw e; // not a quillmark failure — programming error, re-throw
  }
}
```

`QuillmarkError` is a **structural interface, not a class** — the WASM layer
throws a real `Error` and attaches the property, so there is no constructor to
`instanceof` against; narrow with `isQuillmarkError` (which also works on
errors from any build or WASM instance in the page).

`diagnostics` is always non-empty — length 1 for most failures, length N for
backend compilation errors. `message` is derived from `diagnostics`
(`diagnostics[0].message` for single-diagnostic errors; an aggregate
`"<N> error(s): <first.message>"` summary for compilation failures).

Read `err.diagnostics[0]` for the primary diagnostic; iterate the array for
compilation failures. The same shape applies to every throw site:

- `Document.fromMarkdown` — parse errors (missing root `$quill` metadata, YAML
  errors, `parse::input_too_large` for inputs > 10 MB).
- `Document` mutators (`setField`, `setCardField`, etc.) — `EditError`
  variants (`InvalidFieldName`, `InvalidKindName`, `ReservedKind`,
  `IndexOutOfRange`, `ValueTooDeep`) appear in `diagnostics[0].message` with
  the `[EditError::<Variant>]` prefix.
- `engine.render` / `session.render` — backend compilation failures and
  validation errors.

### Lifecycle

The wasm bindings are built with `--weak-refs`, so dropped `Document`,
`Quill`, and `LiveSession` handles are reclaimed by `FinalizationRegistry`
without manual `.free()` discipline. `.free()` is still emitted as an eager
teardown hook for callers that want deterministic release.

`engine.render` and `engine.open` read the `quill` and `doc` handles
synchronously, before their first await, so freeing a handle as soon as the
call returns — `try { return engine.render(quill, doc); } finally
{ doc.free(); }` — is safe even on the first render, while the backend
binary is still loading.

The package floor is Node 22+ (`engines: { node: ">=22" }`) and current
evergreen browsers; `--weak-refs` itself only needs Node 14.6+. The `using`
sugar shown below ([explicit resource management][erm]) needs Node 24, but is
optional — the `try` / `finally` fallback runs on the Node 22 floor.

For environments where `using` (the [explicit resource management][erm]
proposal) hasn't landed, use an explicit `try` / `finally`:

```ts
const session = await engine.open(quill, doc);
try {
  for (let p = 0; p < session.pageCount; p++) {
    session.paint(ctx, p);
  }
} finally {
  session.free();
}
```

[erm]: https://github.com/tc39/proposal-explicit-resource-management

## Notes

- Parsed markdown requires a root `~~~` block (a bare three-tilde fence;
  `~~~card-yaml` is also accepted as a non-canonical alias)
  with a `$quill` system-metadata line. Empty input surfaces a dedicated
  "Empty markdown input cannot be parsed" message.
- A `$quill` mismatch during `engine.render(quill, parsed)` is a thrown error, not a warning: rendering with a quill whose *name* differs (`quill::name_mismatch`) or whose *version* falls outside the selector (`quill::version_mismatch`) is rejected.
- Output schema APIs live on `Quill`, not the engine.

## Changelog

See the [changelog](https://github.com/borb-sh/quillmark/blob/main/CHANGELOG.md)
and the [GitHub Releases](https://github.com/borb-sh/quillmark/releases) page for
release notes and version history.

## License

Apache-2.0
