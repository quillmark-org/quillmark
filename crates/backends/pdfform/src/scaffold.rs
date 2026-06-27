//! Scaffold a starter `Quill.yaml` from a `form.json` [`FormSpec`].
//!
//! [`scaffold_quill_yaml`] converts the field-definition layer of a `pdfform`
//! quill into a parse-valid `Quill.yaml` string. The generated file is meant
//! as a *starting point* that the author then enriches with `example:` blocks
//! and richer descriptions — not a final product.

use crate::form::{FieldKind, FormSpec};

/// Generate a starter `Quill.yaml` string from a parsed [`FormSpec`].
///
/// Fields are emitted in document order (the order they appear in `form.json`).
/// Unbound fields (`schema_field: None`) — typically `Signature` slots filled
/// by the signer, not the data layer — are silently skipped.
///
/// The returned string is a complete, parse-valid `Quill.yaml` ready to be
/// written to disk and loaded by [`quillmark_core::QuillConfig::from_yaml`].
pub fn scaffold_quill_yaml(spec: &FormSpec, quill_name: &str) -> String {
    let mut out = String::new();

    // ── quill: header block ────────────────────────────────────────────────
    out.push_str("quill:\n");
    out.push_str(&format!("  name: {}\n", quill_name));
    out.push_str("  version: \"0.1.0\"\n");
    out.push_str("  backend: pdfform\n");
    out.push_str("  description: \"<TODO placeholder>\"\n");
    out.push('\n');

    // ── main: block ────────────────────────────────────────────────────────
    out.push_str("main:\n");
    out.push_str("  body:\n");
    out.push_str("    enabled: false\n");
    out.push_str("  fields:\n");

    for field in &spec.fields {
        // Skip unbound fields (no schema_field).
        let schema_key = match &field.schema_field {
            Some(k) => k,
            None => continue,
        };

        out.push_str(&format!("    {}:\n", schema_key));

        match &field.kind {
            FieldKind::Text { multiline: false } => {
                out.push_str("      type: string\n");
                out.push_str("      default: \"\"\n");
            }
            FieldKind::Text { multiline: true } => {
                out.push_str("      type: array\n");
                out.push_str("      items:\n");
                out.push_str("        type: string\n");
                out.push_str("      default: []\n");
                out.push_str("      ui:\n");
                out.push_str("        multiline: true\n");
            }
            FieldKind::Checkbox => {
                out.push_str("      type: boolean\n");
                out.push_str("      default: false\n");
            }
            FieldKind::Choice { options } => {
                out.push_str("      type: string\n");
                out.push_str("      enum:\n");
                for opt in options {
                    out.push_str(&format!("        - {}\n", yaml_quote_if_needed(opt)));
                }
                let default = options.first().map(|s| s.as_str()).unwrap_or("");
                if default.is_empty() {
                    out.push_str("      default: \"\"\n");
                } else {
                    out.push_str(&format!(
                        "      default: {}\n",
                        yaml_quote_if_needed(default)
                    ));
                }
            }
            // Signature fields are never emitted (always unbound), but the
            // match arm keeps the compiler happy for future FieldKind variants.
            FieldKind::Signature => continue,
        }

        // Emit description from tooltip when present.
        if let Some(tooltip) = &field.tooltip {
            out.push_str(&format!(
                "      description: {}\n",
                yaml_quote_scalar(tooltip)
            ));
        }
    }

    out
}

/// Quote a YAML scalar string defensively when it contains characters that
/// YAML parsers treat as special (`:`  `#`  `[`  `]`  `{`  `}`  `&`  `*`
/// `!`  `|`  `>`  `'`  `"`  `%`  `@`  `` ` ``), when it starts with a digit
/// or sign, or when it could be parsed as a boolean/null.
///
/// Returns the bare string when it is already safe, otherwise wraps it in
/// double-quotes with internal double-quotes escaped as `\"`.
fn yaml_quote_if_needed(s: &str) -> String {
    if needs_quoting(s) {
        yaml_quote_scalar(s)
    } else {
        s.to_string()
    }
}

/// Always wrap `s` in double-quotes, escaping `"` and `\` inside.
fn yaml_quote_scalar(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

/// Return `true` when the bare scalar `s` is unsafe to emit unquoted in YAML.
fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Reserved bare words
    matches!(
        s,
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
    ) || s.contains(':')
        || s.contains('#')
        || s.contains('[')
        || s.contains(']')
        || s.contains('{')
        || s.contains('}')
        || s.contains('&')
        || s.contains('*')
        || s.contains('!')
        || s.contains('|')
        || s.contains('>')
        || s.contains('\'')
        || s.contains('"')
        || s.contains('%')
        || s.contains('@')
        || s.contains('`')
        || s.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::FormSpec;

    /// Path to the gov_form fixture's form.json — resolved at compile time via
    /// `CARGO_MANIFEST_DIR` so it works in both local and CI environments.
    const GOV_FORM_JSON: &str =
        include_str!("../../../fixtures/resources/quills/gov_form/0.1.0/form.json");

    const GOV_FORM_PDF: &[u8] =
        include_bytes!("../../../fixtures/resources/quills/gov_form/0.1.0/form.pdf");

    fn parse_gov_form() -> FormSpec {
        FormSpec::parse(GOV_FORM_JSON.as_bytes()).expect("gov_form form.json must parse")
    }

    #[test]
    fn scaffold_produces_all_four_bound_fields() {
        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        // All four bound fields must appear.
        assert!(yaml.contains("    full_name:"), "missing full_name field");
        assert!(yaml.contains("    comments:"), "missing comments field");
        assert!(yaml.contains("    agree:"), "missing agree field");
        assert!(
            yaml.contains("    favorite_color:"),
            "missing favorite_color field"
        );

        // The unbound Signature must NOT appear.
        assert!(!yaml.contains("signature"), "Signature must be omitted");
    }

    #[test]
    fn scaffold_full_name_is_string_with_tooltip() {
        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        // full_name → type: string, default: "", description from tooltip.
        let fn_idx = yaml.find("    full_name:").expect("full_name block");
        let fn_block = &yaml[fn_idx..];
        assert!(
            fn_block.starts_with("    full_name:\n      type: string\n"),
            "full_name must be type: string"
        );
        assert!(
            fn_block.contains("default: \"\""),
            "full_name must have default: \"\""
        );
        assert!(
            fn_block.contains("description:"),
            "full_name must have a description from tooltip"
        );
        assert!(
            fn_block.contains("Full legal name of the applicant"),
            "description must contain tooltip text"
        );
    }

    #[test]
    fn scaffold_comments_is_array_multiline() {
        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        let idx = yaml.find("    comments:").expect("comments block");
        let block = &yaml[idx..];
        assert!(
            block.contains("type: array"),
            "comments must be type: array"
        );
        assert!(
            block.contains("items:\n        type: string"),
            "comments must have items.type: string"
        );
        assert!(
            block.contains("default: []"),
            "comments must have default: []"
        );
        assert!(
            block.contains("ui:\n        multiline: true"),
            "comments must have ui.multiline: true"
        );
    }

    #[test]
    fn scaffold_agree_is_boolean() {
        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        let idx = yaml.find("    agree:").expect("agree block");
        let block = &yaml[idx..];
        assert!(
            block.contains("type: boolean"),
            "agree must be type: boolean"
        );
        assert!(
            block.contains("default: false"),
            "agree must have default: false"
        );
    }

    #[test]
    fn scaffold_favorite_color_is_choice() {
        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        let idx = yaml
            .find("    favorite_color:")
            .expect("favorite_color block");
        let block = &yaml[idx..];
        assert!(
            block.contains("type: string"),
            "favorite_color must be type: string"
        );
        assert!(block.contains("enum:"), "favorite_color must have enum");
        assert!(block.contains("red"), "enum must include red");
        assert!(block.contains("green"), "enum must include green");
        assert!(block.contains("blue"), "enum must include blue");
        assert!(
            block.contains("default: red"),
            "default must be first option (red)"
        );
    }

    #[test]
    fn scaffold_header_fields() {
        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        assert!(yaml.contains("name: gov_form"), "name must match");
        assert!(yaml.contains("backend: pdfform"), "backend must be pdfform");
        assert!(yaml.contains("version: \"0.1.0\""), "version must be 0.1.0");
        assert!(
            yaml.contains("enabled: false"),
            "body must have enabled: false"
        );
    }

    #[test]
    fn scaffold_is_parse_valid() {
        use quillmark_core::quill::QuillConfig;

        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        let config =
            QuillConfig::from_yaml(&yaml).expect("scaffolded Quill.yaml must be parse-valid");

        // Confirm the 4 expected schema fields are present.
        let fields = &config.main.fields;
        assert_eq!(fields.len(), 4, "expected exactly 4 fields");
        assert!(fields.contains_key("full_name"), "missing full_name");
        assert!(fields.contains_key("comments"), "missing comments");
        assert!(fields.contains_key("agree"), "missing agree");
        assert!(
            fields.contains_key("favorite_color"),
            "missing favorite_color"
        );
        assert_eq!(config.backend, "pdfform");
    }

    #[test]
    fn scaffold_quill_from_path_roundtrip() {
        use std::fs;

        let spec = parse_gov_form();
        let yaml = scaffold_quill_yaml(&spec, "gov_form");

        // Write scaffolded Quill.yaml + sibling assets into a temp dir, then
        // load it via the full quillmark::quill_from_path path to prove the
        // output is a valid quill (not just parseable YAML).
        let tmp =
            std::env::temp_dir().join(format!("quillmark_scaffold_test_{}", std::process::id()));
        fs::create_dir_all(&tmp).expect("create temp dir");

        let cleanup = || {
            let _ = fs::remove_dir_all(&tmp);
        };

        // Write required files.
        fs::write(tmp.join("Quill.yaml"), &yaml).expect("write Quill.yaml");
        fs::write(tmp.join("form.json"), GOV_FORM_JSON).expect("write form.json");
        fs::write(tmp.join("form.pdf"), GOV_FORM_PDF).expect("write form.pdf");

        let result = quillmark::quill_from_path(&tmp);

        cleanup();

        let quill = result.expect("quill_from_path must succeed on scaffolded output");

        // Confirm the loaded quill exposes the 4 expected schema fields.
        let fields = &quill.config().main.fields;
        assert_eq!(fields.len(), 4, "expected exactly 4 schema fields");
        assert!(fields.contains_key("full_name"));
        assert!(fields.contains_key("comments"));
        assert!(fields.contains_key("agree"));
        assert!(fields.contains_key("favorite_color"));
        assert_eq!(quill.backend_id(), "pdfform");
    }
}
