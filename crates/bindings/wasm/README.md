# Quillmark WASM

WebAssembly bindings for Quillmark.

Maintained by [TTQ](https://tonguetoquill.com).

## Overview

Use Quillmark in browsers/Node.js with explicit in-memory trees (`Map<string, Uint8Array>` / `Record<string, Uint8Array>`).

## Build

```bash
bash scripts/build-wasm.sh
```

The script builds for `bundler` and `experimental-nodejs-module` targets with
`--weak-refs` enabled (see [Lifecycle](#lifecycle)).

## Test

```bash
bash scripts/build-wasm.sh
cd crates/bindings/wasm
npm install
npm test
```

## Usage

```ts
import { Document, Quillmark } from "@quillmark-test/wasm";

const engine = new Quillmark();
const quill = engine.quill(tree);

const markdown = `---
QUILL: my_quill
title: My Document
---

# Hello`;

const parsed = Document.fromMarkdown(markdown);
const result = quill.render(parsed, { format: "pdf" });
```

## API

### `new Quillmark()`
Create engine.

### `engine.quill(tree)`
Build + validate + attach backend. Returns a render-ready `Quill`.

### `Document.fromMarkdown(markdown)`
Parse markdown to a parsed document. Throws a JS `Error` (with `.diagnostics`
attached, see [Errors](#errors)) on any parse failure, including missing
`QUILL`, malformed YAML, and inputs over the 10 MB `parse::input_too_large`
limit.

### `doc.toMarkdown()`
Emit canonical Quillmark Markdown. Type-fidelity round-trip safe:
`Document.fromMarkdown(doc.toMarkdown())` returns a document equal to `doc`
under [`doc.equals`](#docequalsother). The output is **not** guaranteed
byte-equal to the original source — YAML quoting, key ordering, and
whitespace are normalised. Use `equals` (not string comparison) to test
semantic equality.

### `doc.equals(other)`
Structural equality between two `Document` handles. Compares `main` and
`leaves` by value; parse-time `warnings` are intentionally excluded.

Use this to debounce upstream prop updates: keep the last parsed `Document`
and compare instead of re-parsing on every keystroke.

### `doc.leafCount`
O(1) getter for the number of composable leaves (excluding the main leaf).
Use this to validate indices before calling leaf mutators (`removeLeaf`,
`updateLeafField`, etc.) without allocating the full `leaves` array.

### `quill.form(doc)`

Returns `{ main, leaves, diagnostics }` — a schema-aware snapshot of `doc`
without invoking the backend. `diagnostics` contains validation errors and
warnings; an empty array means the document is valid. Useful for validating
content without rendering:

```ts
const form = quill.form(Document.fromMarkdown(markdown));
const errors = form.diagnostics.filter(d => d.severity === "error");
```

### `quill.render(parsed, opts?)` vs. `quill.open(parsed)`

Use **`Quill.render`** for one-shot exports (PDF/SVG/PNG) — compiles, emits
artifacts, done. Use **`RenderSession`** (returned by `Quill.open`) for
reactive previews where you'll paint or re-emit pages multiple times: the
session retains the compiled snapshot so subsequent `paint` / `render`
calls skip recompilation. Don't open a session per export.

### `quill.render(parsed, opts?)`
Render with a pre-parsed `Document`.

### `quill.open(parsed)` + `session.render(opts?)`
Open once, render all or selected pages (`opts.pages`).

The session also exposes `pageCount`, `backendId`, `supportsCanvas`,
`warnings` (snapshot of session-level diagnostics attached at `open` time),
`pageSize(page)`, and `paint(ctx, page, opts?)` for canvas previews. See
below.

A document that compiles to zero pages still produces a valid session
(`pageCount === 0`); `paint(ctx, 0)` and `pageSize(0)` then throw
`page index 0 out of range (pageCount=0)`. Branch on `pageCount === 0` to
render a "no pages to preview" UI without relying on the throw.

### Canvas Preview (Typst only)

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
  layoutScale: 1,                            // layout px per Typst pt
  densityScale: window.devicePixelRatio,     // backing-store density
});

canvas.style.width  = `${result.layoutWidth}px`;
canvas.style.height = `${result.layoutHeight}px`;
```

- `layoutScale` (default 1) sets the canvas's display-box size:
  `layoutWidth = widthPt * layoutScale`. For on-screen canvases this is
  CSS pixels per Typst point. Defaults to 1 (one CSS pixel per pt).
- `densityScale` (default 1) is the backing-store density multiplier.
  Fold `window.devicePixelRatio`, in-app zoom, and `visualViewport.scale`
  (pinch-zoom) into a single value here. Pass `devicePixelRatio` for
  crisp output on high-DPI displays.
- The effective rasterization scale is `layoutScale * densityScale`. If
  that would exceed the safe maximum (16384 px per side), `densityScale`
  is clamped proportionally; compare `result.pixelWidth` against
  `Math.round(result.layoutWidth * densityScale)` to detect.
- `paint` is always a full repaint — setting the backing-store width /
  height clears it. No `clearRect` required.
- `pageCount` and `pageSize(page)` are stable for the session's
  lifetime (immutable snapshot) — cache them.
- Worker support: pass an `OffscreenCanvasRenderingContext2D` and the
  same call signature works. `layoutWidth` / `layoutHeight` are
  informational in that mode (no CSS layout box); fold everything into
  `densityScale`. Loading the WASM module inside a Worker is the host's
  responsibility.
- Backend support: gated by `supportsCanvas`. Probe upfront with
  `quill.supportsCanvas` (or `session.supportsCanvas`) before mounting a
  canvas-based UI; the throw on `paint` / `pageSize` remains the
  enforcement contract and includes the resolved `backendId` for
  debugging.

### Errors

Every method that can fail throws a JS `Error` with `.diagnostics` attached:

```ts
{ message: string, diagnostics: Diagnostic[] }
```

`diagnostics` is always non-empty — length 1 for most failures, length N for
backend compilation errors. `message` is derived from `diagnostics`
(`diagnostics[0].message` for single-diagnostic errors; an aggregate
`"<N> error(s): <first.message>"` summary for compilation failures).

Read `err.diagnostics[0]` for the primary diagnostic; iterate the array for
compilation failures. The same shape applies to every throw site:

- `Document.fromMarkdown` — parse errors (missing `QUILL`, YAML errors,
  `parse::input_too_large` for inputs > 10 MB).
- `Document` mutators (`setField`, `updateLeafField`, etc.) — `EditError`
  variants (`ReservedName`, `InvalidFieldName`, `InvalidTagName`,
  `IndexOutOfRange`) appear in `diagnostics[0].message` with the
  `[EditError::<Variant>]` prefix.
- `quill.render` / `session.render` — backend compilation failures and
  validation errors.

### Lifecycle

The wasm bindings are built with `--weak-refs`, so dropped `Document`,
`Quill`, and `RenderSession` handles are reclaimed by `FinalizationRegistry`
without manual `.free()` discipline. `.free()` is still emitted as an eager
teardown hook for callers that want deterministic release. Requires
Node 14.6+ / current evergreen browsers (all supported targets).

For environments where `using` (the [explicit resource management][erm]
proposal) hasn't landed, use an explicit `try` / `finally`:

```ts
const session = quill.open(doc);
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

- Parsed markdown requires top-level `QUILL` in frontmatter. Empty input
  surfaces a dedicated "Empty markdown input cannot be parsed" message.
- QUILL mismatch during `quill.render(parsed)` is a warning (`quill::ref_mismatch`), not an error.
- Output schema APIs are no longer engine-level in WASM.

## License

Apache-2.0
