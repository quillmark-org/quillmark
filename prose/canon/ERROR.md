# Error Handling System

> **Implementation**: `crates/core/src/error.rs`

## Types

**`Severity`**: `Error` | `Warning` | `Note`

**`Location`**: file name, line (1-indexed), column (1-indexed)

**`Diagnostic`**: severity, optional error `code`, `message`, optional `location` (text anchor: file/line/column), optional `path` (document-model anchor — dotted/bracketed path into the typed `Document`, set by schema validation/coercion), optional `hint`, `source_chain` (omitted from serialization when empty). `location` and `path` are independent and may co-exist.

**`ParseError`**: parsing-stage error enum — `InputTooLarge`, `InvalidStructure`, `EmptyInput`, `MissingQuill`, `InvalidQuillReference`, `YamlErrorWithLocation`; converts to `Diagnostic` via `to_diagnostic()`. The `InvalidQuillReference` case (`parse::invalid_quill_reference`) attaches the canonical `$quill` grammar — `quill_ref_hint()` — as the diagnostic hint. That hint is the single source of truth for the reference grammar: bindings surface it verbatim (e.g. WASM `Document.quillRefHint`) rather than re-stating the rule.

**`RenderError`**: main rendering error enum. Every variant carries the same
payload — a non-empty `diags: Vec<Diagnostic>` — so all consumers (and all
language bindings) handle errors through one code path. The variant records
only the *kind* of failure; `diagnostics()` borrows the vector and
`into_diagnostics()` consumes the error into it. Variants:
- `EngineCreation` — failed to create engine
- `InvalidPayload` — malformed YAML in a card-yaml block (also wraps `ParseError`)
- `CompilationFailed` — backend compilation failed
- `FormatNotSupported` — requested output format not supported
- `UnsupportedBackend` — backend not registered
- `ValidationFailed` — field coercion/schema validation failure
- `QuillConfig` — Quill.yaml configuration error
- `QuillMismatch` — the document was rendered with a quill that does not satisfy its `$quill` reference (wrong name, or version outside the selector). Distinct from `ValidationFailed`: the document is well-formed, but paired with the wrong quill. See [VERSIONING.md](VERSIONING.md).

`ValidationFailed`, `QuillConfig`, and `CompilationFailed` routinely carry
several diagnostics so every problem reaches the caller in one pass; the
remaining kinds are inherently single-diagnostic and carry a one-element
vector.

**`RenderResult`**: successful result carrying artifacts, output format, and non-fatal `Vec<Diagnostic>` warnings

## Bindings Error Delegation

Python and WASM bindings delegate to core types:

- **Python**: `PyDiagnostic` wraps `Diagnostic`. Every raised exception is `QuillmarkError` (a single type; no subclasses per variant). Every exception carries a `diagnostics` list. Base hierarchy: `QuillmarkError → PyException`.
- **WASM**: `WasmError` carries a single `diagnostics: Vec<Diagnostic>` (always non-empty). The thrown JS `Error` has a `.diagnostics` array attached and a `.message` derived from `diagnostics`: `diagnostics[0].message` for single-diagnostic errors, an aggregate `"<N> error(s): <first.message>"` summary whenever there is more than one diagnostic (any variant — compilation, validation, or config). Same shape regardless of underlying variant; consumers read `err.diagnostics[0]` for the primary diagnostic and iterate `err.diagnostics` for the rest. Parse failures (`Document.fromMarkdown`) carry the same shape — including the `parse::input_too_large` diagnostic for inputs over `MAX_INPUT_SIZE` (10 MB) and the various `EditError::*` variants for post-parse mutators.

## Backend Error Mapping

### Typst

Typst diagnostics mapped via `map_typst_errors()`:
- Severity levels mapped (Error/Warning)
- Spans resolved to file/line/column
- Error codes: `"typst::<error_type>"`

See `crates/backends/typst/src/error_mapping.rs`.

## Validation message contract

Field-level validation diagnostics — `validation::type_mismatch`,
`validation::field_absent`, and
`validation::must_fill_sentinel` — emit a single canonical shape:

- **Field path** — the document-model anchor of the offending field
  (`recipient`, `cards[2].author`).
- **Source token** — the YAML scalar that triggered the error, rendered
  verbatim in its YAML-canonical form (`42`, `null`, `true`, `""`). The
  blueprint's `<must-fill>` sentinel renders verbatim. Strings appear quoted;
  primitives appear bare. (Absent fields have no source token.)
- **Schema declaration** — the field's declared type and, when present,
  its default. Defaults render with the same verbatim formatting.
- **Both exits when applicable** — the message names two ways out. The
  parser does not silently coerce; the message is the lever.

Example messages:

```
Field `build_number` got integer `42`, schema declares `string`.
Either quote the value (`build_number: "42"`) or change the schema's
`type:` to `integer`.
```

```
Field `subtitle` got `null`, schema declares `string` with default
`"My Subtitle"`. To use the default, delete this entire line (do NOT
write `subtitle:`, `subtitle: null`, or `subtitle: ~` — all three parse
as null). To set an explicit value, replace the right-hand side with a
string.
```

```
Field `memo_for` is missing, schema declares `string` with no default.
Provide a value of type `string`.
```

```
Field `name` still carries the `<must-fill>` blueprint sentinel,
schema declares `string`. Replace `<must-fill>` with a value of type
`string`.
```

Implementation: `crates/core/src/quill/validation.rs` (the
`ValidationError` `Display` impl).

## Error Presentation

**Pretty printing** (`Diagnostic::fmt_pretty()`):
```
[ERROR] Undefined variable (E001)
  --> template.typ:10:5
  hint: Check variable spelling
```

**Extended printing** (`Diagnostic::fmt_pretty_with_source()`): appends each cause in the source chain as `cause N: <message>`.

**Consolidated printing**: `print_errors()` handles all `RenderError` variants.

**Machine-readable**: all diagnostic types implement `serde::Serialize`.
