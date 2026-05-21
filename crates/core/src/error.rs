//! # Error Handling
//!
//! Structured error handling with diagnostics and source location tracking.
//!
//! ## Overview
//!
//! The `error` module provides error types and diagnostic types for actionable
//! error reporting with source location tracking.
//!
//! ## Key Types
//!
//! - [`RenderError`]: Main error enum for rendering operations

//! - [`Diagnostic`]: Structured diagnostic information
//! - [`Location`]: Source file location (file, line, column)
//! - [`Severity`]: Error severity levels (Error, Warning, Note)
//! - [`RenderResult`]: Result type with artifacts and warnings
//!
//! ## Document Anchors
//!
//! A [`Diagnostic`] can carry two independent "where" anchors, both optional:
//!
//! - [`Diagnostic::location`] — a source-text anchor (`file:line:column`).
//!   Produced by parsers and backend compilers that operate on raw text.
//! - [`Diagnostic::path`] — a document-model anchor pointing into the typed
//!   [`crate::document::Document`]. Produced by schema validation and
//!   coercion, which run on the typed model after line spans are gone.
//!
//! ### Path grammar
//!
//! ```text
//! path        := segment ( "." field_name | "[" index "]" )*
//! field_name  := [a-z_][a-z0-9_]*       // same charset enforced for fields/kinds
//! index       := [0-9]+
//! ```
//!
//! Because field names and card kinds are validated to that charset (no `.`,
//! `[`, `]`, or whitespace can appear in any segment), the dotted form
//! round-trips unambiguously.
//!
//! ### Path conventions
//!
//! | Anchor                        | Path                                          |
//! |-------------------------------|-----------------------------------------------|
//! | Root-block field              | `title`                                       |
//! | Nested in array of objects    | `recipients[0].name`                          |
//! | Main card body                | `main.body`                                   |
//! | Typed card (whole)            | `cards.indorsement[0]`                        |
//! | Field on typed card           | `cards.indorsement[0].signature_block`        |
//! | Body on typed card            | `cards.indorsement[0].body`                   |
//! | Card with unknown kind        | `cards[0]`                                    |
//!
//! The `cards.<kind>[<index>]` form fuses the card kind and the document
//! array index into one segment so consumers receive both pieces of
//! information without a second lookup.
//!
//! ## Error Hierarchy
//!
//! ### RenderError Variants
//!
//! - [`RenderError::EngineCreation`]: Failed to create rendering engine
//! - [`RenderError::InvalidPayload`]: Malformed YAML in a card-yaml block
//! - [`RenderError::CompilationFailed`]: Backend compilation errors
//! - [`RenderError::FormatNotSupported`]: Requested format not supported
//! - [`RenderError::UnsupportedBackend`]: Backend not registered
//! - [`RenderError::ValidationFailed`]: Field coercion/validation failure
//! - [`RenderError::QuillConfig`]: Quill configuration error
//!
//! ## Examples
//!
//! ### Creating Diagnostics
//!
//! ```
//! use quillmark_core::{Diagnostic, Location, Severity};
//!
//! let diag = Diagnostic::new(Severity::Error, "Undefined variable".to_string())
//!     .with_code("E001".to_string())
//!     .with_location(Location {
//!         file: "template.typ".to_string(),
//!         line: 10,
//!         column: 5,
//!     })
//!     .with_hint("Check variable spelling".to_string());
//!
//! println!("{}", diag.fmt_pretty());
//! ```
//!
//! Example output:
//! ```text
//! [ERROR] Undefined variable (E001) at template.typ:10:5
//!   hint: Check variable spelling
//! ```
//!
//! ### Result with Warnings
//!
//! ```no_run
//! # use quillmark_core::{RenderResult, Diagnostic, Severity, OutputFormat};
//! # let artifacts = vec![];
//! let result = RenderResult::new(artifacts, OutputFormat::Pdf)
//!     .with_warning(Diagnostic::new(
//!         Severity::Warning,
//!         "Deprecated field used".to_string(),
//!     ));
//! ```
//!
//! ## Pretty Printing
//!
//! The [`Diagnostic`] type provides [`Diagnostic::fmt_pretty()`] for human-readable output with error code, location, and hints.
//!
//! ## Machine-Readable Output
//!
//! All diagnostic types implement `serde::Serialize` for JSON export:
//!
//! ```no_run
//! # use quillmark_core::{Diagnostic, Severity};
//! # let diagnostic = Diagnostic::new(Severity::Error, "Test".to_string());
//! let json = serde_json::to_string(&diagnostic).unwrap();
//! ```

use crate::OutputFormat;

/// Maximum input size for markdown (10 MB)
pub const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024;

/// Maximum YAML size (1 MB)
pub const MAX_YAML_SIZE: usize = 1024 * 1024;

/// Maximum nesting depth for markdown structures (100 levels)
pub const MAX_NESTING_DEPTH: usize = 100;

/// Maximum YAML nesting depth (100 levels)
/// Prevents stack overflow from deeply nested YAML structures
///
/// Re-exported from [`crate::document::limits::MAX_YAML_DEPTH`].
pub use crate::document::limits::MAX_YAML_DEPTH;

/// Maximum number of CARD blocks allowed per document
/// Prevents memory exhaustion from documents with excessive card blocks
pub const MAX_CARD_COUNT: usize = 1000;

/// Maximum number of fields allowed per document
/// Prevents memory exhaustion from documents with excessive fields
pub const MAX_FIELD_COUNT: usize = 1000;

/// Error severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Fatal error that prevents completion
    Error,
    /// Non-fatal issue that may need attention
    Warning,
    /// Informational message
    Note,
}

/// Location information for diagnostics
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Source file name (e.g., "plate.typ", "template.typ", "input.md")
    pub file: String,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
}

/// Structured diagnostic information.
///
/// `source_chain` is a flat list of error messages from any attached
/// `std::error::Error` cause chain, eagerly walked at construction time so
/// the diagnostic remains trivially `Clone` and fully serializable across
/// every binding boundary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// Error severity level
    pub severity: Severity,
    /// Optional error code (e.g., "E001", "typst::syntax")
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    /// Human-readable error message
    pub message: String,
    /// Primary source location (text anchor: file/line/column).
    ///
    /// Set by parsers and backend compilers. May co-exist with [`Self::path`]
    /// — the two anchors are independent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<Location>,
    /// Document-model anchor — a dotted/bracketed path into the typed
    /// [`crate::document::Document`].
    ///
    /// Set by schema validation and coercion. See the module-level docs for
    /// the path grammar and conventions. May co-exist with [`Self::location`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    /// Optional hint for fixing the error
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
    /// Flattened cause chain (outermost first).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_chain: Vec<String>,
}

impl Diagnostic {
    /// Create a new diagnostic
    pub fn new(severity: Severity, message: String) -> Self {
        Self {
            severity,
            code: None,
            message,
            location: None,
            path: None,
            hint: None,
            source_chain: Vec::new(),
        }
    }

    /// Set the error code
    pub fn with_code(mut self, code: String) -> Self {
        self.code = Some(code);
        self
    }

    /// Set the primary location
    pub fn with_location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    /// Set the document-model path anchor.
    ///
    /// See the module-level docs for the path grammar and conventions.
    pub fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }

    /// Set a hint
    pub fn with_hint(mut self, hint: String) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Attach an error cause chain, walked eagerly into `source_chain`.
    pub fn with_source(mut self, source: &(dyn std::error::Error + 'static)) -> Self {
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(source);
        while let Some(err) = current {
            self.source_chain.push(err.to_string());
            current = err.source();
        }
        self
    }

    /// Format diagnostic for pretty printing
    pub fn fmt_pretty(&self) -> String {
        let mut result = format!(
            "[{}] {}",
            match self.severity {
                Severity::Error => "ERROR",
                Severity::Warning => "WARN",
                Severity::Note => "NOTE",
            },
            self.message
        );

        if let Some(ref code) = self.code {
            result.push_str(&format!(" ({})", code));
        }

        if let Some(ref loc) = self.location {
            result.push_str(&format!("\n  --> {}:{}:{}", loc.file, loc.line, loc.column));
        }

        if let Some(ref path) = self.path {
            result.push_str(&format!("\n  at {}", path));
        }

        if let Some(ref hint) = self.hint {
            result.push_str(&format!("\n  hint: {}", hint));
        }

        result
    }

    /// Format diagnostic with source chain for debugging
    pub fn fmt_pretty_with_source(&self) -> String {
        let mut result = self.fmt_pretty();

        for (i, cause) in self.source_chain.iter().enumerate() {
            result.push_str(&format!("\n  cause {}: {}", i + 1, cause));
        }

        result
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Error type for parsing operations
#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    /// Input too large
    #[error("Input too large: {size} bytes (max: {max} bytes)")]
    InputTooLarge {
        /// Actual size
        size: usize,
        /// Maximum allowed size
        max: usize,
    },

    /// Invalid YAML structure
    #[error("Invalid YAML structure: {0}")]
    InvalidStructure(String),

    /// Markdown input was empty or whitespace-only.
    ///
    /// Emitted as code `parse::empty_input` so consumers can pattern-match
    /// without inspecting the message text.
    #[error("{0}")]
    EmptyInput(String),

    /// The document is missing its root `~~~card-yaml` block, or that block
    /// does not declare the required `$quill` system metadata.
    ///
    /// Emitted as code `parse::missing_quill` so consumers can
    /// pattern-match without inspecting the message text.
    #[error("{0}")]
    MissingQuill(String),

    /// YAML parsing error with location context
    #[error("YAML error at line {line}: {message}")]
    YamlErrorWithLocation {
        /// Error message
        message: String,
        /// Line number in the source document (1-indexed)
        line: usize,
        /// Index of the metadata block (0-indexed)
        block_index: usize,
    },
}

impl ParseError {
    /// Convert the parse error into a structured diagnostic
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            ParseError::InputTooLarge { size, max } => Diagnostic::new(
                Severity::Error,
                format!("Input too large: {} bytes (max: {} bytes)", size, max),
            )
            .with_code("parse::input_too_large".to_string()),
            ParseError::InvalidStructure(msg) => Diagnostic::new(Severity::Error, msg.clone())
                .with_code("parse::invalid_structure".to_string()),
            ParseError::EmptyInput(msg) => Diagnostic::new(Severity::Error, msg.clone())
                .with_code("parse::empty_input".to_string()),
            ParseError::MissingQuill(msg) => Diagnostic::new(Severity::Error, msg.clone())
                .with_code("parse::missing_quill".to_string()),
            ParseError::YamlErrorWithLocation {
                message,
                line,
                block_index,
            } => Diagnostic::new(
                Severity::Error,
                format!(
                    "YAML error at line {} (block {}): {}",
                    line, block_index, message
                ),
            )
            .with_code("parse::yaml_error_with_location".to_string()),
        }
    }
}

/// Main error type for rendering operations.
///
/// Every variant carries a non-empty `diags: Vec<Diagnostic>`. Variants whose
/// failure is inherently a single diagnostic still carry a one-element vector
/// — the uniform shape lets every consumer, and every language binding,
/// handle all rendering errors through a single code path. See
/// [`RenderError::diagnostics`] and [`RenderError::into_diagnostics`]. The
/// variant itself records the *kind* of failure (which bindings map to typed
/// exceptions); the payload is always just the diagnostics.
#[derive(Debug)]
pub enum RenderError {
    /// Failed to create rendering engine.
    EngineCreation {
        /// Diagnostics describing the failure. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// Invalid YAML in a card-yaml block.
    InvalidPayload {
        /// Diagnostics describing the failure. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// Backend compilation failed with one or more errors.
    CompilationFailed {
        /// All compilation diagnostics. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// Requested output format not supported by backend.
    FormatNotSupported {
        /// Diagnostics describing the failure. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// Backend not registered with engine.
    UnsupportedBackend {
        /// Diagnostics describing the failure. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// Validation failed for parsed document — may carry multiple diagnostics
    /// when several problems are detected during a single validation pass
    /// (e.g. multiple missing required fields). Each diagnostic should set
    /// `path` to anchor the error at a specific location in the document model.
    ValidationFailed {
        /// All validation diagnostics. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// Quill configuration error — may carry multiple diagnostics when several
    /// problems are detected during parsing (e.g. several unknown keys at once).
    QuillConfig {
        /// All configuration diagnostics. Always non-empty.
        diags: Vec<Diagnostic>,
    },
}

impl RenderError {
    /// All diagnostics carried by this error, in order. Always non-empty.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            RenderError::EngineCreation { diags }
            | RenderError::InvalidPayload { diags }
            | RenderError::CompilationFailed { diags }
            | RenderError::FormatNotSupported { diags }
            | RenderError::UnsupportedBackend { diags }
            | RenderError::ValidationFailed { diags }
            | RenderError::QuillConfig { diags } => diags,
        }
    }

    /// Consume the error and return its diagnostics. Always non-empty.
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        match self {
            RenderError::EngineCreation { diags }
            | RenderError::InvalidPayload { diags }
            | RenderError::CompilationFailed { diags }
            | RenderError::FormatNotSupported { diags }
            | RenderError::UnsupportedBackend { diags }
            | RenderError::ValidationFailed { diags }
            | RenderError::QuillConfig { diags } => diags,
        }
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::CompilationFailed { diags } => {
                write!(
                    f,
                    "Backend compilation failed with {} error(s)",
                    diags.len()
                )
            }
            RenderError::ValidationFailed { diags } => {
                write!(f, "Validation failed with {} error(s)", diags.len())
            }
            RenderError::QuillConfig { diags } => {
                write!(
                    f,
                    "Quill configuration failed with {} error(s)",
                    diags.len()
                )
            }
            // Single-diagnostic kinds display their primary diagnostic message.
            RenderError::EngineCreation { .. }
            | RenderError::InvalidPayload { .. }
            | RenderError::FormatNotSupported { .. }
            | RenderError::UnsupportedBackend { .. } => match self.diagnostics().first() {
                Some(d) => write!(f, "{}", d.message),
                None => write!(f, "render error"),
            },
        }
    }
}

impl std::error::Error for RenderError {}

/// Convert ParseError to RenderError
impl From<ParseError> for RenderError {
    fn from(err: ParseError) -> Self {
        RenderError::InvalidPayload {
            diags: vec![Diagnostic::new(Severity::Error, err.to_string())
                .with_code("parse::error".to_string())],
        }
    }
}

/// Result type containing artifacts and warnings
#[derive(Debug)]
pub struct RenderResult {
    /// Generated output artifacts
    pub artifacts: Vec<crate::Artifact>,
    /// Non-fatal diagnostic messages
    pub warnings: Vec<Diagnostic>,
    /// Output format that was produced
    pub output_format: OutputFormat,
}

impl RenderResult {
    /// Create a new result with artifacts and output format
    pub fn new(artifacts: Vec<crate::Artifact>, output_format: OutputFormat) -> Self {
        Self {
            artifacts,
            warnings: Vec::new(),
            output_format,
        }
    }

    /// Add a warning to the result
    pub fn with_warning(mut self, warning: Diagnostic) -> Self {
        self.warnings.push(warning);
        self
    }
}

/// Helper to print structured errors
pub fn print_errors(err: &RenderError) {
    for d in err.diagnostics() {
        eprintln!("{}", d.fmt_pretty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_with_source_chain() {
        let root_err = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let diag =
            Diagnostic::new(Severity::Error, "Rendering failed".to_string()).with_source(&root_err);

        assert_eq!(diag.source_chain.len(), 1);
        assert!(diag.source_chain[0].contains("File not found"));
    }

    #[test]
    fn test_diagnostic_serialization() {
        let diag = Diagnostic::new(Severity::Error, "Test error".to_string())
            .with_code("E001".to_string())
            .with_location(Location {
                file: "test.typ".to_string(),
                line: 10,
                column: 5,
            });

        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("Test error"));
        assert!(json.contains("E001"));
        assert!(json.contains("\"severity\":\"error\""));
        assert!(json.contains("\"column\":5"));
    }

    #[test]
    fn test_render_error_diagnostics_extraction() {
        let diag1 = Diagnostic::new(Severity::Error, "Error 1".to_string());
        let diag2 = Diagnostic::new(Severity::Error, "Error 2".to_string());

        let err = RenderError::CompilationFailed {
            diags: vec![diag1, diag2],
        };

        let diags = err.diagnostics();
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn test_render_error_uniform_single_diagnostic_shape() {
        // Single-diagnostic kinds carry a one-element vector and expose it
        // through the same accessors as the multi-diagnostic kinds.
        let err = RenderError::UnsupportedBackend {
            diags: vec![Diagnostic::new(
                Severity::Error,
                "no such backend".to_string(),
            )],
        };
        assert_eq!(err.diagnostics().len(), 1);
        assert_eq!(err.to_string(), "no such backend");

        let owned = err.into_diagnostics();
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].message, "no such backend");
    }

    #[test]
    fn test_render_error_display_aggregates_multi_diagnostic() {
        let err = RenderError::ValidationFailed {
            diags: vec![
                Diagnostic::new(Severity::Error, "a".to_string()),
                Diagnostic::new(Severity::Error, "b".to_string()),
            ],
        };
        assert_eq!(err.to_string(), "Validation failed with 2 error(s)");
    }

    #[test]
    fn test_diagnostic_fmt_pretty() {
        let diag = Diagnostic::new(Severity::Warning, "Deprecated field used".to_string())
            .with_code("W001".to_string())
            .with_location(Location {
                file: "input.md".to_string(),
                line: 5,
                column: 10,
            })
            .with_hint("Use the new field name instead".to_string());

        let output = diag.fmt_pretty();
        assert!(output.contains("[WARN]"));
        assert!(output.contains("Deprecated field used"));
        assert!(output.contains("W001"));
        assert!(output.contains("input.md:5:10"));
        assert!(output.contains("hint:"));
    }

    #[test]
    fn test_diagnostic_with_path() {
        let diag = Diagnostic::new(Severity::Error, "Missing field".to_string())
            .with_code("validation::missing_required".to_string())
            .with_path("cards.indorsement[0].signature_block".to_string());

        assert_eq!(
            diag.path.as_deref(),
            Some("cards.indorsement[0].signature_block")
        );

        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"path\":\"cards.indorsement[0].signature_block\""));

        let pretty = diag.fmt_pretty();
        assert!(pretty.contains("at cards.indorsement[0].signature_block"));
    }

    #[test]
    fn test_diagnostic_path_omitted_when_none() {
        let diag = Diagnostic::new(Severity::Error, "No path".to_string());
        let json = serde_json::to_string(&diag).unwrap();
        assert!(!json.contains("\"path\""));
    }

    #[test]
    fn test_diagnostic_fmt_pretty_with_source() {
        let root_err = std::io::Error::other("Underlying error");
        let diag = Diagnostic::new(Severity::Error, "Top-level error".to_string())
            .with_code("E002".to_string())
            .with_source(&root_err);

        let output = diag.fmt_pretty_with_source();
        assert!(output.contains("[ERROR]"));
        assert!(output.contains("Top-level error"));
        assert!(output.contains("cause 1:"));
        assert!(output.contains("Underlying error"));
    }

    #[test]
    fn test_render_result_with_warnings() {
        let artifacts = vec![];
        let warning = Diagnostic::new(Severity::Warning, "Test warning".to_string());

        let result = RenderResult::new(artifacts, OutputFormat::Pdf).with_warning(warning);

        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].message, "Test warning");
    }
}
