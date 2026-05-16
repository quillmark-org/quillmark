//! # Quillmark Core Overview
//!
//! Core types and functionality for the Quillmark format-first Markdown rendering system.
//!
//! ## Features
//!
//! This crate provides the foundational types and traits for Quillmark:
//!
//! - **Parsing**: YAML frontmatter extraction with Extended YAML Metadata Standard support
//! - **Format model**: [`QuillSource`] type for managing format bundles with in-memory file system
//! - **Backend trait**: Extensible interface for implementing output format backends
//! - **Error handling**: Structured diagnostics with source location tracking
//! - **Utilities**: TOML⇄YAML conversion helpers
//!
//! ## Quick Start
//!
//! ```no_run
//! use quillmark_core::Document;
//!
//! // Parse markdown with frontmatter
//! let markdown = "---\nQUILL: my_quill\ntitle: Example\n---\n\n# Content";
//! let doc = Document::from_markdown(markdown).unwrap();
//! let title = doc.main()
//!     .frontmatter()
//!     .get("title")
//!     .and_then(|v| v.as_str())
//!     .unwrap_or("Untitled");
//! assert_eq!(title, "Example");
//! ```
//!
//! ## Architecture
//!
//! The crate is organized into modules:
//!
//! - [`document`]: Markdown parsing with YAML frontmatter support
//! - [`backend`]: Backend trait for output format implementations
//! - [`error`]: Structured error handling and diagnostics
//! - [`types`]: Core rendering types (OutputFormat, Artifact, RenderOptions)
//! - [`quill`]: QuillSource bundle and related types
//!
//! ## Further Reading
//!
//! - [PARSE.md](https://github.com/nibsbin/quillmark/blob/main/designs/PARSE.md) - Detailed parsing documentation
//! - [Examples](https://github.com/nibsbin/quillmark/tree/main/examples) - Working examples

pub mod document;
pub use document::{
    Card, Document, EditError, Frontmatter, FrontmatterItem, ParseOutput, Sentinel,
};

pub mod backend;
pub use backend::Backend;

pub mod error;
pub use error::{Diagnostic, Location, ParseError, RenderError, RenderResult, Severity};

pub mod types;
pub use types::{Artifact, OutputFormat, RenderOptions};

pub mod session;
pub use session::RenderSession;

pub mod quill;
pub use quill::{FileTreeNode, QuillIgnore, QuillSource};

pub mod value;
pub use value::QuillValue;

pub mod normalize;

pub mod version;
pub use version::{QuillReference, Version, VersionSelector};
