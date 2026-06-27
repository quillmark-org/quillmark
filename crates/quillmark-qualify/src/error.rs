//! The qualification layer's own error type.
//!
//! `quillmark-qualify` is build-time tooling and owns a `QualifyError` rather
//! than borrowing the stamp spine's `PdfError` or `lopdf::Error` — the caller
//! (a CLI subcommand) wants a small, stable set of failure modes it can present
//! clearly. Each variant maps a distinct stage of the pipeline.

use std::fmt;

/// Why a qualification failed.
#[derive(Debug)]
pub enum QualifyError {
    /// The input bytes were not a parseable PDF (malformed structure, truncated,
    /// not a PDF at all).
    Malformed(String),
    /// The PDF is encrypted and decryption failed — usually a wrong or missing
    /// password.
    Decrypt(String),
    /// A field shape this layer does not (yet) support was encountered and could
    /// not be skipped — reserved for hard, unrecoverable shape errors. Note that
    /// *individually* skippable shapes (radio, pushbutton, unknown `/FT`) are
    /// dropped with a note, not surfaced as this error.
    UnsupportedField(String),
    /// An internal/IO error while re-serializing the stripped background.
    Internal(String),
}

impl fmt::Display for QualifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QualifyError::Malformed(m) => write!(f, "malformed PDF: {m}"),
            QualifyError::Decrypt(m) => write!(f, "could not decrypt PDF: {m}"),
            QualifyError::UnsupportedField(m) => write!(f, "unsupported form field: {m}"),
            QualifyError::Internal(m) => write!(f, "internal qualification error: {m}"),
        }
    }
}

impl std::error::Error for QualifyError {}
