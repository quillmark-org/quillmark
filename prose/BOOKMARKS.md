# Bookmarks

Future work surfaced from consumer-perspective audits of the wasm API. Each
item is a known gap, not a planned change — capture only, no commitment.

## 1. Standalone validator on the wasm surface

Today the only path to "is this document valid against this quill?" is
`quill.form(doc)`, which walks the full schema and returns a
projection snapshot whose diagnostics array is the actual signal
(`crates/quillmark/src/form.rs:167,178`). The Rust core already has
`QuillConfig::validate_document` (`crates/core/src/quill/validation.rs`); it
just isn't `#[wasm_bindgen]`-exposed. MCP-style consumers that want to gate a
render on validity have to call `form` for its side effects. Expose a
`quill.validate(doc) -> Diagnostic[]`.

## 2. Stringly-typed field types in the schema payload

`crates/core/src/quill/types.rs:136` — `FieldType` serializes to bare strings
(`"string"`, `"integer"`, `"array"`, `"object"`, `"markdown"`). The wasm
`.d.ts` advertises return types via wasm-bindgen `unchecked_return_type =
"Leaf"` (`crates/bindings/wasm/src/engine.rs:530,538`) but the named type
isn't defined anywhere in the emitted declarations, so it collapses to `any`
for TS consumers. Either ship a real discriminated-union type alongside the
schema payload, or document that consumers must hand-write the TS interface.

## 3. `setField` / `setFill` are schema-blind

`crates/bindings/wasm/src/engine.rs:387-417` — both accept `JsValue` and only
require the value to deserialize as `serde_json::Value`. They never consult
the field's declared `type` or `enum`. A form input that submits the string
`"42"` for an `integer` field is silently accepted; the type mismatch
surfaces only at the next `form` call. A `setField` that returned
per-field diagnostics on the spot would close the loop at the input
boundary.

## 4. Diagnostic codes are unstable, undocumented strings

`crates/quillmark/src/form.rs:167,178` use bare literals
(`"form::unknown_leaf_kind"`, `"form::validation_error"`); edit errors
surface Rust variant names like `"ReservedName"`
(`crates/bindings/wasm/src/engine.rs:571-577`). No exported enum, no
constants, no stability guarantee. Consumers that key behavior off
`err.diagnostics[0].code` are one refactor away from breaking. Ship a
`DiagnosticCode` enum in the .d.ts and document the set as part of the
public API.

## 5. Render is sync with no cancellation

`crates/bindings/wasm/src/engine.rs:120-141` — `quill.render(doc, opts)`
blocks the wasm thread for the duration of compilation. There is no
`AbortSignal`, no progress callback, and the README does not document the
"host this in a Web Worker" requirement for browser consumers. At minimum a
`renderAsync(doc, { signal })` returning a `Promise` (even if the underlying
work is still synchronous wasm) would let consumers wrap their own timeouts
cleanly.

## 6. No engine-level capability discovery

`crates/bindings/wasm/src/engine.rs:90-92` — `new Quillmark()` exposes no
`.version`, no `.backends`, no `.supportedFormats()`. Backend availability
can only be inferred by loading a quill that exercises one
(`engine.rs:154,210-222`). For a server publishing a tool descriptor at
startup ("can produce PDF / SVG / PNG via Typst") this forces a
chicken-and-egg quill load. A static `Quillmark.capabilities()` is the fix.
