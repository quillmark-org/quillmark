//! # Error Handling
//!
//! Error types and diagnostics for parsing and rendering, with source location tracking.
//!
//! ## Document path anchors
//!
//! A [`Diagnostic`] carries two independent "where" anchors, both optional:
//!
//! - [`Diagnostic::location`] — source-text anchor (`file:line:column`).
//!   Produced by parsers and backend compilers operating on raw text.
//! - [`Diagnostic::path`] — document-model anchor into the typed
//!   [`crate::document::Document`]. Produced by schema validation and
//!   coercion, which run on the typed model after line spans are gone.
//!
//! ### Path grammar
//!
//! ```text
//! path        := segment ( "." field_name | "[" index "]" )*
//! field_name  := [A-Za-z_][A-Za-z0-9_]*  // card kinds use lowercase-only [a-z_][a-z0-9_]*
//! index       := [0-9]+
//! ```
//!
//! Because field names and card kinds are validated to charsets that exclude
//! `.`, `[`, `]`, and whitespace, the dotted form round-trips unambiguously.
//!
//! | Anchor                     | Path                                      |
//! |----------------------------|-------------------------------------------|
//! | Root-block field           | `title`                                   |
//! | Nested in array of objects | `recipients[0].name`                      |
//! | Main card body             | `main.body`                               |
//! | Typed card (whole)         | `cards.indorsement[0]`                    |
//! | Field on typed card        | `cards.indorsement[0].signature_block`    |
//! | Body on typed card         | `cards.indorsement[0].body`               |
//! | Card with unknown kind     | `cards[0]`                                |
//!
//! The `cards.<kind>[<index>]` form fuses card kind and document array index so
//! consumers receive both without a second lookup.

use crate::OutputFormat;

/// Maximum input size for markdown (10 MB)
pub const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024;

/// Maximum YAML size (1 MB)
pub const MAX_YAML_SIZE: usize = 1024 * 1024;

/// Maximum nesting depth for markdown structures (100 levels)
pub const MAX_NESTING_DEPTH: usize = 100;

/// Re-exported from [`crate::document::limits::MAX_YAML_DEPTH`].
pub use crate::document::limits::MAX_YAML_DEPTH;

/// Maximum number of card blocks allowed per document
pub const MAX_CARD_COUNT: usize = 1000;

/// Maximum number of fields allowed per document
pub const MAX_FIELD_COUNT: usize = 1000;

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
    pub severity: Severity,
    /// Optional error code (e.g., "E001", "typst::syntax")
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
    /// Flattened cause chain (outermost first).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_chain: Vec<String>,
}

impl Diagnostic {
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

    pub fn with_code(mut self, code: String) -> Self {
        self.code = Some(code);
        self
    }

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

    /// Format diagnostic with source chain for debugging.
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

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("Input too large: {size} bytes (max: {max} bytes)")]
    InputTooLarge { size: usize, max: usize },

    #[error("Invalid YAML structure: {0}")]
    InvalidStructure(String),

    /// Markdown input was empty or whitespace-only.
    ///
    /// Emitted as code `parse::empty_input` so consumers can pattern-match
    /// without inspecting the message text.
    #[error("{0}")]
    EmptyInput(String),

    /// The document is missing its root `~~~` card-yaml block, or that block
    /// does not declare the required `$quill` system metadata.
    ///
    /// Emitted as code `parse::missing_quill` so consumers can
    /// pattern-match without inspecting the message text.
    #[error("{0}")]
    MissingQuill(String),

    /// A `$quill` reference failed to parse as a [`crate::version::QuillReference`].
    /// Code `parse::invalid_quill_reference`; carries
    /// [`crate::version::quill_ref_hint`] as its diagnostic hint.
    #[error("Invalid $quill reference '{value}': {reason}")]
    InvalidQuillReference {
        value: String,
        /// The `from_str` violation.
        reason: String,
    },

    #[error("YAML error at line {line}: {message}")]
    YamlErrorWithLocation {
        message: String,
        /// Line number in the source document (1-indexed)
        line: usize,
        /// Index of the metadata block (0-indexed)
        block_index: usize,
        /// Optional actionable hint attached when the YAML parser's message
        /// is too cryptic to be recoverable on its own. Derived by the
        /// internal `document::yaml_hints` enrichment pass.
        hint: Option<String>,
    },
}

impl ParseError {
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
            ParseError::InvalidQuillReference { value, reason } => Diagnostic::new(
                Severity::Error,
                format!("Invalid $quill reference '{}': {}", value, reason),
            )
            .with_code("parse::invalid_quill_reference".to_string())
            .with_hint(crate::version::quill_ref_hint().to_string()),
            ParseError::YamlErrorWithLocation {
                message,
                line,
                block_index,
                hint,
            } => {
                let mut d = Diagnostic::new(
                    Severity::Error,
                    format!(
                        "YAML error at line {} (block {}): {}",
                        line, block_index, message
                    ),
                )
                .with_code("parse::yaml_error_with_location".to_string());
                if let Some(h) = hint {
                    d = d.with_hint(h.clone());
                }
                d
            }
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
    /// Failed to initialize the backend's rendering engine.
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

    /// The document was rendered with a quill that does not satisfy its `$quill`
    /// reference — a different *name* (`quill::name_mismatch`) or a `version`
    /// outside the selector (`quill::version_mismatch`). Distinct from
    /// [`ValidationFailed`](RenderError::ValidationFailed): the document is
    /// well-formed; it was paired with the wrong quill.
    QuillMismatch {
        /// The mismatch diagnostic. Always non-empty.
        diags: Vec<Diagnostic>,
    },

    /// The backend's session does not support incremental
    /// [`apply`](crate::LiveSession::apply). Both built-in backends support
    /// it; this is the default for a backend that does not override the seam.
    ApplyUnsupported {
        /// Diagnostics describing the failure. Always non-empty.
        diags: Vec<Diagnostic>,
    },
}

impl RenderError {
    /// Returns all diagnostics for this error. Always non-empty.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            RenderError::EngineCreation { diags }
            | RenderError::InvalidPayload { diags }
            | RenderError::CompilationFailed { diags }
            | RenderError::FormatNotSupported { diags }
            | RenderError::UnsupportedBackend { diags }
            | RenderError::ValidationFailed { diags }
            | RenderError::QuillConfig { diags }
            | RenderError::QuillMismatch { diags }
            | RenderError::ApplyUnsupported { diags } => diags,
        }
    }

    /// Consume the error and return its diagnostics.
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        match self {
            RenderError::EngineCreation { diags }
            | RenderError::InvalidPayload { diags }
            | RenderError::CompilationFailed { diags }
            | RenderError::FormatNotSupported { diags }
            | RenderError::UnsupportedBackend { diags }
            | RenderError::ValidationFailed { diags }
            | RenderError::QuillConfig { diags }
            | RenderError::QuillMismatch { diags }
            | RenderError::ApplyUnsupported { diags } => diags,
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
            RenderError::EngineCreation { .. }
            | RenderError::InvalidPayload { .. }
            | RenderError::FormatNotSupported { .. }
            | RenderError::UnsupportedBackend { .. }
            | RenderError::QuillMismatch { .. }
            | RenderError::ApplyUnsupported { .. } => match self.diagnostics().first() {
                Some(d) => write!(f, "{}", d.message),
                None => write!(f, "render error"),
            },
        }
    }
}

impl std::error::Error for RenderError {}

impl From<ParseError> for RenderError {
    fn from(err: ParseError) -> Self {
        RenderError::InvalidPayload {
            diags: vec![err.to_diagnostic()],
        }
    }
}

#[derive(Debug)]
pub struct RenderResult {
    pub artifacts: Vec<crate::Artifact>,
    pub warnings: Vec<Diagnostic>,
    pub output_format: OutputFormat,
    /// Schema-field geometry sidecar, populated only when
    /// [`RenderOptions::regions`](crate::RenderOptions) is set (empty
    /// otherwise). The same entries [`LiveSession::regions`](crate::LiveSession::regions)
    /// serves, for consumers without a live session. Whole-document geometry:
    /// page indices are document-space even under a `pages` subset render.
    pub regions: Vec<crate::RenderedRegion>,
}

impl RenderResult {
    pub fn new(artifacts: Vec<crate::Artifact>, output_format: OutputFormat) -> Self {
        Self {
            artifacts,
            warnings: Vec::new(),
            output_format,
            regions: Vec::new(),
        }
    }

    pub fn with_warning(mut self, warning: Diagnostic) -> Self {
        self.warnings.push(warning);
        self
    }
}

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
        let diag = Diagnostic::new(Severity::Error, "Type mismatch".to_string())
            .with_code("validation::type_mismatch".to_string())
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
