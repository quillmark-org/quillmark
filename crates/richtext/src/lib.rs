//! `RichText` — the canonical corpus content model for Quillmark (issue #831).
//!
//! One [`RichText`] per content field: a single text sequence carrying line
//! attributes, anchored marks, and embedded islands, over one coordinate space
//! of Unicode scalar values. Markdown is demoted to a *projection* — import
//! ([`import::from_markdown`]) and export ([`export::to_markdown`]) codecs — so
//! every edit is a splice and all structure moves with it.
//!
//! `core`, `quillmark`, and both backends (`typst`, `pdfform`) consume this
//! crate: the seam carries corpus JSON, storage embeds it structurally (see
//! `prose/canon/DOCUMENT_STORAGE.md`), and the live-preview edit surface
//! (`change_log`, `ops`) drives it through `LiveSession`. See
//! `prose/plans/richtext/` for the phase map that landed it.
//!
//! ## Layout
//!
//! - [`model`] — the [`RichText`] type, the mark set, normalization (the three
//!   Spike-A rules), and invariants. The freeze.
//! - [`serial`] — canonical, byte-deterministic JSON. One encoding for the seam
//!   and for storage.
//! - [`import`] — markdown → corpus (normalize → pulldown → corpus).
//! - [`export`] — corpus → markdown, per island loss class.
//! - [`change_log`] — monotonic revision + bounded per-field delta ring
//!   (phase 3 PR-C); composed [`change_log::ChangeLog::map_pos`].
//! - [`delta`] — the per-field edit surface: a text-splice change set
//!   (`retain`/`insert`/`delete`, CodeMirror `ChangeSet` semantics) plus the
//!   cold-parse + corpus-diff stale-text writer with a block-move detector. The
//!   text-splice channel is the positional core; mark and line-attribute edits
//!   are separate op channels (phase 3), not op attributes — see [`delta`].
//! - [`ops`] — mark and line op channels (phase 3 PR-D):
//!   [`RichText::apply_text_delta`], [`apply_mark_ops`](RichText::apply_mark_ops),
//!   [`apply_line_ops`](RichText::apply_line_ops).
//! - [`normalize`] — the markdown-string input primitive (line endings, bidi
//!   strip, HTML-comment fence repair), applied at the import boundary.
//! - [`usv`] — coordinate conversions across the UTF-8 / UTF-16 / USV boundary.

pub mod change_log;
pub mod delta;
pub mod export;
pub mod import;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod serial;
pub mod usv;

pub use change_log::{
    ChangeLog, FieldChange, StaleRevision, DEFAULT_CAPACITY as CHANGE_LOG_DEFAULT_CAPACITY,
};
pub use delta::{Assoc, Delta, Op};
pub use model::{
    Container, Invariant, Island, Line, LineKind, Loss, Mark, MarkKind, RichText, Usv,
};
pub use normalize::normalize_markdown;
pub use ops::{ApplyError, LineOp, MarkOp};
pub use serial::ParseError;

/// Maximum container nesting depth the markdown codecs accept before erroring.
/// The import guard ([`import::from_markdown`]) and the typst backend's markup
/// converter share this one limit (the backend re-exports it via
/// `quillmark_core::error::MAX_NESTING_DEPTH`), so a document that imports also
/// renders.
pub const MAX_NESTING_DEPTH: usize = 100;
