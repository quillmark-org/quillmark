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
//! - `coerce_fuzz` - Tests for schema-driven value coercion
//! - `convert_fuzz` - Tests for markdown to Typst conversion and escaping
//! - `emit_roundtrip_fuzz` - Tests that emitted markdown re-parses losslessly
//! - `filter_fuzz` - Tests for filter input validation and injection safety
//! - `parse_fuzz` - Tests for YAML payload and markdown parsing

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
