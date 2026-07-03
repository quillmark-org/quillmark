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
    let message = format!("[EditError::{}] {}", err.variant_name(), err);
    raise_with_diagnostics(
        vec![Diagnostic::new(Severity::Error, message.clone())],
        message,
    )
}

/// Batched-mutator twin of [`convert_edit_error`]: one diagnostic per
/// offending field, each with `path` set to the field name. The exception
/// message embeds the first diagnostic (same shape as WASM's
/// `WasmError::message`), so the `[EditError::<Variant>]` prefix contract
/// holds for batches too.
pub fn convert_edit_errors(errors: Vec<(String, EditError)>) -> PyErr {
    let diags: Vec<Diagnostic> = errors
        .into_iter()
        .map(|(name, err)| {
            Diagnostic::new(
                Severity::Error,
                format!("[EditError::{}] {}", err.variant_name(), err),
            )
            .with_path(name)
        })
        .collect();
    let message = match diags.as_slice() {
        [only] => only.message.clone(),
        _ => format!("{} error(s): {}", diags.len(), diags[0].message),
    };
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
