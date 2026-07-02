//! Generates the virtual `@local/quillmark-helper:0.1.0` package that
//! provides document data and helper functions to Typst plates.
//! The package exports `data` — a dictionary of document fields with markdown
//! and date fields auto-converted to Typst values.

use crate::convert::escape_string;

/// Exposed for fuzzing tests.
#[doc(hidden)]
pub fn inject_json(bytes: &str) -> String {
    format!("json(bytes(\"{}\"))", escape_string(bytes))
}

pub const HELPER_VERSION: &str = "0.1.0";
pub const HELPER_NAMESPACE: &str = "local";
pub const HELPER_NAME: &str = "quillmark-helper";

const LIB_TYP_TEMPLATE: &str = include_str!("lib.typ.template");

/// Generate `lib.typ` for the quillmark-helper package from JSON data.
/// Embeds JSON (including `__meta__` injected by `transform_markdown_fields`) so
/// markdown and date fields are auto-normalized by the Typst template.
pub fn generate_lib_typ(json_data: &str) -> String {
    let escaped_json = escape_string(json_data);

    LIB_TYP_TEMPLATE
        .replace("{version}", HELPER_VERSION)
        .replace("{escaped_json}", &escaped_json)
}

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
        let json = r#"{"title":"Test","$body":"Hello","date":"2025-01-15","__meta__":{"content_fields":["$body"],"card_content_fields":{},"date_fields":["date"],"card_date_fields":{}}}"#;
        let lib = generate_lib_typ(json);

        assert!(lib.contains("Version: 0.1.0"));
        assert!(lib.contains("json(bytes("));
        // The template must expose only the private `_parse-date` helper —
        // no public `parse-date` and no `eval-markup` symbol.
        assert!(!lib.contains("eval-markup"));
        assert!(lib.contains("#let _parse-date(s)"));
        assert!(!lib.contains("#let parse-date(s)"));
        assert!(lib.contains("meta.date_fields"));
        assert!(lib.contains("meta.card_date_fields"));
    }

    #[test]
    fn test_generate_lib_typ_escapes_json() {
        let json = r#"{"title": "Test \"quoted\""}"#;
        let lib = generate_lib_typ(json);

        assert!(lib.contains("\\\""));
    }

    #[test]
    fn test_generate_lib_typ_handles_newlines() {
        let json = "{\n\"title\": \"Test\"\n}";
        let lib = generate_lib_typ(json);

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
}
