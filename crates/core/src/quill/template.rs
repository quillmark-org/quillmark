//! Auto-generated Markdown template for a Quill.
//!
//! Produces a fill-in-the-blank document that is dense enough to replace the
//! schema for LLM consumers. Each field is annotated with inline comments
//! (type, required, enum constraints, e.g. hint) and a preceding description
//! comment where the schema declares one. No UI metadata is emitted.

use super::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

impl QuillConfig {
    /// Generate a fill-in-the-blank Markdown template for this quill.
    ///
    /// Annotation rules:
    /// - Preceding `# <description>` comment for every field that declares one.
    /// - Inline `# type | required | enum… | e.g. …` comment.
    ///   - Type annotation only for non-obvious types (number, integer, boolean,
    ///     markdown, object, date, datetime).
    ///   - `required` marker when the field is required.
    ///   - Enum values when declared.
    ///   - `e.g. <value>` for optional fields that have an example but no enum
    ///     (the example is illustrative, not prescriptive).
    /// - Placeholder value precedence:
    ///   - Required: example → default → type-based placeholder.
    ///   - Optional: default → type-based empty; example → `e.g.` comment only.
    pub fn template(&self) -> String {
        let mut out = String::new();
        let main_desc = self
            .main
            .description
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| Some(self.description.as_str()).filter(|s| !s.is_empty()));
        write_card_frontmatter(
            &mut out,
            &self.main,
            &format!("QUILL: {}@{}", self.name, self.version),
            main_desc,
        );
        let hide_body = self
            .main
            .ui
            .as_ref()
            .and_then(|u| u.hide_body)
            .unwrap_or(false);
        if !hide_body {
            out.push_str("\n<body>\n");
        }
        for card in &self.card_types {
            let sentinel = match &card.title {
                Some(t) => format!("CARD: {}  # {}", card.name, t),
                None => format!("CARD: {}", card.name),
            };
            out.push('\n');
            write_card_frontmatter(&mut out, card, &sentinel, card.description.as_deref());
            let hide = card.ui.as_ref().and_then(|u| u.hide_body).unwrap_or(false);
            if !hide {
                out.push_str("\n<body>\n");
            }
        }
        out
    }
}

fn write_card_frontmatter(
    out: &mut String,
    card: &CardSchema,
    sentinel_line: &str,
    description: Option<&str>,
) {
    out.push_str("---\n");
    if let Some(desc) = description {
        for line in desc.lines() {
            out.push_str(&format!("# {}\n", line));
        }
    }
    out.push_str(sentinel_line);
    out.push('\n');
    let mut fields: Vec<&FieldSchema> = card.fields.values().collect();
    fields.sort_by_key(|f| f.ui.as_ref().and_then(|u| u.order).unwrap_or(i32::MAX));
    for field in fields {
        write_field(out, field, 0);
    }
    out.push_str("---\n");
}

fn write_field(out: &mut String, field: &FieldSchema, indent: usize) {
    let pad = "  ".repeat(indent);

    // Preceding description comment.
    if let Some(desc) = &field.description {
        let clean = desc.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("{}# {}\n", pad, clean));
    }

    // Build inline comment parts: type | required | enums | e.g.
    let mut parts: Vec<String> = Vec::new();

    if let Some(hint) = type_annotation(&field.r#type) {
        parts.push(hint.to_string());
    }
    if field.required {
        parts.push("required".to_string());
    }
    if let Some(vals) = &field.enum_values {
        parts.push(vals.join(" | "));
    }
    // e.g. hint: only for optional fields with an example and no enum.
    if !field.required && field.enum_values.is_none() {
        if let Some(eg) = field.example.as_ref().map(|e| eg_hint(e)) {
            parts.push(format!("e.g. {}", eg));
        }
    }

    let comment = if parts.is_empty() {
        String::new()
    } else {
        format!("  # {}", parts.join(" | "))
    };

    let value = field_value(field);
    write_value(out, &field.name, &value, &comment, &pad);
}

/// The value to render for a field in the template.
enum FieldValue {
    Scalar(String),
    Array(Vec<serde_json::Value>),
    EmptyArray,
    Empty, // renders as ""
}

fn field_value(field: &FieldSchema) -> FieldValue {
    if field.required {
        // Required: example > default > type-based placeholder.
        if let Some(v) = field.example.as_ref().or(field.default.as_ref()) {
            return json_to_value(v.as_json(), &field.r#type);
        }
        required_placeholder(&field.r#type, field.title.as_deref().unwrap_or(&field.name))
    } else {
        // Optional: default only (example goes to e.g. comment).
        if let Some(v) = field.default.as_ref() {
            return json_to_value(v.as_json(), &field.r#type);
        }
        // Enum with no default: first enum value is the canonical placeholder.
        if let Some(first) = field.enum_values.as_ref().and_then(|v| v.first()) {
            return FieldValue::Scalar(first.clone());
        }
        optional_placeholder(&field.r#type)
    }
}

fn required_placeholder(t: &FieldType, label: &str) -> FieldValue {
    match t {
        FieldType::Array => FieldValue::EmptyArray,
        FieldType::Boolean => FieldValue::Scalar("false".to_string()),
        FieldType::Number | FieldType::Integer => FieldValue::Scalar("0".to_string()),
        // Date/datetime use empty string; type annotation carries the format hint.
        FieldType::Date | FieldType::DateTime => FieldValue::Empty,
        // String, markdown, object: angle-bracket placeholder signals "fill this in".
        _ => FieldValue::Scalar(format!("\"<{}>\"", label)),
    }
}

fn optional_placeholder(t: &FieldType) -> FieldValue {
    match t {
        FieldType::Array => FieldValue::EmptyArray,
        FieldType::Boolean => FieldValue::Scalar("false".to_string()),
        FieldType::Number | FieldType::Integer => FieldValue::Scalar("0".to_string()),
        _ => FieldValue::Empty,
    }
}

fn json_to_value(val: &serde_json::Value, _t: &FieldType) -> FieldValue {
    match val {
        serde_json::Value::Array(items) if items.is_empty() => FieldValue::EmptyArray,
        serde_json::Value::Array(items) => FieldValue::Array(items.clone()),
        serde_json::Value::String(s) if s.is_empty() => FieldValue::Empty,
        other => FieldValue::Scalar(render_scalar(other)),
    }
}

fn write_value(out: &mut String, key: &str, val: &FieldValue, comment: &str, pad: &str) {
    match val {
        FieldValue::Scalar(s) => {
            out.push_str(&format!("{}{}: {}{}\n", pad, key, s, comment));
        }
        FieldValue::Empty => {
            out.push_str(&format!("{}{}: \"\"{}\n", pad, key, comment));
        }
        FieldValue::EmptyArray => {
            out.push_str(&format!("{}{}: []{}\n", pad, key, comment));
        }
        FieldValue::Array(items) => {
            out.push_str(&format!("{}{}:{}\n", pad, key, comment));
            write_array_items(out, items, pad);
        }
    }
}

fn write_array_items(out: &mut String, items: &[serde_json::Value], pad: &str) {
    let item_pad = format!("{}  ", pad);
    for item in items {
        match item {
            serde_json::Value::Object(map) => {
                let mut entries = map.iter();
                if let Some((first_key, first_val)) = entries.next() {
                    out.push_str(&format!(
                        "{}- {}: {}\n",
                        item_pad,
                        first_key,
                        render_scalar(first_val)
                    ));
                    let inner = format!("{}  ", item_pad);
                    for (k, v) in entries {
                        out.push_str(&format!("{}{}: {}\n", inner, k, render_scalar(v)));
                    }
                }
            }
            _ => out.push_str(&format!("{}- {}\n", item_pad, render_scalar(item))),
        }
    }
}

/// Inline type annotation for non-obvious types. `string` and `array` are
/// self-evident from the YAML value; no annotation needed.
fn type_annotation(t: &FieldType) -> Option<&'static str> {
    match t {
        FieldType::Number => Some("number"),
        FieldType::Integer => Some("integer"),
        FieldType::Boolean => Some("boolean"),
        FieldType::Markdown => Some("markdown"),
        FieldType::Object => Some("object"),
        FieldType::Date => Some("YYYY-MM-DD"),
        FieldType::DateTime => Some("ISO 8601"),
        FieldType::String | FieldType::Array => None,
    }
}

/// Format the first (or only) value of an example as a compact e.g. hint.
fn eg_hint(example: &QuillValue) -> String {
    match example.as_json() {
        serde_json::Value::Array(items) => items
            .first()
            .map(|v| render_scalar(v))
            .unwrap_or_default(),
        val => render_scalar(val),
    }
}

fn render_scalar(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => yaml_string(s),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => yaml_string(&other.to_string()),
    }
}

/// Quote a YAML string only when necessary.
fn yaml_string(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || matches!(s, "true" | "false" | "null" | "yes" | "no" | "on" | "off")
        || s.starts_with(|c: char| {
            matches!(
                c,
                '{' | '[' | '&' | '*' | '!' | '|' | '>' | '\'' | '"' | '%' | '@' | '`'
            )
        })
        || s.contains(": ")
        || s.contains(" #")
        || s.starts_with("- ")
        || s.starts_with('#');
    if needs_quotes {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quill::QuillConfig;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid yaml")
    }

    #[test]
    fn required_string_without_example_uses_angle_bracket_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    author: { type: string, title: Author, required: true }
"#)
        .template();
        assert!(t.contains("author: \"<Author>\"  # required"));
    }

    #[test]
    fn required_field_uses_example_over_default() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, required: true, default: draft, example: final }
"#)
        .template();
        assert!(t.contains("status: final  # required"));
    }

    #[test]
    fn optional_field_uses_default_example_becomes_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: string, default: "", example: CONFIDENTIAL }
"#)
        .template();
        assert!(t.contains("classification: \"\"  # e.g. CONFIDENTIAL"));
    }

    #[test]
    fn optional_array_with_no_default_shows_empty_with_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      example:
        - AFM 33-326, Communications
"#)
        .template();
        assert!(t.contains("refs: []  # e.g. AFM 33-326, Communications"));
    }

    #[test]
    fn enum_field_shows_values_and_no_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    format: { type: string, enum: [standard, informal], default: standard }
"#)
        .template();
        assert!(t.contains("format: standard  # standard | informal"));
        assert!(!t.contains("e.g."));
    }

    #[test]
    fn required_array_uses_example_as_items() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    memo_from:
      type: array
      required: true
      example:
        - ORG/SYMBOL
        - City ST 12345
"#)
        .template();
        assert!(t.contains("memo_from:  # required\n  - ORG/SYMBOL\n  - City ST 12345\n"));
    }

    #[test]
    fn description_emitted_as_preceding_comment() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    subject:
      type: string
      required: true
      description: Be brief and clear.
"#)
        .template();
        assert!(t.contains("# Be brief and clear.\nsubject: \"<subject>\"  # required\n"));
    }

    #[test]
    fn non_obvious_types_get_annotation() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    body: { type: markdown }
    issued: { type: date }
"#)
        .template();
        assert!(t.contains("size: 11  # number"));
        assert!(t.contains("flag: false  # boolean"));
        assert!(t.contains("body: \"\"  # markdown"));
        assert!(t.contains("issued: \"\"  # YYYY-MM-DD"));
    }

    #[test]
    fn card_description_emitted_after_sentinel() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  note:
    title: Note
    description: A short note appended to the document.
    fields:
      author: { type: string }
"#)
        .template();
        assert!(t.contains(
            "# A short note appended to the document.\nCARD: note  # Note\n"
        ));
    }

    #[test]
    fn hide_body_card_omits_body_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  skills:
    ui: { hide_body: true }
    fields:
      items: { type: array, required: true }
"#)
        .template();
        let after = &t[t.find("CARD: skills").unwrap()..];
        assert!(!after.contains("<body>"));
    }

    #[test]
    fn sentinel_and_body_present() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .template();
        assert!(t.starts_with("---\n# x\nQUILL: taro@0.1.0\n"));
        assert!(t.contains("<body>"));
    }
}
