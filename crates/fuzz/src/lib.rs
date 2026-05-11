//! # Quillmark Fuzzing Tests
//!
//! This crate contains comprehensive property-based fuzzing tests for Quillmark
//! using the `proptest` framework. These tests validate the security of:
//!
//! - Escaping functions (`escape_string`, `escape_markup`)
//! - Markdown parser with malicious inputs
//! - Filter inputs for injection vulnerabilities
//!
//! ## Test Organization
//!
//! - `convert_fuzz` - Tests for markdown to Typst conversion and escaping
//! - `filter_fuzz` - Tests for filter input validation and injection safety
//! - `parse_fuzz` - Tests for YAML frontmatter and markdown parsing

#[cfg(test)]
mod coerce_fuzz;

#[cfg(test)]
mod convert_fuzz;

#[cfg(test)]
mod emit_roundtrip_fuzz;

#[cfg(test)]
mod filter_fuzz;

#[cfg(test)]
mod parse_fuzz;
