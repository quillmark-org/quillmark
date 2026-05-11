//! Error handling utilities for WASM bindings

use crate::types::Diagnostic as WasmDiagnostic;
use quillmark_core::{Diagnostic, ParseError, RenderError, Severity};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Serializable error for JavaScript consumption.
///
/// Single uniform shape regardless of underlying error variant: a non-empty
/// list of [`Diagnostic`]s. The thrown JS `Error`'s `.message` is derived
/// from `diagnostics` (`diagnostics[0].message` for single-diagnostic
/// errors, an aggregate `"… N error(s)"` summary for compilation failures),
/// and a `.diagnostics` property carries the full array.
///
/// Read `err.diagnostics[0]` for the primary diagnostic; iterate the array
/// for backend compilation failures.
#[derive(Debug, Clone)]
pub struct WasmError {
    pub diagnostics: Vec<Diagnostic>,
}

impl WasmError {
    /// Display message for the JS `Error` constructor.
    ///
    /// For single-diagnostic errors this is the diagnostic's `message`. For
    /// multi-diagnostic errors (backend compilation) this is an aggregate
    /// `"… N error(s)"` summary; callers should iterate `diagnostics` for
    /// the per-error details.
    pub fn message(&self) -> String {
        match self.diagnostics.len() {
            0 => "Unknown error".to_string(),
            1 => self.diagnostics[0].message.clone(),
            n => format!("{} error(s): {}", n, self.diagnostics[0].message),
        }
    }

    /// Convert to a JS `Error` object for throwing.
    ///
    /// Returns a real `Error` whose `.message` is [`WasmError::message`] and
    /// whose `.diagnostics` property is an array of diagnostic objects
    /// matching the shape used in `RenderResult.warnings`.
    pub fn to_js_value(&self) -> JsValue {
        let err = js_sys::Error::new(&self.message());
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        let wasm_diags: Vec<WasmDiagnostic> =
            self.diagnostics.iter().cloned().map(Into::into).collect();
        if let Ok(data) = wasm_diags.serialize(&serializer) {
            let _ = js_sys::Reflect::set(&err, &JsValue::from_str("diagnostics"), &data);
        }
        err.into()
    }
}

impl From<ParseError> for WasmError {
    fn from(error: ParseError) -> Self {
        WasmError {
            diagnostics: vec![error.to_diagnostic()],
        }
    }
}

impl From<RenderError> for WasmError {
    fn from(error: RenderError) -> Self {
        match error {
            // Multi-diagnostic variants forward every diagnostic so JS
            // consumers can iterate `err.diagnostics` and read each entry's
            // `path` for document-model navigation.
            RenderError::CompilationFailed { diags }
            | RenderError::QuillConfig { diags }
            | RenderError::ValidationFailed { diags } => WasmError { diagnostics: diags },
            _ => {
                let diagnostic = error
                    .diagnostics()
                    .first()
                    .map(|d| (*d).clone())
                    .unwrap_or_else(|| Diagnostic::new(Severity::Error, error.to_string()));
                WasmError {
                    diagnostics: vec![diagnostic],
                }
            }
        }
    }
}

impl From<String> for WasmError {
    fn from(message: String) -> Self {
        WasmError {
            diagnostics: vec![Diagnostic::new(Severity::Error, message)],
        }
    }
}

impl From<&str> for WasmError {
    fn from(message: &str) -> Self {
        WasmError::from(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_too_large_conversion() {
        let err = ParseError::InputTooLarge {
            size: 1_000_000,
            max: 100_000,
        };
        let wasm_err: WasmError = err.into();

        assert_eq!(wasm_err.diagnostics.len(), 1);
        let diag = &wasm_err.diagnostics[0];
        assert_eq!(diag.code.as_deref(), Some("parse::input_too_large"));
        assert!(diag.message.contains("Input too large"));
        assert_eq!(wasm_err.message(), diag.message);
    }

    #[test]
    fn test_compilation_failed_carries_all_diagnostics() {
        let diag1 = Diagnostic::new(Severity::Error, "Error 1".to_string());
        let diag2 = Diagnostic::new(Severity::Error, "Error 2".to_string());
        let render_err = RenderError::CompilationFailed {
            diags: vec![diag1, diag2],
        };
        let wasm_err: WasmError = render_err.into();

        assert_eq!(wasm_err.diagnostics.len(), 2);
        assert_eq!(wasm_err.diagnostics[0].message, "Error 1");
        assert_eq!(wasm_err.diagnostics[1].message, "Error 2");
        let summary = wasm_err.message();
        assert!(summary.contains("2"));
        assert!(summary.contains("Error 1"));
    }

    #[test]
    fn test_validation_failed_carries_all_diagnostics() {
        // Regression: prior to the PR-#566 fix, the `From<RenderError>`
        // fallback called `.diagnostics().first()` and dropped every
        // diagnostic after the first for non-CompilationFailed variants.
        let diag1 = Diagnostic::new(Severity::Error, "missing title".to_string())
            .with_path("title".to_string());
        let diag2 = Diagnostic::new(Severity::Error, "missing author".to_string())
            .with_path("author".to_string());
        let render_err = RenderError::ValidationFailed {
            diags: vec![diag1, diag2],
        };
        let wasm_err: WasmError = render_err.into();

        assert_eq!(wasm_err.diagnostics.len(), 2);
        assert_eq!(wasm_err.diagnostics[0].path.as_deref(), Some("title"));
        assert_eq!(wasm_err.diagnostics[1].path.as_deref(), Some("author"));
    }

    #[test]
    fn test_quill_config_carries_all_diagnostics() {
        // Same regression as above, for `QuillConfig`.
        let diag1 = Diagnostic::new(Severity::Error, "config error 1".to_string());
        let diag2 = Diagnostic::new(Severity::Error, "config error 2".to_string());
        let render_err = RenderError::QuillConfig {
            diags: vec![diag1, diag2],
        };
        let wasm_err: WasmError = render_err.into();

        assert_eq!(wasm_err.diagnostics.len(), 2);
        assert_eq!(wasm_err.diagnostics[0].message, "config error 1");
        assert_eq!(wasm_err.diagnostics[1].message, "config error 2");
    }

    #[test]
    fn test_string_conversion_yields_single_diagnostic() {
        let wasm_err: WasmError = "Simple error".into();
        assert_eq!(wasm_err.message(), "Simple error");
        assert_eq!(wasm_err.diagnostics.len(), 1);
        assert_eq!(wasm_err.diagnostics[0].message, "Simple error");
    }
}
