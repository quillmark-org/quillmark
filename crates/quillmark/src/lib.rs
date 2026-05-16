//! # Quillmark
//!
//! Quillmark is a flexible, format-first Markdown rendering system that converts Markdown
//! with YAML frontmatter into various output artifacts (PDF, SVG, TXT, etc.).
//!
//! ## Quick Start
//!
//! ```no_run
//! use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
//!
//! let engine = Quillmark::new();
//! let quill = engine.quill_from_path("path/to/quill").unwrap();
//!
//! let parsed = Document::from_markdown("---\nQUILL: my_quill\ntitle: Hello\n---\n# Hello World").unwrap();
//! let result = quill.render(&parsed, &RenderOptions {
//!     output_format: Some(OutputFormat::Pdf),
//!     ..Default::default()
//! }).unwrap();
//! ```

// Re-export core types for convenience. Note: `QuillSource` is not re-exported
// at the crate root — Quillmark consumers work with the renderable `Quill`.
pub use quillmark_core::{
    Artifact, Backend, Card, Diagnostic, Document, Location, OutputFormat, ParseError, ParseOutput,
    RenderError, RenderOptions, RenderResult, RenderSession, Severity,
};

// Declare modules
pub mod form;
pub mod orchestration;

// Re-export commonly-used form types at the crate root
pub use form::{Form, FormCard, FormFieldSource, FormFieldValue};

// Re-export types from orchestration module
pub use orchestration::{Quill, Quillmark};
