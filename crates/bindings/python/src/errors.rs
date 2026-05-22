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
        EditError::InvalidFieldName(_) => "InvalidFieldName",
        EditError::InvalidKindName(_) => "InvalidKindName",
        EditError::ReservedKind => "ReservedKind",
        EditError::IndexOutOfRange { .. } => "IndexOutOfRange",
    };
    PyEditError::new_err(format!("[EditError::{}] {}", variant, err))
}

pub fn convert_render_error(err: RenderError) -> PyErr {
    Python::attach(|py| {
        let diags = err.diagnostics();
        debug_assert!(
            !diags.is_empty(),
            "RenderError always carries at least one diagnostic"
        );

        // The variant kind selects the Python exception type and the summary
        // message; the diagnostic payload is uniform across every variant.
        let py_err = match &err {
            RenderError::CompilationFailed { diags } => CompilationError::new_err(format!(
                "Compilation failed with {} error(s)",
                diags.len()
            )),
            RenderError::InvalidPayload { diags } => ParseError::new_err(primary_message(diags)),
            RenderError::QuillConfig { diags } => {
                QuillmarkError::new_err(summary_message("Quill configuration has", diags))
            }
            RenderError::ValidationFailed { diags } => {
                QuillmarkError::new_err(summary_message("Validation failed with", diags))
            }
            RenderError::EngineCreation { diags }
            | RenderError::FormatNotSupported { diags }
            | RenderError::UnsupportedBackend { diags } => {
                QuillmarkError::new_err(primary_message(diags))
            }
        };

        // Every exception carries the full `.diagnostics` list; the
        // convenience `.diagnostic` singular is attached when there is
        // exactly one diagnostic. Each diagnostic's `path` anchors the
        // error in the document model for UI navigation.
        let py_diags: Vec<crate::types::PyDiagnostic> = err
            .into_diagnostics()
            .into_iter()
            .map(|d| crate::types::PyDiagnostic { inner: d.into() })
            .collect();
        if let Ok(exc) = py_err.value(py).downcast::<pyo3::types::PyAny>() {
            if let [only] = py_diags.as_slice() {
                let _ = exc.setattr("diagnostic", only.clone());
            }
            let _ = exc.setattr("diagnostics", py_diags);
        }
        py_err
    })
}

/// Message for single-diagnostic error kinds: the primary diagnostic's text.
fn primary_message(diags: &[Diagnostic]) -> String {
    diags
        .first()
        .map(|d| d.message.clone())
        .unwrap_or_else(|| "render error".to_string())
}

/// Message for multi-diagnostic error kinds: the lone diagnostic's text when
/// there is exactly one, otherwise an aggregate `"<prefix> N error(s)"`.
fn summary_message(prefix: &str, diags: &[Diagnostic]) -> String {
    match diags {
        [only] => only.message.clone(),
        _ => format!("{} {} error(s)", prefix, diags.len()),
    }
}
