//! Property-based fuzz tests for Quillmark using `proptest`.
//!
//! Covers escaping correctness (`escape_string`, `escape_markup`),
//! parse/emit round-trip stability, and schema coercion invariants.

#[cfg(test)]
mod coerce_fuzz;

#[cfg(test)]
mod convert_fuzz;

#[cfg(test)]
mod emit_roundtrip_fuzz;

#[cfg(test)]
mod parse_fuzz;
