//! `RichText` — the canonical corpus content model for Quillmark (issue #831).
//!
//! One [`RichText`] per content field: a single text sequence carrying line
//! attributes, anchored marks, and embedded islands, over one coordinate space
//! of Unicode scalar values. Markdown is demoted to a *projection* — import
//! ([`import::from_markdown`]) and export ([`export::to_markdown`]) codecs — so
//! every edit is a splice and all structure moves with it.
//!
//! This crate is **phase 1**: the model, the canonical serialization freeze,
//! the markdown codecs, and the delta/rebase surface, exercised in isolation.
//! No engine crate consumes it yet (phase 2 wires the seam and storage). See
//! `prose/plans/richtext/` for the phase map.
//!
//! ## Layout
//!
//! - [`model`] — the [`RichText`] type, the mark set, normalization (the three
//!   Spike-A rules), and invariants. The freeze.
//! - [`serial`] — canonical, byte-deterministic JSON. One encoding for the seam
//!   and for storage.
//! - [`import`] — markdown → corpus (normalize → pulldown → corpus).
//! - [`export`] — corpus → markdown, per island loss class.
//! - [`delta`] — a per-field delta (Quill-Delta semantics), cold-parse + corpus
//!   diff, and mark/island rebase with a block-move detector.
//! - [`usv`] — coordinate conversions across the UTF-8 / UTF-16 / USV boundary.

pub mod delta;
pub mod export;
pub mod import;
pub mod model;
pub mod serial;
pub mod usv;

pub use model::{Container, Invariant, Island, Line, LineKind, Loss, Mark, MarkKind, RichText, Usv};
pub use serial::ParseError;
