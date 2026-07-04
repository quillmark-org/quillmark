//! Throwaway Phase-0 spikes for the richtext content model (#831).
//!
//! This crate is a **probe, not a product**. It exists to answer the three
//! questions a `RichText` schema freeze cannot cheaply revise — mark semantics,
//! source-map inversion, seam determinism — and to leave each answer as an
//! executable finding. Delete it when the phases it de-risks land. See
//! `prose/plans/richtext/phase-0-spikes.md` and the sibling `phase-0-finding-*`
//! docs.
//!
//! Layout:
//! - [`model`] — the `RichText` prototype (corpus + lines + marks + islands).
//! - [`canonical`] — byte-deterministic JSON + content hash (Spike C).
//! - [`codec`] — markdown ⇄ corpus + the pdfform `.text` lowering (Spikes A/C).
//! - [`sourcemap`] — per-run escape-transform inversion (Spike B).
//! - [`diff`] — cold-parse + corpus diff and position rebasing (Spike A).
//! - [`usv`] — the USV ↔ UTF-16/UTF-8 boundary conversions (cross-binding tax).
//!
//! The runnable evidence lives in `tests/spike_{a,b,c}_*.rs`; each assertion is
//! a claim in the corresponding finding.

pub mod canonical;
pub mod codec;
pub mod diff;
pub mod model;
pub mod sourcemap;
pub mod usv;

pub use model::{
    CharRange, Container, Island, Line, LineKind, Loss, Mark, MarkKind, RichText, Usv, ISLAND_SLOT,
};
