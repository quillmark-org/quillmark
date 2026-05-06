//! Auto-generated Markdown blueprint for a Quill.
//!
//! Produces an annotated reference document dense enough to replace the schema
//! for LLM consumers. The blueprint shows the document's shape — fields,
//! constraints, examples — so a consumer can write a fresh document from it.
//!
//! Annotation grammar:
//! - **Leading `# …` comment lines** carry human prose: description,
//!   `required`, `enum: a | b | c`, `example: <value>`.
//! - **Inline `# …` annotations** carry structural type/constraint info:
//!   non-obvious type hints (`# integer`, `# YYYY-MM-DD`, `# markdown`) on
//!   ordinary fields, and `# sentinel` / `# sentinel, composable (0..N)` on
//!   the `QUILL:` and `CARD:` lines respectively.
//! - **Body regions** are signalled by `Write main body here.` after the main
//!   fence and `Write <card name> body here.` after each card fence. When
//!   `body.example` is set, the example text is embedded verbatim instead.
//!   Absent when `body.enabled` is false.
//!
//! Most UI metadata is stripped, but two semantic-structure hints are honored:
//! `ui.group` produces `# ==== SECTION ====` banners and `ui.order` controls
//! field ordering within a group.

use std::collections::BTreeMap;

use super::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::document::emit::emit_double_quoted;
use crate::value::QuillValue;

impl QuillConfig {
    /// Generate an annotated Markdown blueprint for this quill. See module
    /// docs for the annotation grammar; the function is total over any valid
    /// `QuillConfig`.
    pub fn blueprint(&self) -> String {
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
            &format!("QUILL: {}@{}  # sentinel", self.name, self.version),
            main_desc,
        );
        if self.main.body_enabled() {
            let example = self.main.body.as_ref().and_then(|b| b.example.as_deref());
            let text = example.unwrap_or("Write main body here.");
            out.push_str(&format!("\n{}\n", text));
        }
        for card in &self.card_types {
            let sentinel = format!("CARD: {}  # sentinel, composable (0..N)", card.name);
            out.push('\n');
            write_card_frontmatter(&mut out, card, &sentinel, card.description.as_deref());
            if card.body_enabled() {
                let example = card.body.as_ref().and_then(|b| b.example.as_deref());
                let fallback = format!("Write {} body here.", card.name);
                let text = example.unwrap_or(fallback.as_str());
                out.push_str(&format!("\n{}\n", text));
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
    for (group, fields) in group_fields(card.fields.values()) {
        if let Some(name) = group {
            out.push_str(&format!("# ==== {} ====\n", name.to_uppercase()));
        }
        for field in fields {
            write_field(out, field, 0);
        }
    }
    out.push_str("---\n");
}

/// Partition fields by `ui.group`, preserving first-appearance order of groups
/// and sorting fields within each group by `ui.order`. Ungrouped fields form
/// the leading section (no banner).
fn group_fields<'a, I: IntoIterator<Item = &'a FieldSchema>>(
    fields: I,
) -> Vec<(Option<String>, Vec<&'a FieldSchema>)> {
    let mut sorted: Vec<&FieldSchema> = fields.into_iter().collect();
    sorted.sort_by_key(|f| ui_order(f));
    let mut groups: Vec<(Option<String>, Vec<&FieldSchema>)> = Vec::new();
    for field in sorted {
        let group = field
            .ui
            .as_ref()
            .and_then(|u| u.group.as_ref())
            .map(|s| s.to_string());
        match groups.iter_mut().find(|(g, _)| g == &group) {
            Some(slot) => slot.1.push(field),
            None => groups.push((group, vec![field])),
        }
    }
    // Ungrouped fields lead; named groups follow in first-appearance order.
    groups.sort_by_key(|(g, _)| g.is_some());
    groups
}

fn write_field(out: &mut String, field: &FieldSchema, indent: usize) {
    let pad = "  ".repeat(indent);

    // Typed table: array whose items are a typed object. Render with full
    // per-property annotations; a synthetic row when no values are supplied.
    if matches!(field.r#type, FieldType::Array) {
        if let Some(items) = &field.items {
            if matches!(items.r#type, FieldType::Object) {
                if let Some(props) = &items.properties {
                    write_typed_table_field(out, field, props, indent);
                    return;
                }
            }
        }
    }

    write_field_comments(out, field, &pad);
    write_example_comment(out, field, &pad);
    let comment = match type_annotation(&field.r#type) {
        Some(hint) => format!("  # {}", hint),
        None => String::new(),
    };
    let value = field_value(field);
    // Optional fields with no default and no enum have nothing concrete to
    // offer; comment them out so the author can uncomment what they need.
    let commented = !field.required && field.default.is_none() && field.enum_values.is_none();
    write_value(out, &field.name, &value, &comment, &pad, commented);
}

/// Description / `# required` / `# enum:` lines. Always safe to emit; carries
/// the structural prose every field needs.
fn write_field_comments(out: &mut String, field: &FieldSchema, pad: &str) {
    if let Some(desc) = &field.description {
        let clean = desc.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("{}# {}\n", pad, clean));
    }
    if field.required {
        out.push_str(&format!("{}# required\n", pad));
    }
    if let Some(vals) = &field.enum_values {
        out.push_str(&format!("{}# enum: {}\n", pad, vals.join(" | ")));
    }
}

/// `# example: …` line — emitted only for optional, non-enum fields. Required
/// fields use the example as the value; enum fields use the first enum value;
/// typed tables surface examples as actual rows.
fn write_example_comment(out: &mut String, field: &FieldSchema, pad: &str) {
    if !field.required && field.enum_values.is_none() {
        if let Some(eg) = field.example.as_ref().map(eg_hint) {
            out.push_str(&format!("{}# example: {}\n", pad, eg));
        }
    }
}

fn ui_order(f: &FieldSchema) -> i32 {
    f.ui.as_ref().and_then(|u| u.order).unwrap_or(i32::MAX)
}

fn sort_props(props: &BTreeMap<String, Box<FieldSchema>>) -> Vec<&FieldSchema> {
    let mut v: Vec<&FieldSchema> = props.values().map(|b| b.as_ref()).collect();
    v.sort_by_key(|f| ui_order(f));
    v
}

/// Emit a typed-table field: description/required/enum comments, then the
/// field key, then either example/default rows or one synthetic template row.
/// `# example:` is intentionally suppressed — the rows below carry the example.
fn write_typed_table_field(
    out: &mut String,
    field: &FieldSchema,
    item_props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);
    write_field_comments(out, field, &pad);
    out.push_str(&format!("{}{}:\n", pad, field.name));

    let concrete_rows = field
        .example
        .as_ref()
        .or(field.default.as_ref())
        .and_then(|v| match v.as_json() {
            serde_json::Value::Array(items) if !items.is_empty() => Some(items.clone()),
            _ => None,
        });

    match concrete_rows {
        Some(items) => write_array_items(out, &items, &pad),
        None => {
            let dash_pad = "  ".repeat(indent + 1);
            out.push_str(&format!("{}-\n", dash_pad));
            for prop in sort_props(item_props) {
                write_field(out, prop, indent + 2);
            }
        }
    }
}

/// The value to render for a field in the template.
enum FieldValue {
    Inline(String),                // goes on the same line as the key
    Block(Vec<serde_json::Value>), // rendered as indented items below the key
}

fn field_value(field: &FieldSchema) -> FieldValue {
    if field.required {
        // Required: example > default > type-based placeholder.
        if let Some(v) = field.example.as_ref().or(field.default.as_ref()) {
            return json_to_value(v.as_json());
        }
        placeholder(&field.r#type, Some(&field.name))
    } else {
        // Optional: default only (example goes to the `# example:` comment).
        if let Some(v) = field.default.as_ref() {
            return json_to_value(v.as_json());
        }
        // Enum with no default: first enum value is the canonical placeholder.
        if let Some(first) = field.enum_values.as_ref().and_then(|v| v.first()) {
            return FieldValue::Inline(first.clone());
        }
        placeholder(&field.r#type, None)
    }
}

/// Type-based placeholder for a field that has no usable example/default.
/// `label` is `Some(field_name)` when the field is required (string/markdown/
/// object then render as `"<field_name>"`); `None` for optional fields, which
/// fall through to an empty value.
fn placeholder(t: &FieldType, label: Option<&str>) -> FieldValue {
    match t {
        FieldType::Array => FieldValue::Inline("[]".into()),
        FieldType::Boolean => FieldValue::Inline("false".into()),
        FieldType::Number | FieldType::Integer => FieldValue::Inline("0".into()),
        // Date/datetime use empty string; type annotation carries the format hint.
        FieldType::Date | FieldType::DateTime => FieldValue::Inline("\"\"".into()),
        // String, markdown, object: angle-bracket placeholder when required;
        // empty when optional.
        _ => FieldValue::Inline(match label {
            Some(name) => format!("\"<{}>\"", name),
            None => "\"\"".into(),
        }),
    }
}

fn json_to_value(val: &serde_json::Value) -> FieldValue {
    match val {
        serde_json::Value::Array(items) if items.is_empty() => FieldValue::Inline("[]".into()),
        serde_json::Value::Array(items) => FieldValue::Block(items.clone()),
        serde_json::Value::String(s) if s.is_empty() => FieldValue::Inline("\"\"".into()),
        other => FieldValue::Inline(render_scalar(other)),
    }
}

fn write_value(
    out: &mut String,
    key: &str,
    val: &FieldValue,
    comment: &str,
    pad: &str,
    commented: bool,
) {
    match val {
        FieldValue::Inline(s) => {
            if commented {
                out.push_str(&format!("{}# {}: {}{}\n", pad, key, s, comment));
            } else {
                out.push_str(&format!("{}{}: {}{}\n", pad, key, s, comment));
            }
        }
        FieldValue::Block(items) => {
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

/// Format an example value as a compact one-line hint. Arrays render as a YAML
/// flow sequence (`[a, b, c]`) so multi-element shape information is preserved
/// without expanding into multiple comment lines.
fn eg_hint(example: &QuillValue) -> String {
    match example.as_json() {
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(render_scalar_flow).collect();
            format!("[{}]", parts.join(", "))
        }
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

/// Render a scalar in YAML flow context — strings containing flow indicators
/// (`,`, `[`, `]`, `{`, `}`) must be quoted so the surrounding `[…]` parses
/// as a single item, not a comma-split list.
fn render_scalar_flow(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => yaml_string_flow(s),
        other => render_scalar(other),
    }
}

/// Quote a YAML string only when necessary in block context.
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
        quote(s)
    } else {
        s.to_string()
    }
}

/// Quote a YAML string for flow context — adds flow indicators (`,`, `[`, `]`,
/// `{`, `}`) to the trigger set so flow-sequence items round-trip as single
/// values.
fn yaml_string_flow(s: &str) -> String {
    if s.contains([',', '[', ']', '{', '}']) {
        quote(s)
    } else {
        yaml_string(s)
    }
}

fn quote(s: &str) -> String {
    let mut out = String::new();
    emit_double_quoted(&mut out, s);
    out
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;
    use crate::Document;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid yaml")
    }

    #[test]
    fn required_string_without_example_uses_angle_bracket_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    author: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("# required\nauthor: \"<author>\"\n"));
    }

    #[test]
    fn required_field_uses_example_over_default() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, required: true, default: draft, example: final }
"#)
        .blueprint();
        assert!(t.contains("# required\nstatus: final\n"));
    }

    #[test]
    fn optional_field_uses_default_example_becomes_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: string, default: "", example: CONFIDENTIAL }
"#)
        .blueprint();
        assert!(t.contains("# example: CONFIDENTIAL\nclassification: \"\"\n"));
    }

    #[test]
    fn optional_array_example_renders_as_flow_sequence_with_context_quoting() {
        // Multi-element array examples render as YAML flow sequences so the
        // full shape survives. Items containing flow indicators (`,`, `[`, `]`,
        // `{`, `}`) get quoted; bare items don't.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    recipient:
      type: array
      example:
        - Mr. John Doe
        - 123 Main St
        - "Anytown, USA"
"#)
        .blueprint();
        assert!(t.contains(
            "# example: [Mr. John Doe, 123 Main St, \"Anytown, USA\"]\n# recipient: []\n"
        ));
    }

    #[test]
    fn enum_field_shows_values_and_no_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    format: { type: string, enum: [standard, informal], default: standard }
"#)
        .blueprint();
        assert!(t.contains("# enum: standard | informal\nformat: standard\n"));
        assert!(!t.contains("example:"));
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
        .blueprint();
        assert!(t.contains("# required\nmemo_from:\n  - ORG/SYMBOL\n  - City ST 12345\n"));
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
        .blueprint();
        assert!(t.contains("# Be brief and clear.\n# required\nsubject: \"<subject>\"\n"));
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
        .blueprint();
        assert!(t.contains("size: 11  # number"));
        assert!(t.contains("flag: false  # boolean"));
        assert!(t.contains("# body: \"\"  # markdown"));
        assert!(t.contains("# issued: \"\"  # YYYY-MM-DD"));
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
    description: A short note appended to the document.
    fields:
      author: { type: string }
"#)
        .blueprint();
        assert!(t.contains(
            "# A short note appended to the document.\nCARD: note  # sentinel, composable (0..N)\n"
        ));
    }

    #[test]
    fn body_disabled_card_omits_body_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  skills:
    body: { enabled: false }
    fields:
      items: { type: array, required: true }
"#)
        .blueprint();
        let after = &t[t.find("CARD: skills").unwrap()..];
        assert!(!after.contains("skills body"));
    }

    #[test]
    fn body_example_appears_verbatim() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  note:
    body:
      example: "This is an example note."
    fields:
      author: { type: string }
"#)
        .blueprint();
        let after = &t[t.find("CARD: note").unwrap()..];
        assert!(after.contains("\nThis is an example note.\n"));
        assert!(!after.contains("Write note body here."));
    }

    #[test]
    fn main_body_example_appears_verbatim() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Dear Sir or Madam,\n\nI am writing to..."
  fields:
    to: { type: string }
"#)
        .blueprint();
        assert!(t.contains("\nDear Sir or Madam,\n\nI am writing to...\n"));
        assert!(!t.contains("Write main body here."));
    }

    #[test]
    fn sentinel_and_body_present() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        assert!(t.starts_with("---\n# x\nQUILL: taro@0.1.0  # sentinel\n"));
        assert!(t.contains("\nWrite main body here.\n"));
    }

    #[test]
    fn card_body_placeholder_uses_card_name() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  indorsement:
    fields:
      from: { type: string }
"#)
        .blueprint();
        assert!(t.contains("\nWrite indorsement body here.\n"));
    }

    #[test]
    fn ui_groups_emit_section_banners_in_first_appearance_order() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    memo_for: { type: array, required: true, ui: { group: Addressing } }
    subject: { type: string, required: true, ui: { group: Addressing } }
    letterhead_title: { type: string, default: HQ, ui: { group: Letterhead } }
    notes: { type: string }
"#)
        .blueprint();
        let after_quill = &t[t.find("QUILL:").unwrap()..];
        let addressing = after_quill.find("# ==== ADDRESSING ====").unwrap();
        let letterhead = after_quill.find("# ==== LETTERHEAD ====").unwrap();
        let notes = after_quill.find("notes:").unwrap();
        // Ungrouped (notes) leads; Addressing precedes Letterhead.
        assert!(notes < addressing);
        assert!(addressing < letterhead);
        // No banner for the ungrouped section.
        assert!(!after_quill[..notes].contains("# ===="));
    }

    #[test]
    fn typed_table_emits_synthetic_row_when_no_example() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    references:
      type: array
      description: Cited works.
      items:
        type: object
        properties:
          org: { type: string, required: true, description: Citing organization. }
          year: { type: integer, description: Publication year. }
"#)
        .blueprint();
        assert!(t.contains("# Cited works.\nreferences:\n  -\n"));
        assert!(t.contains("    # Citing organization.\n    # required\n    org: \"<org>\"\n"));
        assert!(t.contains("    # Publication year.\n    # year: 0  # integer\n"));
    }

    #[test]
    fn typed_table_with_example_renders_example_rows() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      example:
        - { org: ACME, year: 2020 }
      items:
        type: object
        properties:
          org: { type: string, required: true }
          year: { type: integer }
"#)
        .blueprint();
        // Example rows are rendered inline; no synthetic bare-dash row, and no
        // `# example:` comment (which would be an unhelpful JSON blob).
        assert!(t.contains("refs:\n  - org: ACME\n"));
        assert!(!t.contains("refs:\n  -\n"));
        assert!(!t.contains("# example:"));
    }

    #[test]
    fn typed_table_with_default_renders_default_rows() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default:
        - { org: ACME }
      items:
        type: object
        properties:
          org: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("refs:\n  - org: ACME\n"));
        assert!(!t.contains("refs:\n  -\n"));
    }

    #[test]
    fn typed_table_with_empty_default_falls_through_to_synthetic_row() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default: []
      items:
        type: object
        properties:
          org: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("refs:\n  -\n    # required\n    org: \"<org>\"\n"));
    }

    const LETTER_QUILL: &str = r#"
quill: { name: letter, version: 1.0.0, backend: typst, description: A formal letter. }
main:
  fields:
    to:
      type: string
      required: true
      description: Recipient name.
    subject:
      type: string
      required: true
    date:
      type: date
    priority:
      type: string
      enum: [normal, urgent]
      default: normal
    attachments:
      type: array
      example:
        - report.pdf
card_types:
  enclosure:
    description: An enclosure attached to the letter.
    fields:
      label: { type: string, required: true }
      pages: { type: integer, default: 1 }
"#;

    #[test]
    fn optional_no_default_field_is_commented_out() {
        // No default, no enum → value line gets a leading `# `.
        // Description and `# example:` comments are still emitted above it.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    note:
      type: string
      description: An optional note.
      example: See attached.
    count: { type: integer }
    flag: { type: boolean }
    issued: { type: date }
    tags: { type: array }
"#)
        .blueprint();
        assert!(t.contains("# An optional note.\n# example: See attached.\n# note: \"\"\n"));
        assert!(t.contains("# count: 0  # integer\n"));
        assert!(t.contains("# flag: false  # boolean\n"));
        assert!(t.contains("# issued: \"\"  # YYYY-MM-DD\n"));
        assert!(t.contains("# tags: []\n"));
    }

    #[test]
    fn optional_with_default_stays_active() {
        // A default value is meaningful; the field line must not be commented out.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    priority: { type: string, default: normal }
    count: { type: integer, default: 0 }
"#)
        .blueprint();
        assert!(t.contains("priority: normal\n"));
        assert!(t.contains("count: 0  # integer\n"));
        assert!(!t.contains("# priority:"));
        assert!(!t.contains("# count:"));
    }

    #[test]
    fn blueprint_round_trips_idempotently() {
        let bp = cfg(LETTER_QUILL).blueprint();
        let doc1 = Document::from_markdown(&bp).expect("blueprint must parse");
        let md2 = doc1.to_markdown();
        let doc2 = Document::from_markdown(&md2).expect("round-tripped markdown must parse");
        assert_eq!(
            doc1, doc2,
            "Document must be equal after blueprint → parse → emit → parse"
        );
    }
}
