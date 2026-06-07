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
    let variant = match &err {
        EditError::InvalidFieldName(_) => "InvalidFieldName",
        EditError::InvalidKindName(_) => "InvalidKindName",
        EditError::ReservedKind => "ReservedKind",
        EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
    };
    let message = format!("[EditError::{}] {}", variant, err);
    raise_with_diagnostics(vec![Diagnostic::new(Severity::Error, message.clone())], message)
}

pub fn convert_render_error(err: RenderError) -> PyErr {
    let diags = err.diagnostics();
    debug_assert!(
        !diags.is_empty(),
        "RenderError always carries at least one diagnostic"
    );

    let message = match &err {
        RenderError::CompilationFailed { diags } => {
            format!("Compilation failed with {} error(s)", diags.len())
        }
        RenderError::QuillConfig { diags } => summary_message("Quill configuration has", diags),
        RenderError::ValidationFailed { diags } => summary_message("Validation failed with", diags),
        RenderError::InvalidPayload { diags }
        | RenderError::EngineCreation { diags }
        | RenderError::FormatNotSupported { diags }
        | RenderError::UnsupportedBackend { diags }
        | RenderError::QuillMismatch { diags } => primary_message(diags),
    };

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

fn primary_message(diags: &[Diagnostic]) -> String {
    diags
        .first()
        .map(|d| d.message.clone())
        .unwrap_or_else(|| "render error".to_string())
}

fn summary_message(prefix: &str, diags: &[Diagnostic]) -> String {
    match diags {
        [only] => only.message.clone(),
        _ => format!("{} {} error(s)", prefix, diags.len()),
    }
}
