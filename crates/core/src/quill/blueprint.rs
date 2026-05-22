//! Auto-generated Markdown blueprint for a Quill.
//!
//! Produces an annotated reference document dense enough to replace the schema
//! for LLM consumers. The blueprint shows the document's shape — fields,
//! constraints, examples — so a consumer can write a fresh document from it.
//!
//! Every block is a `~~~card-yaml` fence: the root block declares
//! `$quill: <name>@<version>` and each composable card declares
//! `$kind: <kind>` (see `prose/references/markdown-spec.md`).
//!
//! Annotation grammar:
//! - **Leading `# …` lines** carry prose: `# <description>` (single line,
//!   whitespace-collapsed) and `# e.g. <value>` (whenever an `example:` is
//!   configured, regardless of cell or type).
//! - **Inline `# …` annotation** on the value line is structural:
//!   `# <type>[<format>][; skip-ok]`. Type is mandatory on every field.
//!   Format slot uses angle brackets (`array<string>`, `date<YYYY-MM-DD>`,
//!   `enum<a | b | c>`). The optional `; skip-ok` tag marks **Endorsed**
//!   cells whose rendered default is shippable as-is; its absence marks
//!   **Must Fill** cells, which carry the `<must-fill>` sentinel in the
//!   value cell instead.
//! - **Metadata annotation.** The `$quill` / `$kind` system-metadata lines
//!   have no inline-annotation slot, so their role annotation
//!   (`system metadata; verbatim` for the root block,
//!   `composable (0..N)` for cards) is emitted as an own-line `# …` comment
//!   directly under the `$` line.
//! - **Body regions** are signalled by `Write main body here.` after the main
//!   fence and `Write <card kind> body here.` after each card fence. When
//!   `body.example` is set, the example text is embedded verbatim instead.
//!   Absent when `body.enabled` is false.
//!
//! `ui.order` controls field ordering. `ui.group` clusters fields together
//! within the document but emits no banner.

use std::collections::BTreeMap;

use super::validation::MUST_FILL_SENTINEL;
use super::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::document::emit::{saphyr_emit_flow, saphyr_emit_scalar};
use crate::value::QuillValue;
use serde_json::Value as JsonValue;

impl QuillConfig {
    /// Generate an annotated Markdown blueprint for this quill. See module
    /// docs for the annotation grammar; the function is total over any valid
    /// `QuillConfig`.
    ///
    /// The result is guaranteed schema-valid and parseable (every key
    /// present, every value type-correct). It is *not* guaranteed to render
    /// — that is the quill authoring contract on `plate.typ`; see
    /// `prose/canon/BLUEPRINT.md` §Guarantees.
    pub fn blueprint(&self) -> String {
        let mut out = String::new();
        let main_desc = self
            .main
            .description
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| Some(self.description.as_str()).filter(|s| !s.is_empty()));
        write_main_fence(
            &mut out,
            &self.main,
            &format!("{}@{}", self.name, self.version),
            main_desc,
        );
        if self.main.body_enabled() {
            let example = self.main.body.as_ref().and_then(|b| b.example.as_deref());
            let text = example.unwrap_or("Write main body here.");
            out.push_str(&format!("\n{}\n", text));
        }
        for card in &self.card_kinds {
            out.push('\n');
            write_card_fence(&mut out, card);
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

/// Emit a whitespace-collapsed `# <text>` comment line. No-op when `text`
/// collapses to the empty string.
fn write_comment(out: &mut String, text: &str) {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !clean.is_empty() {
        out.push_str(&format!("# {}\n", clean));
    }
}

/// Emit the root block:
/// `~~~card-yaml\n$quill: …\n$kind: main\n# system metadata; …\n[# desc\n]<fields>~~~\n`.
///
/// The `$quill` system-metadata line leads the block; the role annotation
/// and the optional description follow as own-line comments.
fn write_main_fence(
    out: &mut String,
    card: &CardSchema,
    quill_ref: &str,
    description: Option<&str>,
) {
    out.push_str("~~~card-yaml\n");
    out.push_str("$quill: ");
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(
        quill_ref.to_string(),
    )));
    out.push('\n');
    out.push_str("$kind: main\n");
    write_comment(out, "system metadata; verbatim");
    if let Some(desc) = description {
        write_comment(out, desc);
    }
    write_card_fields(out, card);
    out.push_str("~~~\n");
}

/// Emit a composable card as a `~~~card-yaml` block declaring `$kind: <kind>`.
/// The `composable (0..N)` role annotation and the optional description are
/// emitted as own-line comments directly under the `$kind` header.
fn write_card_fence(out: &mut String, card: &CardSchema) {
    out.push_str("~~~card-yaml\n");
    out.push_str("$kind: ");
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(card.name.clone())));
    out.push('\n');
    out.push_str("# composable (0..N)\n");
    if let Some(desc) = &card.description {
        write_comment(out, desc);
    }
    write_card_fields(out, card);
    out.push_str("~~~\n");
}

/// Emit a card's fields, clustered by `ui.group` and ordered by `ui.order`.
fn write_card_fields(out: &mut String, card: &CardSchema) {
    for (_, fields) in group_fields(card.fields.values()) {
        for field in fields {
            write_field(out, field, 0);
        }
    }
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
        let inline = inline_annotation(field, false, field.default.is_some());
        write_markdown_block(out, field, &pad, &inline);
        return;
    }

    let inline = format!("  # {}", inline_annotation(field, false, field.default.is_some()));
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
/// Independent of cell, type, or enum-ness; examples never become rendered
/// values.
fn write_eg_comment(out: &mut String, field: &FieldSchema, pad: &str) {
    if let Some(eg) = field.example.as_ref().map(eg_hint) {
        out.push_str(&format!("{}# e.g. {}\n", pad, eg));
    }
}

fn write_markdown_block(out: &mut String, field: &FieldSchema, pad: &str, inline: &str) {
    out.push_str(&format!("{}{}: |-  # {}\n", pad, field.name, inline));
    let body_pad = format!("{}  ", pad);
    // Endorsed cell with content → render the default. Endorsed cell with
    // empty/absent string default → one indented blank line (the
    // "skippable" markdown cell). Must Fill cell → the sentinel on one
    // line inside the block scalar.
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
        Some(_) => {
            out.push_str(&format!("{}\n", body_pad));
        }
        None => {
            out.push_str(&format!("{}{}\n", body_pad, MUST_FILL_SENTINEL));
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

/// Emit a typed-table field: description + `# e.g.` line (whenever an example
/// is configured), then the field key with its `array<object>` inline
/// annotation, then either default rows or a synthetic template row. An
/// example never renders as rows — like every other field type it surfaces
/// only in the `# e.g.` leading line.
///
/// Cell rule: an Endorsed container (with a non-empty `default:`) carries
/// `; skip-ok` on the outer key and renders concrete rows without
/// per-property annotations. A Must Fill container (no default) drops
/// `; skip-ok` from the outer key (state is a leaf concern) and recurses
/// into one synthetic row whose leaves carry their own cell signals.
fn write_typed_table_field(
    out: &mut String,
    field: &FieldSchema,
    item_props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);

    let concrete_rows = field.default.as_ref().and_then(|v| match v.as_json() {
        serde_json::Value::Array(items) if !items.is_empty() => Some(items.clone()),
        _ => None,
    });

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    let inline = inline_annotation(field, true, concrete_rows.is_some());
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

/// Emit a typed-dictionary field: description + `# e.g.` line (whenever an
/// example is configured), then the field key with its `object` inline
/// annotation, then either a concrete mapping from `default` or per-property
/// annotations. An example never renders as a concrete mapping — like every
/// other field type it surfaces only in the `# e.g.` leading line.
///
/// Cell rule: an Endorsed container (with a non-empty `default:`) carries
/// `; skip-ok` on the outer key and renders a concrete block mapping
/// without per-property annotations. A Must Fill container (no default)
/// drops `; skip-ok` from the outer key (state is a leaf concern) and
/// recurses into per-property annotations whose leaves carry their own
/// cell signals.
fn write_typed_object_field(
    out: &mut String,
    field: &FieldSchema,
    props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);

    let concrete = field.default.as_ref().and_then(|v| match v.as_json() {
        serde_json::Value::Object(map) if !map.is_empty() => Some(map.clone()),
        _ => None,
    });

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    let inline = inline_annotation(field, false, concrete.is_some());
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
///
/// `force_array_object` is `true` for typed-table outer fields, which always
/// render as `array<object>`; plain arrays render as `array<string>`.
/// `endorsed` carries the cell decision: scalars use `field.default.is_some()`
/// (any default, even `""`, is shippable); containers (typed-table /
/// typed-dict) use "default present AND non-empty" so an empty default
/// still recurses into the synthetic row / per-property shape.
fn inline_annotation(field: &FieldSchema, force_array_object: bool, endorsed: bool) -> String {
    let type_expr = type_expression(field, force_array_object);
    if endorsed {
        format!("{}; skip-ok", type_expr)
    } else {
        type_expr
    }
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

/// The value to render for a field. Endorsed cells render their default;
/// Must Fill cells render the `<must-fill>` sentinel in the value cell.
enum FieldValue {
    Inline(String),
    Block(Vec<serde_json::Value>),
}

fn field_value(field: &FieldSchema) -> FieldValue {
    if let Some(v) = field.default.as_ref() {
        return json_to_value(v.as_json());
    }
    // No default → Must Fill cell. The sentinel sits in the value cell
    // regardless of declared type (markdown is special-cased earlier and
    // never reaches this code path).
    FieldValue::Inline(MUST_FILL_SENTINEL.to_string())
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

/// Format an example value as a compact one-line hint. Arrays and objects
/// render as YAML flow collections (`[a, b, c]`, `{k: v}`) so multi-element
/// shape information is preserved without expanding into multiple comment
/// lines.
fn eg_hint(example: &QuillValue) -> String {
    match example.as_json() {
        v @ (serde_json::Value::Array(_) | serde_json::Value::Object(_)) => saphyr_emit_flow(v),
        val => render_scalar(val),
    }
}

/// Emit a scalar (or fallback container) as a single-line YAML value
/// using saphyr's quoting heuristics. Saphyr decides between plain
/// (`hello`, `42`), double-quoted (`"on"`, `"01234"`), and single-quoted
/// forms based on whether the unquoted form would re-parse to the same
/// `QuillValue`.
fn render_scalar(val: &serde_json::Value) -> String {
    saphyr_emit_scalar(val)
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;
    use crate::Document;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid yaml")
    }

    #[test]
    fn must_fill_string_renders_sentinel_with_no_skip_ok() {
        // No `default:` → Must Fill. Sentinel sits in the value cell; the
        // inline annotation drops `; skip-ok`.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    author: { type: string }
"#)
        .blueprint();
        assert!(t.contains("author: <must-fill>  # string\n"));
    }

    #[test]
    fn endorsed_field_with_example_does_not_use_example_as_value() {
        // Examples never render as values — they always surface in `# e.g.`.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, default: draft, example: final }
"#)
        .blueprint();
        assert!(t.contains("# e.g. final\nstatus: draft  # string; skip-ok\n"));
    }

    #[test]
    fn endorsed_empty_default_renders_value_with_skip_ok_and_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: string, default: "", example: CONFIDENTIAL }
"#)
        .blueprint();
        assert!(t.contains("# e.g. CONFIDENTIAL\nclassification: \"\"  # string; skip-ok\n"));
    }

    #[test]
    fn must_fill_array_example_renders_as_flow_sequence_with_context_quoting() {
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
            "# e.g. [Mr. John Doe, 123 Main St, \"Anytown, USA\"]\nrecipient: <must-fill>  # array<string>\n"
        ));
    }

    #[test]
    fn enum_endorsed_uses_enum_format_slot_and_no_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    format: { type: string, enum: [standard, informal], default: standard }
"#)
        .blueprint();
        assert!(t.contains("format: standard  # enum<standard | informal>; skip-ok\n"));
        assert!(!t.contains("e.g."));
    }

    #[test]
    fn enum_must_fill_renders_sentinel_in_value_cell() {
        // An enum field with no `default:` renders `<must-fill>` rather than
        // the first enum value — the cell is Must Fill regardless.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    severity: { type: string, enum: [low, medium, high] }
"#)
        .blueprint();
        assert!(t.contains("severity: <must-fill>  # enum<low | medium | high>\n"));
    }

    #[test]
    fn must_fill_array_with_example_renders_eg_only_not_value() {
        // Plain (non-typed-table) Must Fill arrays render the sentinel; the
        // example surfaces in the leading `# e.g.` line.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    memo_from:
      type: array
      example:
        - ORG/SYMBOL
        - City ST 12345
"#)
        .blueprint();
        assert!(t.contains(
            "# e.g. [ORG/SYMBOL, City ST 12345]\nmemo_from: <must-fill>  # array<string>\n"
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
      description: Be brief and clear.
"#)
        .blueprint();
        assert!(t.contains("# Be brief and clear.\nsubject: <must-fill>  # string\n"));
    }

    #[test]
    fn every_field_carries_inline_type_and_cell_signal() {
        // Endorsed cells carry `; skip-ok`; Must Fill cells carry the
        // sentinel in the value cell and drop the tag.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    issued: { type: date }
    published: { type: datetime }
    refs: { type: array, default: [] }
"#)
        .blueprint();
        assert!(t.contains("title: <must-fill>  # string\n"));
        assert!(t.contains("size: 11  # number; skip-ok\n"));
        assert!(t.contains("flag: false  # boolean; skip-ok\n"));
        assert!(t.contains("issued: <must-fill>  # date<YYYY-MM-DD>\n"));
        assert!(t.contains("published: <must-fill>  # datetime<ISO 8601>\n"));
        assert!(t.contains("refs: []  # array<string>; skip-ok\n"));
    }

    #[test]
    fn must_fill_markdown_renders_sentinel_inside_block_scalar() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown }
"#)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown\n  <must-fill>\n"));
    }

    #[test]
    fn endorsed_empty_markdown_renders_blank_line_with_skip_ok() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown, default: "" }
"#)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown; skip-ok\n  \n"));
        assert!(!t.contains("<must-fill>"));
    }

    #[test]
    fn endorsed_markdown_default_fills_block() {
        let t = cfg(r###"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio:
      type: markdown
      default: "## About me\n\nHello."
"###)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown; skip-ok\n  ## About me\n  \n  Hello.\n"));
    }

    #[test]
    fn quill_metadata_line_is_verbatim() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        assert!(t.starts_with(
            "~~~card-yaml\n$quill: taro@0.1.0\n$kind: main\n# system metadata; verbatim\n# x\n"
        ));
        assert!(t.contains("\nWrite main body here.\n"));
    }

    #[test]
    fn card_fence_carries_composable_annotation() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  note:
    description: A short note appended to the document.
    fields:
      author: { type: string }
"#)
        .blueprint();
        assert!(t.contains(
            "~~~card-yaml\n$kind: note\n# composable (0..N)\n# A short note appended to the document.\n"
        ));
    }

    #[test]
    fn body_disabled_card_omits_body_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  skills:
    body: { enabled: false }
    fields:
      items: { type: array }
"#)
        .blueprint();
        let after = &t[t.find("$kind: skills").unwrap()..];
        assert!(!after.contains("skills body"));
    }

    #[test]
    fn body_example_appears_verbatim() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  note:
    body:
      example: "This is an example note."
    fields:
      author: { type: string }
"#)
        .blueprint();
        let after = &t[t.find("$kind: note").unwrap()..];
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
    fn card_body_placeholder_uses_card_name() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
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
    memo_for: { type: array, ui: { group: Addressing } }
    subject: { type: string, ui: { group: Addressing } }
    letterhead_title: { type: string, default: HQ, ui: { group: Letterhead } }
    notes: { type: string }
"#)
        .blueprint();
        let after_quill = &t[t.find("$quill:").unwrap()..];
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
    fn typed_table_must_fill_emits_synthetic_row_with_leaf_sentinels() {
        // Must Fill container → outer key has no `; skip-ok` (state is a
        // leaf concern). Property leaves carry their own cell signals.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    references:
      type: array
      description: Cited works.
      properties:
        org: { type: string, description: Citing organization. }
        year: { type: integer, default: 0, description: Publication year. }
"#)
        .blueprint();
        assert!(t.contains("# Cited works.\nreferences:  # array<object>\n  -\n"));
        assert!(t.contains("    # Citing organization.\n    org: <must-fill>  # string\n"));
        assert!(t.contains("    # Publication year.\n    year: 0  # integer; skip-ok\n"));
    }

    #[test]
    fn typed_table_with_example_keeps_eg_line_and_synthetic_row() {
        // Examples never render as rows — they surface only in `# e.g.`,
        // consistent with every other field type.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      example:
        - { org: ACME, year: 2020 }
      properties:
        org: { type: string }
        year: { type: integer, default: 0 }
"#)
        .blueprint();
        assert!(t.contains("# e.g. [{org: ACME, year: 2020}]\n"));
        assert!(t.contains("refs:  # array<object>\n  -\n"));
        assert!(t.contains("    org: <must-fill>  # string\n"));
        assert!(t.contains("    year: 0  # integer; skip-ok\n"));
    }

    #[test]
    fn typed_table_endorsed_renders_default_rows_with_skip_ok() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default:
        - { org: ACME }
      properties:
        org: { type: string }
"#)
        .blueprint();
        assert!(t.contains("refs:  # array<object>; skip-ok\n  - org: ACME\n"));
        assert!(!t.contains("refs:  # array<object>; skip-ok\n  -\n"));
    }

    #[test]
    fn typed_table_with_empty_default_falls_through_to_synthetic_row() {
        // An empty default array is treated like a Must Fill container
        // for rendering purposes (we need a synthetic row to show the
        // shape), and the outer key drops `; skip-ok`.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default: []
      properties:
        org: { type: string }
"#)
        .blueprint();
        assert!(t.contains(
            "refs:  # array<object>\n  -\n    org: <must-fill>  # string\n"
        ));
    }

    #[test]
    fn typed_dict_must_fill_emits_per_property_annotations() {
        // Must Fill container → outer key has no `; skip-ok`; per-property
        // recursion with leaf cell signals.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      description: Mailing address.
      properties:
        street: { type: string, description: Street line. }
        city:   { type: string }
        zip:    { type: string, default: "" }
"#)
        .blueprint();
        assert!(t.contains("# Mailing address.\naddress:  # object\n"));
        assert!(t.contains("  # Street line.\n  street: <must-fill>  # string\n"));
        assert!(t.contains("  city: <must-fill>  # string\n"));
        assert!(t.contains("  zip: \"\"  # string; skip-ok\n"));
    }

    #[test]
    fn typed_dict_endorsed_renders_block_mapping_with_skip_ok() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      default: { street: "5000 Forbes Ave", city: Pittsburgh }
      properties:
        street: { type: string }
        city:   { type: string }
"#)
        .blueprint();
        assert!(t.contains("address:  # object; skip-ok\n"));
        assert!(
            t.contains("  street: 5000 Forbes Ave\n")
                || t.contains("  street: \"5000 Forbes Ave\"\n")
        );
        assert!(t.contains("  city: Pittsburgh\n"));
        // No per-property annotations when concrete values are present.
        assert!(!t.contains("# string"));
    }

    #[test]
    fn typed_dict_with_example_keeps_eg_line_and_per_property() {
        // Examples never render as a concrete mapping — they surface only in
        // `# e.g.`, consistent with every other field type.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      example: { street: "1 Infinite Loop", city: Cupertino }
      properties:
        street: { type: string }
        city:   { type: string, default: "" }
"#)
        .blueprint();
        assert!(t.contains("address:  # object\n"));
        assert!(
            t.contains("# e.g. {street: 1 Infinite Loop, city: Cupertino}\n")
                || t.contains("# e.g. {city: Cupertino, street: 1 Infinite Loop}\n")
        );
        assert!(t.contains("  street: <must-fill>  # string\n"));
        assert!(t.contains("  city: \"\"  # string; skip-ok\n"));
    }

    const LETTER_QUILL: &str = r#"
quill: { name: letter, version: 1.0.0, backend: typst, description: A formal letter. }
main:
  fields:
    to:
      type: string
      description: Recipient name.
    subject:
      type: string
    date:
      type: date
    priority:
      type: string
      enum: [normal, urgent]
      default: normal
    attachments:
      type: array
      default: []
      example:
        - report.pdf
card_kinds:
  enclosure:
    description: An enclosure attached to the letter.
    fields:
      label: { type: string }
      pages: { type: integer, default: 1 }
"#;

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

    /// Regression: string defaults that look numeric/boolean/null get
    /// quoted so the schema-validated payload still types as `string`
    /// after round-trip. Pre-saphyr, defaults like `1.0`, `on`, `01234`,
    /// or `null` were emitted bare and re-parsed as the wrong YAML type.
    #[test]
    fn type_ambiguous_string_defaults_round_trip_as_strings() {
        let bp = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    version:     { type: string, default: "1.0" }
    activation:  { type: string, default: "on" }
    code:        { type: string, default: "01234" }
    placeholder: { type: string, default: "null" }
    yes_flag:    { type: string, default: "yes" }
"#)
        .blueprint();

        let doc = Document::from_markdown(&bp).expect("blueprint must parse");
        let payload = doc.main().payload();
        for (key, expected) in [
            ("version", "1.0"),
            ("activation", "on"),
            ("code", "01234"),
            ("placeholder", "null"),
            ("yes_flag", "yes"),
        ] {
            let v = payload.get(key).unwrap_or_else(|| panic!("missing {key}"));
            assert!(
                v.as_str().is_some(),
                "field {key} must round-trip as a string, got {:?}\nBlueprint:\n{}",
                v,
                bp
            );
            assert_eq!(v.as_str().unwrap(), expected, "field {key}: value mismatch");
        }
    }
}
