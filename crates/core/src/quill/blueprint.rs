//! Auto-generated Markdown blueprint for a Quill.
//!
//! Produces an annotated reference document dense enough to replace the schema
//! for LLM consumers. The blueprint shows the document's shape — fields,
//! constraints, examples — so a consumer can write a fresh document from it.
//!
//! Annotation grammar:
//! - **Leading `# …` lines** carry prose: `# <description>` (single line,
//!   whitespace-collapsed) and `# e.g. <value>` (whenever an `example:` is
//!   configured, regardless of role or type).
//! - **Inline `# …` annotation** on the value line is structural:
//!   `# <type>[<format>]; <role>[, <extras>...]`. Type is mandatory on every
//!   field. Format slot uses angle brackets (`array<string>`,
//!   `date<YYYY-MM-DD>`, `enum<a | b | c>`). Role is `required`, `optional`,
//!   or `composable (0..N)` for leaves. The QUILL sentinel adds a `verbatim`
//!   extra signaling that the value must not be modified.
//! - **Body regions** are signalled by `Write main body here.` after the main
//!   fence and `Write <leaf name> body here.` after each leaf fence. When
//!   `body.example` is set, the example text is embedded verbatim instead.
//!   Absent when `body.enabled` is false.
//!
//! `ui.order` controls field ordering. `ui.group` clusters fields together
//! within the document but emits no banner.

use std::collections::BTreeMap;

use super::{FieldSchema, FieldType, LeafSchema, QuillConfig};
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
        write_fence_block(
            &mut out,
            &self.main,
            &format!(
                "QUILL: {}@{}  # sentinel; required, verbatim",
                self.name, self.version
            ),
            main_desc,
            "---\n",
            "---\n",
        );
        if self.main.body_enabled() {
            let example = self.main.body.as_ref().and_then(|b| b.example.as_deref());
            let text = example.unwrap_or("Write main body here.");
            out.push_str(&format!("\n{}\n", text));
        }
        for leaf in &self.leaf_kinds {
            let sentinel = format!("KIND: {}  # sentinel; composable (0..N)", leaf.name);
            out.push('\n');
            write_fence_block(
                &mut out,
                leaf,
                &sentinel,
                leaf.description.as_deref(),
                "```leaf\n",
                "```\n",
            );
            if leaf.body_enabled() {
                let example = leaf.body.as_ref().and_then(|b| b.example.as_deref());
                let fallback = format!("Write {} body here.", leaf.name);
                let text = example.unwrap_or(fallback.as_str());
                out.push_str(&format!("\n{}\n", text));
            }
        }
        out
    }
}

fn write_fence_block(
    out: &mut String,
    leaf: &LeafSchema,
    sentinel_line: &str,
    description: Option<&str>,
    open_fence: &str,
    close_fence: &str,
) {
    out.push_str(open_fence);
    if let Some(desc) = description {
        let clean = desc.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("# {}\n", clean));
    }
    out.push_str(sentinel_line);
    out.push('\n');
    for (_, fields) in group_fields(leaf.fields.values()) {
        for field in fields {
            write_field(out, field, 0);
        }
    }
    out.push_str(close_fence);
}

/// Cluster fields by `ui.group` (preserving first-appearance order; ungrouped
/// fields lead) and sort within each group by `ui.order`. The grouping is
/// purely positional now — no banner is emitted.
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
    groups.sort_by_key(|(g, _)| g.is_some());
    groups
}

fn write_field(out: &mut String, field: &FieldSchema, indent: usize) {
    let pad = "  ".repeat(indent);

    // Typed table: array with a properties map directly on the field.
    if matches!(field.r#type, FieldType::Array) {
        if let Some(props) = &field.properties {
            write_typed_table_field(out, field, props, indent);
            return;
        }
    }

    // Typed dictionary: standalone object with defined properties.
    if matches!(field.r#type, FieldType::Object) {
        if let Some(props) = &field.properties {
            write_typed_object_field(out, field, props, indent);
            return;
        }
    }

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    // Markdown fields render as a YAML block scalar so multi-line content has
    // a consistent shape regardless of whether a default is configured.
    if matches!(field.r#type, FieldType::Markdown) {
        let inline = inline_annotation(field, false);
        write_markdown_block(out, field, &pad, &inline);
        return;
    }

    let inline = format!("  # {}", inline_annotation(field, false));
    let value = field_value(field);
    write_value(out, &field.name, &value, &inline, &pad);
}

fn write_description(out: &mut String, field: &FieldSchema, pad: &str) {
    if let Some(desc) = &field.description {
        let clean = desc.split_whitespace().collect::<Vec<_>>().join(" ");
        if !clean.is_empty() {
            out.push_str(&format!("{}# {}\n", pad, clean));
        }
    }
}

/// `# e.g. <value>` — emitted whenever `example:` is configured on the field.
/// Independent of role, type, or enum-ness; examples never become rendered
/// values.
fn write_eg_comment(out: &mut String, field: &FieldSchema, pad: &str) {
    if let Some(eg) = field.example.as_ref().map(eg_hint) {
        out.push_str(&format!("{}# e.g. {}\n", pad, eg));
    }
}

fn write_markdown_block(out: &mut String, field: &FieldSchema, pad: &str, inline: &str) {
    out.push_str(&format!("{}{}: |-  # {}\n", pad, field.name, inline));
    let body_pad = format!("{}  ", pad);
    // Render default content if present; otherwise leave one indented blank
    // line so the block scalar has a body for the author to fill in.
    let content = field.default.as_ref().and_then(|v| match v.as_json() {
        serde_json::Value::String(s) => Some(s),
        _ => None,
    });
    match content {
        Some(text) if !text.is_empty() => {
            for line in text.lines() {
                out.push_str(&format!("{}{}\n", body_pad, line));
            }
        }
        _ => {
            out.push_str(&format!("{}\n", body_pad));
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

/// Emit a typed-table field: description + optional `# e.g.` line, then the
/// field key with its `array<object>; <role>` inline annotation, then either
/// example/default rows or a synthetic template row. When concrete rows are
/// rendered, the `# e.g.` comment is suppressed (the rows themselves carry
/// the example shape).
fn write_typed_table_field(
    out: &mut String,
    field: &FieldSchema,
    item_props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);

    let concrete_rows = field
        .example
        .as_ref()
        .or(field.default.as_ref())
        .and_then(|v| match v.as_json() {
            serde_json::Value::Array(items) if !items.is_empty() => Some(items.clone()),
            _ => None,
        });

    write_description(out, field, &pad);
    if concrete_rows.is_none() {
        write_eg_comment(out, field, &pad);
    }

    let inline = inline_annotation(field, true);
    out.push_str(&format!("{}{}:  # {}\n", pad, field.name, inline));

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

/// Emit a typed-dictionary field: description + optional `# e.g.` line, then the
/// field key with its `object; <role>` inline annotation, then either a concrete
/// mapping from example/default or per-property annotations. When concrete values
/// are rendered, the `# e.g.` comment is suppressed (the mapping itself carries
/// the example shape).
fn write_typed_object_field(
    out: &mut String,
    field: &FieldSchema,
    props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);

    let concrete = field
        .example
        .as_ref()
        .or(field.default.as_ref())
        .and_then(|v| match v.as_json() {
            serde_json::Value::Object(map) if !map.is_empty() => Some(map.clone()),
            _ => None,
        });

    write_description(out, field, &pad);
    if concrete.is_none() {
        write_eg_comment(out, field, &pad);
    }

    let inline = inline_annotation(field, false);
    out.push_str(&format!("{}{}:  # {}\n", pad, field.name, inline));

    match concrete {
        Some(map) => {
            let inner_pad = format!("{}  ", pad);
            for (k, v) in &map {
                out.push_str(&format!("{}{}: {}\n", inner_pad, k, render_scalar(v)));
            }
        }
        None => {
            for prop in sort_props(props) {
                write_field(out, prop, indent + 1);
            }
        }
    }
}

/// Build the inline annotation body (without the leading `# `).
/// `force_array_object` is `true` for typed-table outer fields, which always
/// renders as `array<object>`; plain arrays render as `array<string>`.
fn inline_annotation(field: &FieldSchema, force_array_object: bool) -> String {
    let role = if field.required {
        "required"
    } else {
        "optional"
    };
    let type_expr = type_expression(field, force_array_object);
    format!("{}; {}", type_expr, role)
}

fn type_expression(field: &FieldSchema, force_array_object: bool) -> String {
    if let Some(values) = &field.enum_values {
        return format!("enum<{}>", values.join(" | "));
    }
    match field.r#type {
        FieldType::String => "string".into(),
        FieldType::Number => "number".into(),
        FieldType::Integer => "integer".into(),
        FieldType::Boolean => "boolean".into(),
        FieldType::Object => "object".into(),
        FieldType::Markdown => "markdown".into(),
        FieldType::Date => "date<YYYY-MM-DD>".into(),
        FieldType::DateTime => "datetime<ISO 8601>".into(),
        FieldType::Array => {
            let item = if force_array_object {
                "object"
            } else {
                "string"
            };
            format!("array<{}>", item)
        }
    }
}

/// The value to render for a field. Single cascade independent of role:
/// default → first enum value → type-empty.
enum FieldValue {
    Inline(String),
    Block(Vec<serde_json::Value>),
}

fn field_value(field: &FieldSchema) -> FieldValue {
    if let Some(v) = field.default.as_ref() {
        return json_to_value(v.as_json());
    }
    if let Some(first) = field.enum_values.as_ref().and_then(|v| v.first()) {
        return FieldValue::Inline(first.clone());
    }
    type_empty(&field.r#type)
}

fn type_empty(t: &FieldType) -> FieldValue {
    match t {
        FieldType::Array => FieldValue::Inline("[]".into()),
        FieldType::Boolean => FieldValue::Inline("false".into()),
        FieldType::Number | FieldType::Integer => FieldValue::Inline("0".into()),
        FieldType::Date | FieldType::DateTime => FieldValue::Inline("\"\"".into()),
        // String, markdown, object: empty string. Markdown is special-cased
        // earlier in `write_field` and never reaches this code path.
        _ => FieldValue::Inline("\"\"".into()),
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

fn write_value(out: &mut String, key: &str, val: &FieldValue, comment: &str, pad: &str) {
    match val {
        FieldValue::Inline(s) => {
            out.push_str(&format!("{}{}: {}{}\n", pad, key, s, comment));
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
    fn required_string_renders_empty_with_required_role() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    author: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("author: \"\"  # string; required\n"));
    }

    #[test]
    fn required_field_with_example_does_not_use_example_as_value() {
        // Examples never render as values — they always surface in `# e.g.`.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, required: true, default: draft, example: final }
"#)
        .blueprint();
        assert!(t.contains("# e.g. final\nstatus: draft  # string; required\n"));
    }

    #[test]
    fn optional_field_default_renders_as_value_with_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: string, default: "", example: CONFIDENTIAL }
"#)
        .blueprint();
        assert!(t.contains("# e.g. CONFIDENTIAL\nclassification: \"\"  # string; optional\n"));
    }

    #[test]
    fn optional_array_example_renders_as_flow_sequence_with_context_quoting() {
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
            "# e.g. [Mr. John Doe, 123 Main St, \"Anytown, USA\"]\nrecipient: []  # array<string>; optional\n"
        ));
    }

    #[test]
    fn enum_field_uses_enum_format_slot_and_no_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    format: { type: string, enum: [standard, informal], default: standard }
"#)
        .blueprint();
        assert!(t.contains("format: standard  # enum<standard | informal>; optional\n"));
        assert!(!t.contains("e.g."));
    }

    #[test]
    fn required_array_with_example_renders_eg_only_not_value() {
        // Plain (non-typed-table) required arrays render type-empty; the
        // example surfaces in the leading `# e.g.` line.
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
        assert!(t.contains(
            "# e.g. [ORG/SYMBOL, City ST 12345]\nmemo_from: []  # array<string>; required\n"
        ));
    }

    #[test]
    fn description_emitted_as_single_line() {
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
        assert!(t.contains("# Be brief and clear.\nsubject: \"\"  # string; required\n"));
    }

    #[test]
    fn every_field_carries_inline_type_and_role() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    issued: { type: date }
    published: { type: datetime }
    refs: { type: array }
"#)
        .blueprint();
        assert!(t.contains("title: \"\"  # string; optional\n"));
        assert!(t.contains("size: 11  # number; optional\n"));
        assert!(t.contains("flag: false  # boolean; optional\n"));
        assert!(t.contains("issued: \"\"  # date<YYYY-MM-DD>; optional\n"));
        assert!(t.contains("published: \"\"  # datetime<ISO 8601>; optional\n"));
        assert!(t.contains("refs: []  # array<string>; optional\n"));
    }

    #[test]
    fn markdown_field_renders_as_block_scalar() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown }
"#)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown; optional\n  \n"));
    }

    #[test]
    fn markdown_field_with_default_fills_block() {
        let t = cfg(r###"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio:
      type: markdown
      default: "## About me\n\nHello."
"###)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown; optional\n  ## About me\n  \n  Hello.\n"));
    }

    #[test]
    fn quill_sentinel_line_is_required_verbatim() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        assert!(t.starts_with("---\n# x\nQUILL: taro@0.1.0  # sentinel; required, verbatim\n"));
        assert!(t.contains("\nWrite main body here.\n"));
    }

    #[test]
    fn leaf_sentinel_line_is_composable() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
leaf_kinds:
  note:
    description: A short note appended to the document.
    fields:
      author: { type: string }
"#)
        .blueprint();
        assert!(t.contains(
            "# A short note appended to the document.\nKIND: note  # sentinel; composable (0..N)\n"
        ));
    }

    #[test]
    fn body_disabled_leaf_omits_body_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
leaf_kinds:
  skills:
    body: { enabled: false }
    fields:
      items: { type: array, required: true }
"#)
        .blueprint();
        let after = &t[t.find("KIND: skills").unwrap()..];
        assert!(!after.contains("skills body"));
    }

    #[test]
    fn body_example_appears_verbatim() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
leaf_kinds:
  note:
    body:
      example: "This is an example note."
    fields:
      author: { type: string }
"#)
        .blueprint();
        let after = &t[t.find("KIND: note").unwrap()..];
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
    fn leaf_body_placeholder_uses_leaf_name() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
leaf_kinds:
  indorsement:
    fields:
      from: { type: string }
"#)
        .blueprint();
        assert!(t.contains("\nWrite indorsement body here.\n"));
    }

    #[test]
    fn ui_groups_cluster_fields_without_emitting_banner() {
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
        // No banners emitted at all.
        assert!(!after_quill.contains("===="));
        // Order: ungrouped first, then groups in first-appearance order.
        let notes = after_quill.find("notes:").unwrap();
        let memo_for = after_quill.find("memo_for:").unwrap();
        let letterhead = after_quill.find("letterhead_title:").unwrap();
        assert!(notes < memo_for);
        assert!(memo_for < letterhead);
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
      properties:
        org: { type: string, required: true, description: Citing organization. }
        year: { type: integer, description: Publication year. }
"#)
        .blueprint();
        assert!(t.contains("# Cited works.\nreferences:  # array<object>; optional\n  -\n"));
        assert!(t.contains("    # Citing organization.\n    org: \"\"  # string; required\n"));
        assert!(t.contains("    # Publication year.\n    year: 0  # integer; optional\n"));
    }

    #[test]
    fn typed_table_with_example_renders_example_rows_no_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      example:
        - { org: ACME, year: 2020 }
      properties:
        org: { type: string, required: true }
        year: { type: integer }
"#)
        .blueprint();
        assert!(t.contains("refs:  # array<object>; optional\n  - org: ACME\n"));
        assert!(!t.contains("refs:  # array<object>; optional\n  -\n"));
        assert!(!t.contains("# e.g."));
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
      properties:
        org: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("refs:  # array<object>; optional\n  - org: ACME\n"));
        assert!(!t.contains("refs:  # array<object>; optional\n  -\n"));
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
      properties:
        org: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains(
            "refs:  # array<object>; optional\n  -\n    org: \"\"  # string; required\n"
        ));
    }

    #[test]
    fn typed_dict_emits_per_property_annotations() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      description: Mailing address.
      properties:
        street: { type: string, required: true, description: Street line. }
        city:   { type: string, required: true }
        zip:    { type: string }
"#)
        .blueprint();
        assert!(t.contains("# Mailing address.\naddress:  # object; optional\n"));
        assert!(t.contains("  # Street line.\n  street: \"\"  # string; required\n"));
        assert!(t.contains("  city: \"\"  # string; required\n"));
        assert!(t.contains("  zip: \"\"  # string; optional\n"));
    }

    #[test]
    fn typed_dict_required_carries_role() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      required: true
      properties:
        street: { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("address:  # object; required\n"));
    }

    #[test]
    fn typed_dict_with_default_renders_block_mapping_no_annotations() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      default: { street: "5000 Forbes Ave", city: Pittsburgh }
      properties:
        street: { type: string, required: true }
        city:   { type: string, required: true }
"#)
        .blueprint();
        assert!(t.contains("address:  # object; optional\n"));
        assert!(
            t.contains("  street: 5000 Forbes Ave\n")
                || t.contains("  street: \"5000 Forbes Ave\"\n")
        );
        assert!(t.contains("  city: Pittsburgh\n"));
        // No per-property annotations when concrete values are present.
        assert!(!t.contains("# string; required"));
    }

    #[test]
    fn typed_dict_with_example_suppresses_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      example: { street: "1 Infinite Loop", city: Cupertino }
      properties:
        street: { type: string, required: true }
        city:   { type: string }
"#)
        .blueprint();
        assert!(t.contains("address:  # object; optional\n"));
        // eg comment suppressed when concrete values are rendered.
        assert!(!t.contains("# e.g."));
        assert!(t.contains("  city: Cupertino\n"));
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
leaf_kinds:
  enclosure:
    description: An enclosure attached to the letter.
    fields:
      label: { type: string, required: true }
      pages: { type: integer, default: 1 }
"#;

    #[test]
    fn blueprint_round_trips_idempotently() {
        let bp = cfg(LETTER_QUILL).blueprint();
        let doc1 = Document::from_markdown(&bp).expect("blueprint must parse");
        // The blueprint declares one leaf kind (`enclosure`). It must survive
        // parsing — earlier the leaf was emitted as `---/KIND/---`, which the
        // parser silently dropped into body prose. See LEAF_REWORK.md §3.3.
        assert_eq!(
            doc1.leaves().len(),
            1,
            "blueprint emits one leaf; parser must recognise it"
        );
        assert_eq!(doc1.leaves()[0].tag(), "enclosure");
        let md2 = doc1.to_markdown();
        let doc2 = Document::from_markdown(&md2).expect("round-tripped markdown must parse");
        assert_eq!(
            doc1, doc2,
            "Document must be equal after blueprint → parse → emit → parse"
        );
    }
}
