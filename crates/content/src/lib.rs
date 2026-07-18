//! `Content` — the canonical content content model for Quillmark (issue #831).
//!
//! One [`Content`] per content field: a single text sequence carrying line
//! attributes, anchored marks, and embedded islands, over one coordinate space
//! of Unicode scalar values. Markdown is demoted to a *projection* — import
//! ([`import::from_markdown`]) and export ([`export::to_markdown`]) codecs — so
//! every edit is a splice and all structure moves with it.
//!
//! `core`, `quillmark`, and both backends (`typst`, `pdfform`) consume this
//! crate: the seam carries content JSON, storage embeds it structurally (see
//! `prose/canon/DOCUMENT_STORAGE.md`), and the content edit surface
//! (`delta`, `ops`) drives per-field splices. See
//! `prose/plans/richtext/` for the phase map that landed it.
//!
//! ## Layout
//!
//! - [`model`] — the [`Content`] type, the mark set, normalization (the three
//!   Spike-A rules), and invariants. The freeze.
//! - [`serial`] — canonical, byte-deterministic JSON. One encoding for the seam
//!   and for storage.
//! - [`import`] — markdown → content (normalize → pulldown → content).
//! - [`export`] — content → markdown, per island loss class.
//! - [`delta`] — the per-field edit surface: a text-splice change set
//!   (`retain`/`insert`/`delete`, CodeMirror `ChangeSet` semantics) plus the
//!   cold-parse + content-diff stale-text writer with a block-move detector. The
//!   text-splice channel is the positional core; mark and line-attribute edits
//!   are separate op channels, not op attributes — see [`delta`].
//! - [`ops`] — mark and line op channels:
//!   [`Content::apply_text_delta`], [`apply_mark_ops`](Content::apply_mark_ops),
//!   [`apply_line_ops`](Content::apply_line_ops).
//! - [`normalize`] — the markdown-string input primitive (line endings, bidi
//!   strip, HTML-comment fence repair), applied at the import boundary.
//! - [`usv`] — USV → UTF-8 byte-offset conversion for slicing the content.

pub mod delta;
pub mod export;
pub mod import;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod serial;
pub mod usv;

pub use delta::{diff_import, Assoc, Delta, Op};
pub use export::{to_markdown, to_plaintext};
pub use import::{from_markdown, from_plaintext};
pub use model::{
    Container, Invariant, Island, Line, LineKind, Loss, Mark, MarkKind, Content, Usv,
};
pub use normalize::normalize_markdown;
pub use ops::{
    change_bundle_from_value, line_op_from_value, line_op_to_value, mark_op_from_value,
    mark_op_to_value, ApplyError, LineOp, MarkOp,
};
pub use serial::ParseError;

/// Maximum container nesting depth the markdown codecs accept before erroring.
/// The import guard ([`import::from_markdown`]) and the typst backend's markup
/// converter share this one limit (the backend re-exports it via
/// `quillmark_core::error::MAX_NESTING_DEPTH`), so a document that imports also
/// renders.
pub const MAX_NESTING_DEPTH: usize = 100;
