//! # Orchestration
//!
//! Orchestrates the Quillmark engine. The portable [`Quill`](quillmark_core::Quill)
//! type lives in core; this module is the render dispatcher over it.
//!
//! ## Usage
//!
//! 1. Load a quill with [`Quill::from_tree`](quillmark_core::Quill::from_tree) or
//!    [`crate::quill_from_path`] (no engine required — a `Quill` is engine-free,
//!    validated data)
//! 2. Create an engine with [`Quillmark::new`]
//! 3. Render documents via [`Quillmark::render`] or [`Quillmark::open`]

mod engine;

pub use engine::Quillmark;
