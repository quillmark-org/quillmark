//! # Quillmark Core
//!
//! Foundational types and traits for the Quillmark schema-driven document
//! engine: card-yaml block parsing (`~~~` metadata blocks), the [`Quill`]
//! format bundle and its in-memory file tree, the [`Backend`] trait for output
//! backends, and structured diagnostics with source-location tracking.
//!
//! ```no_run
//! use quillmark_core::Document;
//!
//! // Parse markdown with a card-yaml metadata block
//! let markdown = "~~~\n$quill: my_quill\n$kind: main\ntitle: Example\n~~~\n\n# Content";
//! let doc = Document::parse(markdown).unwrap().document;
//! let title = doc.main()
//!     .payload()
//!     .get("title")
//!     .and_then(|v| v.as_str())
//!     .unwrap_or("Untitled");
//! assert_eq!(title, "Example");
//! ```
//!
//! ## Further Reading
//!
//! - [markdown-spec.md](https://github.com/borb-sh/quillmark/blob/main/prose/references/markdown-spec.md) - Quillmark Markdown parsing specification
//! - [Examples](https://github.com/borb-sh/quillmark/tree/main/crates/core/examples) - Working examples

pub mod document;
pub use document::{
    Card, CardWire, Document, EditError, Parsed, Payload, PayloadItem, PayloadItemWire,
    RichtextDecodeError, SeedOverlay, WireError,
};

pub mod writer;
pub use writer::{CardWriter, TypedWriter};

pub mod backend;
pub use backend::{formats_support_canvas, Backend};

pub mod error;
pub use error::{Diagnostic, Location, ParseError, RenderError, RenderResult, Severity};

pub mod types;
pub use types::{Artifact, OutputFormat, RenderOptions};

pub mod region;
pub use region::{field_boxes, CorpusHit, HitGranularity, RenderedRegion};

pub mod session;
pub use session::{ApplyError, Assoc, ChangeSet, Delta, LineOp, LiveSession, MarkOp, Op};

/// The canonical content content model — re-exported so consumers of the
/// document mutators ([`Card::install_body`], [`Card::apply_body_change`])
/// can name the type without depending on `quillmark-content` directly.
pub use quillmark_content::Content;

pub mod quill;
pub use quill::{zero_value, FileTreeNode, Quill, QuillIgnore, STANDARD_METADATA_KEYS};

pub mod value;
pub use value::{json_depth_exceeds, PathSegment, QuillValue};

pub mod normalize;

pub mod version;
pub use version::{quill_ref_hint, QuillReference, Version, VersionSelector};
