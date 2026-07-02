//! # Orchestration
//!
//! Orchestrates the Quillmark engine. The portable [`Quill`](quillmark_core::Quill)
//! type lives in core; this module is the render dispatcher over it. Load a
//! quill with [`Quill::from_tree`](quillmark_core::Quill::from_tree) or
//! [`crate::quill_from_path`] (no engine required — a `Quill` is portable,
//! declarative data), create an engine with [`Quillmark::new`], then render
//! documents via [`Quillmark::render`] or [`Quillmark::open`].

mod engine;

pub use engine::Quillmark;
