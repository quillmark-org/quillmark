use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use quillmark_core::{Diagnostic, EditError, RenderError};

// Base exception
create_exception!(_quillmark, QuillmarkError, PyException);

// Specific exceptions
create_exception!(_quillmark, ParseError, QuillmarkError);
create_exception!(_quillmark, TemplateError, QuillmarkError);
create_exception!(_quillmark, CompilationError, QuillmarkError);

// Python exception for editor-surface errors.
// Raised by `Document` and `Card` mutators when an invariant is violated.
// The exception message includes the `EditError` variant name and details.
create_exception!(_quillmark, PyEditError, QuillmarkError);

/// Convert an [`EditError`] to a `PyErr` (raises `quillmark.EditError`).
pub fn convert_edit_error(err: EditError) -> PyErr {
    let variant = match &err {
        EditError::ReservedName(_) => "ReservedName",
        EditError::InvalidFieldName(_) => "InvalidFieldName",
        EditError::InvalidTagName(_) => "InvalidTagName",
        EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
    };
    PyEditError::new_err(format!("[EditError::{}] {}", variant, err))
}

fn with_diag_attached(py: Python, py_err: PyErr, diag: Diagnostic) -> PyErr {
    if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
        let py_diag = crate::types::PyDiagnostic { inner: diag.into() };
        let _ = exc.setattr("diagnostic", py_diag);
    }
    py_err
}

pub fn convert_render_error(err: RenderError) -> PyErr {
    Python::attach(|py| match err {
        RenderError::CompilationFailed { diags } => {
            let py_err = CompilationError::new_err(format!(
                "Compilation failed with {} error(s)",
                diags.len()
            ));
            if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
                let py_diags: Vec<crate::types::PyDiagnostic> = diags
                    .into_iter()
                    .map(|d| crate::types::PyDiagnostic { inner: d.into() })
                    .collect();
                let _ = exc.setattr("diagnostics", py_diags);
            }
            py_err
        }
        RenderError::QuillConfig { diags } => {
            let msg = if diags.len() == 1 {
                diags[0].message.clone()
            } else {
                format!("Quill configuration has {} error(s)", diags.len())
            };
            let py_err = QuillmarkError::new_err(msg);
            if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
                let py_diags: Vec<crate::types::PyDiagnostic> = diags
                    .into_iter()
                    .map(|d| crate::types::PyDiagnostic { inner: d.into() })
                    .collect();
                let _ = exc.setattr("diagnostics", py_diags);
            }
            py_err
        }
        RenderError::ValidationFailed { diags } => {
            // Multi-diagnostic: each entry sets `path` to anchor the error
            // in the document model. Consumers should iterate `.diagnostics`.
            //
            // Pre-PR-#566 versions of this binding attached a single
            // `.diagnostic` for `ValidationFailed`. To soften that
            // breaking change for the common single-error case, we also
            // attach `.diagnostic = diagnostics[0]` when there is exactly
            // one diagnostic. New code should prefer `.diagnostics`.
            let msg = if diags.len() == 1 {
                diags[0].message.clone()
            } else {
                format!("Validation failed with {} error(s)", diags.len())
            };
            let py_err = QuillmarkError::new_err(msg);
            if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
                let py_diags: Vec<crate::types::PyDiagnostic> = diags
                    .into_iter()
                    .map(|d| crate::types::PyDiagnostic { inner: d.into() })
                    .collect();
                if py_diags.len() == 1 {
                    let _ = exc.setattr("diagnostic", py_diags[0].clone());
                }
                let _ = exc.setattr("diagnostics", py_diags);
            }
            py_err
        }
        RenderError::InvalidFrontmatter { diag } => {
            with_diag_attached(py, ParseError::new_err(diag.message.clone()), *diag)
        }
        RenderError::EngineCreation { diag }
        | RenderError::FormatNotSupported { diag }
        | RenderError::UnsupportedBackend { diag } => {
            with_diag_attached(py, QuillmarkError::new_err(diag.message.clone()), *diag)
        }
    })
}
