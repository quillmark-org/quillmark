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
//! - **Body regions** are signalled by `main body...` after the main fence
//!   and `<card name> body...` after each card fence. The trailing ellipsis
//!   reads as "prose continues here"; no markup conflict with HTML or
//!   Markdown.
//!
//! Most UI metadata is stripped, but two semantic-structure hints are honored:
//! `ui.group` produces `# === <Group> ===` banners and `ui.order` controls
//! field ordering within a group.

use std::collections::BTreeMap;

use super::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::value::QuillValue;

impl QuillConfig {
    /// Generate an annotated Markdown blueprint for this quill.
    ///
    /// The blueprint is a reference document — consumers (typically LLMs) read
    /// it to understand the document's shape and write fresh content from
    /// scratch, not to edit it in place.
    ///
    /// Annotation rules:
    /// - Preceding `# …` comment lines, in order: description, `required`,
    ///   `enum: a | b | c`, `example: <value>`.
    ///   - `example:` is emitted only for optional fields with an example and
    ///     no enum (the example is illustrative, not prescriptive).
    /// - Inline `# <type>` annotation only for non-obvious types (number,
    ///   integer, boolean, markdown, date, datetime). String and array are
    ///   self-evident from the YAML value.
    /// - Placeholder value precedence:
    ///   - Required: example → default → type-based placeholder.
    ///   - Optional: default → type-based empty; example surfaces only as
    ///     `# example: …` above the field.
    /// - Typed tables (arrays whose `items` is a typed object) render every
    ///   item property with full annotations: an `example` or non-empty
    ///   `default` is rendered as actual rows; otherwise one synthetic row is
    ///   emitted to teach the shape.
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
        let hide_body = self
            .main
            .ui
            .as_ref()
            .and_then(|u| u.hide_body)
            .unwrap_or(false);
        if !hide_body {
            out.push_str("\nmain body...\n");
        }
        for card in &self.card_types {
            let sentinel = format!("CARD: {}  # sentinel, composable (0..N)", card.name);
            out.push('\n');
            write_card_frontmatter(&mut out, card, &sentinel, card.description.as_deref());
            let hide = card.ui.as_ref().and_then(|u| u.hide_body).unwrap_or(false);
            if !hide {
                out.push_str(&format!("\n{} body...\n", card.name));
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
            out.push_str(&format!("\n# === {} ===\n", name));
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
    sorted.sort_by_key(|f| f.ui.as_ref().and_then(|u| u.order).unwrap_or(i32::MAX));
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
    let comment = match type_annotation(&field.r#type) {
        Some(hint) => format!("  # {}", hint),
        None => String::new(),
    };
    let value = field_value(field);
    write_value(out, &field.name, &value, &comment, &pad);
}

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
    if !field.required && field.enum_values.is_none() {
        if let Some(eg) = field.example.as_ref().map(|e| eg_hint(e)) {
            out.push_str(&format!("{}# example: {}\n", pad, eg));
        }
    }
}

fn sort_props(props: &BTreeMap<String, Box<FieldSchema>>) -> Vec<&FieldSchema> {
    let mut v: Vec<&FieldSchema> = props.values().map(|b| b.as_ref()).collect();
    v.sort_by_key(|f| f.ui.as_ref().and_then(|u| u.order).unwrap_or(i32::MAX));
    v
}

/// Emit a typed-table field: description/required/enum comments, then the
/// field key, then either example/default rows or one synthetic template row.
fn write_typed_table_field(
    out: &mut String,
    field: &FieldSchema,
    item_props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);
    // Suppress `# example:` for typed tables — values or synthetic row carry it.
    write_field_comments_no_example(out, field, &pad);

    let rows = pick_table_rows(field);
    out.push_str(&format!("{}{}:\n", pad, field.name));
    match rows {
        Some(items) => write_array_items(out, &items, &pad),
        None => write_typed_table_row(out, item_props, indent),
    }
}

fn write_field_comments_no_example(out: &mut String, field: &FieldSchema, pad: &str) {
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

/// Pick concrete rows for a typed table: example (any required-ness) over
/// non-empty default. None signals "use synthetic row."
fn pick_table_rows(field: &FieldSchema) -> Option<Vec<serde_json::Value>> {
    fn non_empty(v: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
        match v {
            serde_json::Value::Array(items) if !items.is_empty() => Some(items.clone()),
            _ => None,
        }
    }
    field
        .example
        .as_ref()
        .and_then(|e| non_empty(e.as_json()))
        .or_else(|| field.default.as_ref().and_then(|d| non_empty(d.as_json())))
}

/// Emit one synthetic row of a typed-object array. The list marker `-` lives
/// on its own line at `field_indent + 1`; item keys live at `field_indent + 2`.
fn write_typed_table_row(
    out: &mut String,
    props: &BTreeMap<String, Box<FieldSchema>>,
    field_indent: usize,
) {
    let dash_pad = "  ".repeat(field_indent + 1);
    out.push_str(&format!("{}-\n", dash_pad));
    for prop in sort_props(props) {
        write_field(out, prop, field_indent + 2);
    }
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
        required_placeholder(&field.r#type, &field.name)
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
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
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
    fn optional_array_with_no_default_shows_empty_with_flow_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      example:
        - "AFM 33-326, Communications"
"#)
        .blueprint();
        // Multi-element examples render as YAML flow sequences so the full
        // shape survives. Items with embedded `, ` get YAML-quoted so the
        // flow form remains parseable.
        assert!(t.contains("# example: [\"AFM 33-326, Communications\"]\nrefs: []\n"));
    }

    #[test]
    fn optional_array_example_renders_full_flow_sequence() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    recipient:
      type: array
      example:
        - Mr. John Doe
        - 123 Main St
        - Anytown
"#)
        .blueprint();
        assert!(t.contains("# example: [Mr. John Doe, 123 Main St, Anytown]\nrecipient: []\n"));
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
        .blueprint();
        let after = &t[t.find("CARD: skills").unwrap()..];
        assert!(!after.contains("body..."));
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
        assert!(t.contains("\nmain body...\n"));
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
        assert!(t.contains("CARD: indorsement  # sentinel, composable (0..N)\n"));
        assert!(t.contains("\nindorsement body...\n"));
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
        let addressing = after_quill.find("# === Addressing ===").unwrap();
        let letterhead = after_quill.find("# === Letterhead ===").unwrap();
        let notes = after_quill.find("notes:").unwrap();
        // Ungrouped (notes) leads; Addressing precedes Letterhead.
        assert!(notes < addressing);
        assert!(addressing < letterhead);
        // No banner for the ungrouped section.
        assert!(!after_quill[..notes].contains("# ==="));
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
        assert!(t.contains("    # Publication year.\n    year: 0  # integer\n"));
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
}
