# Error Handling

Every failure — parse, validation, quill config, backend compile — travels as a **`Diagnostic`**, and each binding raises a single error type that always carries a non-empty `diagnostics` list. Consumers route on a diagnostic's namespaced `code`, never on an exception subclass.

## The Diagnostic shape

| Field | Meaning |
|---|---|
| `severity` | `Error` (blocks its stage) or `Warning` (never blocks) |
| `code` | Namespaced id — e.g. `parse::missing_quill`, `validation::type_mismatch`, `quill::version_mismatch` — the machine-routable identity |
| `message` | Human-readable text |
| `location` | Optional text anchor: `file`, `line` (1-based), `column` |
| `path` | Optional document-model anchor: `main.recipient`, `cards.indorsement[0].author` |
| `hint` | Optional actionable suggestion |

`location` (where in the source text) and `path` (which field in the model) are independent and may co-exist.

## Catching errors

=== "Python"

    ```python
    from quillmark import QuillmarkError

    try:
        result = engine.render(quill, doc, OutputFormat.PDF)
    except QuillmarkError as exc:
        for d in exc.diagnostics:            # always non-empty
            print(d.severity, d.code, d.message)
            if d.path:
                print("  at", d.path)
    ```

=== "JavaScript"

    ```javascript
    try {
      const result = await engine.render(quill, doc, { format: "pdf" });
    } catch (err) {
      for (const d of err.diagnostics) {     // always non-empty
        console.error(d.severity, d.code, d.message);
      }
    }
    ```

A multi-problem stage (validation, quill config, backend compile) reports **every** problem in one pass, so `diagnostics` may carry several entries; `diagnostics[0]` is the primary. The error's `message` follows a count-based rule: the primary message for one diagnostic, `"<N> error(s): <first message>"` for more.

## Codes, not types

The `code` namespaces are the routing surface:

`parse::*` · `validation::*` · `quill::*` · `edit::*` (mutators) · `typst::*` · `pdfform::*` · `backend::*` · `engine::*`.

Notable codes: `quill::name_mismatch` / `quill::version_mismatch` (a well-formed document paired with the wrong quill — see [Versioning](../quills/versioning.md)); `engine::backend_not_found` (the quill's declared backend is not registered); `parse::input_too_large` (input over 10 MB).

## Warnings vs errors

Fatality is a two-value ladder: `Error` blocks the stage that emits it; `Warning` never does. There is no lint-level configuration and no warning-to-error promotion. Warnings ride the same `Diagnostic` currency on non-fatal channels:

- **Parse warnings** — e.g. a `~~~` opener missing its blank line — carried on the parsed document (`doc.warnings`) and spliced into a render's warnings.
- **Validation warnings** — `quill.validate(doc)` returns every diagnostic; `validation::must_fill` (an outstanding `!must_fill` marker) and the `$seed` checks are the non-fatal ones. The render path never gates on incompleteness — an absent field zero-fills.
- **Compile warnings** — a backend's non-fatal diagnostics (font fallback, overfull pages), carried on `result.warnings`.

A successful render returns artifacts **and** a `warnings` list, so inspect it even on success.

Full model: [ERROR.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/ERROR.md).
