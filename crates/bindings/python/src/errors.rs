//! Single-exception, uniform-diagnostic error contract.
//!
//! Mirrors the WASM binding's `WasmError`: every raised exception is
//! `QuillmarkError` and carries a non-empty `.diagnostics` list. The
//! `EditError::<Variant>` prefix lives in the exception message, not the type.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use quillmark_core::{Diagnostic, EditError, RenderError, Severity};

create_exception!(_quillmark, QuillmarkError, PyException);

pub fn convert_edit_error(err: EditError) -> PyErr {
    let diagnostic =
        Diagnostic::new(Severity::Error, err.to_string()).with_code(err.code().to_string());
    let message = diagnostic.message.clone();
    raise_with_diagnostics(vec![diagnostic], message)
}

/// Batched-mutator twin of [`convert_edit_error`]: one diagnostic per
/// offending field, each carrying the `edit::` code and `path` set to the
/// field name. The exception message follows the shared count-based rule
/// (same shape as WASM's `WasmError::message`).
pub fn convert_edit_errors(errors: Vec<(String, EditError)>) -> PyErr {
    let diags: Vec<Diagnostic> = errors
        .into_iter()
        .map(|(name, err)| {
            Diagnostic::new(Severity::Error, err.to_string())
                .with_code(err.code().to_string())
                .with_path(name)
        })
        .collect();
    let message = RenderError::summary_message(&diags);
    raise_with_diagnostics(diags, message)
}

/// The exception message follows the count-based rule shared with the WASM
/// binding and `RenderError`'s own `Display`: the primary diagnostic's message
/// for a single diagnostic, an `"<N> error(s): <first>"` aggregate for more.
pub fn convert_render_error(err: RenderError) -> PyErr {
    debug_assert!(
        !err.diagnostics().is_empty(),
        "RenderError always carries at least one diagnostic"
    );
    let message = err.to_string();
    raise_with_diagnostics(err.into_diagnostics(), message)
}

/// Construct a `QuillmarkError` whose `.diagnostics` attribute lists every
/// diagnostic the underlying error carried.
pub fn raise_with_diagnostics(diags: Vec<Diagnostic>, message: String) -> PyErr {
    Python::attach(|py| {
        let py_err = QuillmarkError::new_err(message);
        let py_diags: Vec<crate::types::PyDiagnostic> = diags
            .into_iter()
            .map(|d| crate::types::PyDiagnostic { inner: d })
            .collect();
        if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
            let _ = exc.setattr("diagnostics", py_diags);
        }
        py_err
    })
}
