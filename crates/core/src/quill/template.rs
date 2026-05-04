//! Auto-generated Markdown template for a Quill, combining structure and format
//! into a single fill-in-the-blank document for LLM consumers.

use super::{CardSchema, FieldSchema, FieldType, QuillConfig};

impl QuillConfig {
    /// Generate a fill-in-the-blank Markdown template for this quill.
    ///
    /// The template shows the correct document structure (YAML frontmatter +
    /// body + cards) with placeholder values and inline comments for required
    /// fields and enum constraints. Placeholder precedence: `example` →
    /// `default` → type-based placeholder.
    pub fn template(&self) -> String {
        let mut out = String::new();
        write_card_frontmatter(
            &mut out,
            &self.main,
            &format!("QUILL: {}@{}", self.name, self.version),
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
            let card_label = match &card.title {
                Some(t) => format!("CARD: {}  # {} (optional, repeat as needed)", card.name, t),
                None => format!("CARD: {}  # (optional, repeat as needed)", card.name),
            };
            out.push('\n');
            write_card_frontmatter(&mut out, card, &card_label);
            let card_hide_body = card.ui.as_ref().and_then(|u| u.hide_body).unwrap_or(false);
            if !card_hide_body {
                out.push_str("\n<body>\n");
            }
        }
        out
    }
}

fn write_card_frontmatter(out: &mut String, card: &CardSchema, first_line: &str) {
    out.push_str("---\n");
    out.push_str(first_line);
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

    let mut comment_parts: Vec<&str> = Vec::new();
    if field.required {
        comment_parts.push("required");
    }
    let enum_str;
    if let Some(vals) = &field.enum_values {
        enum_str = vals.join(" | ");
        comment_parts.push(&enum_str);
    }
    let comment = if comment_parts.is_empty() {
        String::new()
    } else {
        format!("  # {}", comment_parts.join(" | "))
    };

    let source = field
        .example
        .as_ref()
        .or(field.default.as_ref())
        .map(|v| v.as_json());

    match source {
        Some(serde_json::Value::Array(items)) if !items.is_empty() => {
            out.push_str(&format!("{}{}:{}\n", pad, field.name, comment));
            write_array_items(out, items, indent + 1);
        }
        Some(serde_json::Value::Array(_)) => {
            out.push_str(&format!("{}{}: []{}\n", pad, field.name, comment));
        }
        Some(val) => {
            let rendered = render_scalar(val);
            out.push_str(&format!("{}{}: {}{}\n", pad, field.name, rendered, comment));
        }
        None => write_type_placeholder(out, field, &pad, &comment),
    }
}

fn write_array_items(out: &mut String, items: &[serde_json::Value], indent: usize) {
    let pad = "  ".repeat(indent);
    for item in items {
        match item {
            serde_json::Value::Object(map) => {
                let mut entries = map.iter();
                if let Some((first_key, first_val)) = entries.next() {
                    out.push_str(&format!(
                        "{}- {}: {}\n",
                        pad,
                        first_key,
                        render_scalar(first_val)
                    ));
                    let inner_pad = format!("{}  ", pad);
                    for (k, v) in entries {
                        out.push_str(&format!("{}{}: {}\n", inner_pad, k, render_scalar(v)));
                    }
                }
            }
            _ => out.push_str(&format!("{}- {}\n", pad, render_scalar(item))),
        }
    }
}

fn write_type_placeholder(out: &mut String, field: &FieldSchema, pad: &str, comment: &str) {
    match field.r#type {
        FieldType::Array => {
            out.push_str(&format!("{}{}: []{}\n", pad, field.name, comment));
        }
        FieldType::Boolean => {
            out.push_str(&format!("{}{}: false{}\n", pad, field.name, comment));
        }
        FieldType::Number | FieldType::Integer => {
            out.push_str(&format!("{}{}: 0{}\n", pad, field.name, comment));
        }
        FieldType::Date => {
            out.push_str(&format!(
                "{}{}: \"\"  # YYYY-MM-DD{}\n",
                pad,
                field.name,
                if comment.is_empty() {
                    String::new()
                } else {
                    comment.trim_start_matches("  ").to_string()
                }
            ));
        }
        FieldType::DateTime => {
            out.push_str(&format!(
                "{}{}: \"\"  # ISO 8601{}\n",
                pad,
                field.name,
                if comment.is_empty() {
                    String::new()
                } else {
                    comment.trim_start_matches("  ").to_string()
                }
            ));
        }
        _ => {
            let label = field.title.as_deref().unwrap_or(&field.name);
            out.push_str(&format!(
                "{}{}: \"<{}>\"{}\n",
                pad, field.name, label, comment
            ));
        }
    }
}

/// Render a JSON scalar (or array/object fallback) as a YAML-safe string.
fn render_scalar(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => yaml_string(s),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        // Arrays/objects at scalar position: shouldn't appear but fall back gracefully
        other => yaml_string(&other.to_string()),
    }
}

/// Quote a string for YAML if it contains characters that would be misinterpreted.
fn yaml_string(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || matches!(
            s,
            "true" | "false" | "null" | "yes" | "no" | "on" | "off"
        )
        || s.starts_with(|c: char| matches!(c, '{' | '[' | '&' | '*' | '!' | '|' | '>' | '\'' | '"' | '%' | '@' | '`'))
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
    fn simple_quill_has_sentinel_and_body() {
        let config = cfg(r#"
quill:
  name: taro
  version: 0.1.0
  backend: typst
  description: test
main:
  fields:
    author:
      type: string
      title: Author
      required: true
    flavor:
      type: string
      default: taro
"#);
        let t = config.template();
        assert!(t.contains("QUILL: taro@0.1.0"));
        assert!(t.contains("author: \"<Author>\"  # required"));
        assert!(t.contains("flavor: taro"));
        assert!(t.contains("<body>"));
    }

    #[test]
    fn example_takes_precedence_over_default() {
        let config = cfg(r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    status:
      type: string
      default: draft
      example: final
"#);
        let t = config.template();
        assert!(t.contains("status: final"));
        assert!(!t.contains("draft"));
    }

    #[test]
    fn enum_comment_appended() {
        let config = cfg(r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    format:
      type: string
      enum: [standard, informal, separate_page]
      default: standard
"#);
        let t = config.template();
        assert!(t.contains("format: standard  # standard | informal | separate_page"));
    }

    #[test]
    fn array_example_rendered_as_items() {
        let config = cfg(r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    memo_from:
      type: array
      required: true
      example:
        - ORG/SYMBOL
        - City ST 12345
"#);
        let t = config.template();
        assert!(t.contains("memo_from:  # required\n  - ORG/SYMBOL\n  - City ST 12345\n"));
    }

    #[test]
    fn card_has_sentinel_and_repeat_comment() {
        let config = cfg(r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    title:
      type: string
card_types:
  quote:
    title: Inspirational Quote
    fields:
      author:
        type: string
        required: true
"#);
        let t = config.template();
        assert!(t.contains("CARD: quote  # Inspirational Quote (optional, repeat as needed)"));
        assert!(t.contains("author: \"<author>\"  # required"));
    }

    #[test]
    fn hide_body_card_omits_body_placeholder() {
        let config = cfg(r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    title:
      type: string
card_types:
  skills:
    ui:
      hide_body: true
    fields:
      items:
        type: array
        required: true
"#);
        let t = config.template();
        let skills_pos = t.find("CARD: skills").unwrap();
        let after_skills = &t[skills_pos..];
        // Should not have <body> after the skills card
        assert!(!after_skills.contains("<body>"));
    }

    #[test]
    fn date_type_gets_format_hint() {
        let config = cfg(r#"
quill:
  name: x
  version: 1.0.0
  backend: typst
  description: x
main:
  fields:
    issued:
      type: date
"#);
        let t = config.template();
        assert!(t.contains("issued: \"\"  # YYYY-MM-DD"));
    }
}
