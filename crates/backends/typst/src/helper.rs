//! # Quillmark Helper Package Generator
//!
//! This module generates the virtual `@local/quillmark-helper:0.1.0` package
//! that provides document data and helper functions to Typst plates.
//!
//! ## Package Contents
//!
//! The generated package exports:
//! - `data` - A dictionary containing all document fields, with markdown fields
//!   and date fields automatically converted to Typst values
//!
//! ## Usage in Plates
//!
//! ```typst
//! #import "@local/quillmark-helper:0.1.0": data
//!
//! #data.title
//! #data.BODY
//! #data.date
//! ```

use crate::convert::escape_string;

/// Helper function to inject JSON into Typst code.
/// Exposed for fuzzing tests.
#[doc(hidden)]
pub fn inject_json(bytes: &str) -> String {
    format!("json(bytes(\"{}\"))", escape_string(bytes))
}

/// Helper package version
pub const HELPER_VERSION: &str = "0.1.0";

/// Helper package namespace
pub const HELPER_NAMESPACE: &str = "local";

/// Helper package name
pub const HELPER_NAME: &str = "quillmark-helper";

/// Template for the `lib.typ` file, loaded at compile time
const LIB_TYP_TEMPLATE: &str = include_str!("lib.typ.template");

/// Generate the `lib.typ` content for the quillmark-helper package.
///
/// The generated file contains:
/// - Embedded JSON data (including `__meta__` injected by `transform_fields`)
///   with markdown and date fields auto-normalized
pub fn generate_lib_typ(json_data: &str) -> String {
    let escaped_json = escape_string(json_data);

    LIB_TYP_TEMPLATE
        .replace("{version}", HELPER_VERSION)
        .replace("{escaped_json}", &escaped_json)
}

/// Generate the `typst.toml` content for the quillmark-helper package.
pub fn generate_typst_toml() -> String {
    format!(
        r#"[package]
name = "{name}"
version = "{version}"
namespace = "{namespace}"
entrypoint = "lib.typ"
"#,
        name = HELPER_NAME,
        version = HELPER_VERSION,
        namespace = HELPER_NAMESPACE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_lib_typ_basic() {
        let json = r#"{"title":"Test","BODY":"Hello","date":"2025-01-15","__meta__":{"content_fields":["BODY"],"card_content_fields":{},"date_fields":["date"],"card_date_fields":{}}}"#;
        let lib = generate_lib_typ(json);

        // Should contain the version comment
        assert!(lib.contains("Version: 0.1.0"));

        // Should contain the raw JSON data
        assert!(lib.contains("json(bytes("));

        // Should NOT contain eval-markup (auto-eval replaces it)
        assert!(!lib.contains("eval-markup"));

        // Should contain private date parser and conversion metadata handling
        assert!(lib.contains("#let _parse-date(s)"));
        assert!(!lib.contains("#let parse-date(s)"));
        assert!(lib.contains("meta.date_fields"));
        assert!(lib.contains("meta.card_date_fields"));
    }

    #[test]
    fn test_generate_lib_typ_escapes_json() {
        // JSON with special characters that need escaping
        let json = r#"{"title": "Test \"quoted\""}"#;
        let lib = generate_lib_typ(json);

        // The quotes in JSON should be escaped for Typst string literal
        assert!(lib.contains("\\\""));
    }

    #[test]
    fn test_generate_lib_typ_handles_newlines() {
        let json = "{\n\"title\": \"Test\"\n}";
        let lib = generate_lib_typ(json);

        // Newlines should be escaped
        assert!(lib.contains("\\n"));
    }

    #[test]
    fn test_generate_typst_toml() {
        let toml = generate_typst_toml();

        assert!(toml.contains("name = \"quillmark-helper\""));
        assert!(toml.contains("version = \"0.1.0\""));
        assert!(toml.contains("namespace = \"local\""));
        assert!(toml.contains("entrypoint = \"lib.typ\""));
    }

    #[test]
    fn test_helper_constants() {
        assert_eq!(HELPER_VERSION, "0.1.0");
        assert_eq!(HELPER_NAMESPACE, "local");
        assert_eq!(HELPER_NAME, "quillmark-helper");
    }
}
