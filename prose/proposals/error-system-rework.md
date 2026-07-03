# Error System Rework

> **Tracking**: closes [#784](https://github.com/quillmark-org/quillmark/issues/784) (stages 2–3); supersedes its narrow fix.
> **Touches**: `crates/core/src/error.rs`, `crates/core/src/session.rs`, `crates/backends/typst/src/{compile,lib,error_mapping}.rs`, bindings, `prose/canon/{ERROR,PREVIEW}.md`.

## Problem

The diagnostic system has one currency (`Diagnostic`) and two rails that do not
meet:

- **Errors** are fully plumbed. Every `RenderError` variant carries a non-empty
  `Vec<Diagnostic>`; Typst compile errors map through `map_typst_errors` with
  resolved spans; both bindings flatten to one exception shape.
- **Warnings** are plumbed for two families and dropped for the third. Parse
  warnings (`Document::from_markdown_with_warnings`) and `validation::must_fill`
  are live producers, spliced into `RenderResult.warnings` by the CLI
  (`render.rs:148`) and the WASM one-shot render (`engine.rs:262-264`). But the
  session-side channels are dry — the `RenderResult::with_warning` and
  `LiveSession::with_warnings` builders have zero non-test callers — and
  Typst's compile warnings, handed over in exactly the right shape as
  `Warned { output, warnings }`, are `eprintln!`'d and discarded at
  `compile.rs:48-50` (#784). The drop affects `open`, `apply`, and one-shot
  `render`.

The root cause is structural: `compile_document` returns
`Result<PagedDocument, RenderError>`, and the success arm has no slot for
warnings. Warnings are opt-in per producer via bolt-on builders, so the path of
least resistance is to drop them.

Secondary costs of the current shape:

- `RenderError`'s nine variants all carry the identical payload; `diagnostics()`
  and `into_diagnostics()` are 9-arm matches over one binding pattern. The
  machine-routable identity is already the diagnostic `code`
  (`parse::*`, `validation::*`, `typst::*`, `quill::*`, `backend::*`,
  `engine::*`); the variant adds a second, weaker taxonomy that the bindings
  flatten away (Python raises one `QuillmarkError`; WASM throws one shape).
- `Severity::Note` has zero producers yet is carried through core, the CLI, and
  both binding enums.
- The "always non-empty" invariant on `diags` is enforced by comment at nine
  construction sites.

## Target shape

### One failure type

`RenderError` becomes a struct; the variant taxonomy is deleted:

```rust
pub struct RenderError {
    diags: Vec<Diagnostic>,   // non-empty, held by the constructor
}

impl RenderError {
    pub fn new(diags: Vec<Diagnostic>) -> Self;          // debug_asserts non-empty
    pub fn from_diag(diag: Diagnostic) -> Self;
    pub fn diagnostics(&self) -> &[Diagnostic];
    pub fn into_diagnostics(self) -> Vec<Diagnostic>;
}
```

Consumers route on `code`, not variant. Every construction site already sets a
namespaced code (audited in stage 1); the variant carries no information the
code does not. `Display` derives from the diagnostics alone: the primary
message for one diagnostic, an `"N error(s): <first>"` aggregate for more —
the rule `WasmError::message` already applies.

The tradeoff, accepted deliberately: external Rust consumers lose typed,
exhaustive matching over failure kinds and route on string codes with no
compile-time existence guarantee. Pre-1.0, with the bindings as the dominant
consumer surface and the only in-tree variant matches being tests plus
Python's message-prefix picker, the taxonomy costs more than it informs.

`ParseError` stays as the parser's leaf error type (its structured fields feed
the YAML-hint enrichment pass); the `From<ParseError> for RenderError`
conversion retargets to the struct. Leaf error types with boundary translation
(`PdfError`, `ParseError`) are the pattern, not the exception.

### Two severities

`Severity::Note` is removed. `Error` blocks the stage that emits it; `Warning`
never does. Fatality policy is this fixed rule — no lint levels, no
warning-to-error promotion, no per-code suppression. A note, when one is
needed, is a `hint` on the diagnostic it annotates.

### Warnings ride the compile seam

`compile_document` returns the warnings Typst already produces:

```rust
pub(crate) fn compile_document(world: &QuillWorld)
    -> Result<(PagedDocument, Vec<Diagnostic>), RenderError>
```

Warnings map through the same span-resolution as errors (`error_mapping.rs`
already maps `typst::diag::Severity` both ways); the `eprintln!` drop is
retired. The `typst::{message-prefix}` code heuristic applies to warnings as
it does to errors. The rule this instantiates: a boundary return is at least
as wide as what its dependency hands it. Typst hands `Warned`; the seam
carries it.

Compile warnings join the existing parse warnings in `RenderResult.warnings`.
Ordering is deliberate and pipeline-ordered: parse warnings first (the CLI and
WASM one-shot paths already prepend them), then compile warnings. No dedup —
the two families cannot overlap (parse warnings anchor `location` in the
markdown source, compile warnings in Typst sources).

Validation needs no change — `Quill::validate` already returns
`Vec<Diagnostic>` mixing severities and stays the editor-facing surface for
`validation::must_fill`. The render pipeline continues to zero-fill rather
than warn on incomplete documents.

### Session warnings are compile state

`LiveSession.warnings` is redefined from "open-time snapshot" to **warnings of
the current compile**, swapped transactionally with it:

- `SessionHandle` gains `fn warnings(&self) -> &[Diagnostic] { &[] }` — a
  slice, so `LiveSession::warnings() -> &[Diagnostic]` keeps its signature and
  the WASM getter is untouched. `TypstSession` stores the compile's warnings
  beside `document` / `page_hashes` and swaps all three together in `apply` —
  a failed apply keeps the last-good compile *and* its last-good warnings, by
  the same invariant that protects reads. pdfform keeps the empty default.
- `LiveSession` drops its `warnings` field and the `with_warnings` builder;
  `LiveSession::warnings()` delegates to the handle. `render()` keeps appending
  session warnings to `RenderResult.warnings`, which makes the one-shot path
  (`open` → `render`) surface compile warnings with no further plumbing.
- `ChangeSet` is unchanged: `{page_count, dirty_pages}` stays pure geometry.

## Explicitly not built

- A generic `Outcome<T>`/`Warned<T>` wrapper for every stage. Error-only stages
  (parse, quill config, page rendering, stamping) keep `Result<T, RenderError>`;
  only the compile seam returns a tuple. The boundary-width rule above is held
  by review, not by a universal type.
- A `kind()` accessor or any variant-taxonomy replacement. Codes are the
  routing surface.
- A code registry, `Code` newtype, or drift-guard test. Codes that both a
  producer and a consumer name become consts; the rest stay literals.
- Multi-anchor diagnostics / LSP `relatedInformation`. No producer emits a
  two-location diagnostic; the serde shape can grow a field later.
- A non-empty-vec type. The constructor holds the invariant.

## Stages

Each stage passes `cargo test --workspace` **and** the CI clippy gate
(`cargo clippy --all-features --all-targets -- -D warnings`) on its own;
commit per stage. The compile and session seams land as one stage — splitting
them leaves `TypstSession.compile_warnings` written but unread, which
`-D warnings` denies as dead code.

### 1. Core types (`crates/core/src/error.rs`)

- Collapse `RenderError` to the struct; delete the 9-arm matches; port
  `Display` to the count-based rule (updates the aggregate-message assertion
  at `error.rs:548`).
- Remove `Severity::Note`; update the serde round-trip tests
  (`wasm/types.rs:355,368`).
- Every construction site already sets a namespaced `code` (audited:
  `engine::backend_not_found`, `backend::apply_unsupported`,
  `quill::{name,version}_mismatch`, `typst::*`, `validation::*`, `parse::*`,
  pdfform via `engine_err`/`map_pdf_err`); re-verify as sites move.
- Rewrite variant-matching consumers to codes: Python's summary-prefix match
  (`bindings/python/src/errors.rs:53-63`) becomes the count-based message rule
  (aligning `str(exc)` with the WASM rule canon already documents);
  tests (`sig_field.rs:265`, `version_mismatch_test.rs:54`,
  `quill_engine_test.rs:64,114`, `quiver_test.rs:64,108`,
  `usaf_memo_signature_test.rs:77`) assert on `code`.

### 2. Compile and session seams (`backends/typst`, `core/src/session.rs`)

- `compile_document` → `Result<(PagedDocument, Vec<Diagnostic>), RenderError>`
  (`pub(crate)`, two callers, both in the typst crate); map warnings through
  `error_mapping.rs` with spans; delete the `eprintln!` loop.
- `TypstSession` stores `compile_warnings`; `open` (`lib.rs:296`) and `apply`
  (`lib.rs:194`) thread them; `apply` swaps them transactionally with the
  document.
- Add `SessionHandle::warnings(&self) -> &[Diagnostic]` (default `&[]`;
  pdfform keeps the default); `TypstSession` returns the stored slice.
- Delete `LiveSession::{warnings field, with_warnings}`; delegate
  `warnings()` to the handle; update the doc contract on `warnings()`,
  `apply()`, and `render()`.

### 3. Bindings and CLI

- WASM: drop `Note` from `types.rs` `Severity` and the TS enum; update the
  `warnings` getter doc (`engine.rs:1360-1372`) to the current-compile
  contract.
- Python: drop `PySeverity::NOTE`; exception shape (`.diagnostics`, codes) is
  unchanged; message text changes per stage 1.
- CLI: `validate.rs` local severity, `errors.rs` printing — mechanical.

### 4. Prose and migration

- Rewrite `prose/canon/ERROR.md` (types, one failure shape, warning flow);
  update `prose/canon/PREVIEW.md` (`warnings` accessor bullet, lifecycle
  example).
- Working migration guide (`docs/migrations/0.92-to-0.93.md`): `severity:
  "note"` removed from the wire; `RenderError` variants removed (route on
  `diagnostics[*].code`); multi-error `Display`/`str(exc)` text now
  count-based in Rust and Python; `LiveSession.warnings` now refreshes per
  committed apply. **Reconcile, don't append**: the guide already names
  `RenderError::ApplyUnsupported` as a variant (line 358) — that section must
  be rewritten to the code, not contradicted later in the same guide.
- Close #784; delete this proposal.

## Binding-surface impact

Small, because both bindings already flatten errors to one shape. Visible
changes: the `"note"` severity value disappears from the wire enums;
Typst compile warnings start arriving in `RenderResult.warnings` (after parse
warnings) and in `LiveSession.warnings`, which previously held only open-time
state; multi-error exception/`Display` messages become count-based; Rust
callers matching `RenderError` variants switch to codes.
