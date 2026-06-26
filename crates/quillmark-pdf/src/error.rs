//! The stamp spine's own error type.
//!
//! `quillmark-pdf` is leaf infra and owns a `PdfError` rather than returning
//! `quillmark_core::RenderError` with backend-flavoured codes — that inversion
//! is not carried forward. Each backend maps `PdfError` to `RenderError` at its
//! boundary. Every failure here is a single `code` + `message`, so a struct
//! (not a sprawling enum) is the honest shape.

/// An error from the stamp spine. Carries a stable `code` (a `pdf::*` string a
/// consumer can match on) and a human-readable `message`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PdfError {
    /// Stable error code, e.g. `pdf::xref_stream`, `pdf::encrypted`.
    pub code: &'static str,
    /// Human-readable description.
    pub message: String,
}

impl PdfError {
    /// Build a `PdfError` from a code and message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
