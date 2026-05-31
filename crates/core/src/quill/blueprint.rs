//! Auto-generated Markdown blueprint for a Quill.
//!
//! Produces an annotated reference document dense enough to replace the schema
//! for LLM consumers. The blueprint shows the document's shape — fields,
//! constraints, examples — so a consumer can write a fresh document from it.
//!
//! Every block is a `~~~` card-yaml fence: the root block declares
//! `$quill: <name>@<version>` and each composable card declares
//! `$kind: <kind>` (see `prose/references/markdown-spec.md`).
//!
//! Annotation grammar:
//! - **Leading `# …` lines** carry prose: `# <description>` (single line,
//!   whitespace-collapsed) and `# e.g. <value>` (whenever an `example:` is
//!   configured, regardless of cell or type).
//! - **Inline `# …` annotation** on the value line is structural:
//!   `# <type>[<format>][; delete-ok]`. Type is mandatory on every field.
//!   Format slot uses angle brackets (`array<string>`, `date<YYYY-MM-DD>`,
//!   `enum<a | b | c>`). The optional `; delete-ok` tag marks **Endorsed**
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
use super::{zero_value, CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::document::emit::{saphyr_emit_flow, saphyr_emit_scalar};
use crate::value::QuillValue;
use serde_json::Value as JsonValue;

/// Internal strategy controlling how each field's value cell is rendered in a
/// generated reference document. The value is chosen at generation time from
/// the typed schema, so the document never re-parses the annotation grammar.
///
/// Two variants for the two named reference documents — no configurable
/// precedence policy (see `prose/canon/BLUEPRINT.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FillSource {
    /// Render the `<must-fill>` sentinel in every Must Fill cell (a field with
    /// no `default:`) — downstream validation reports
    /// `validation::must_fill_sentinel`. The canonical authoring blueprint
    /// produced by [`QuillConfig::blueprint`].
    Sentinel,
    /// Fill each cell with its illustrative value: `example:` › `default:` ›
    /// the type-empty zero value. The `example` reference document produced by
    /// [`QuillConfig::example`].
    Example,
}

/// The value source for a field's rendered cell, by precedence:
///
/// - [`FillSource::Sentinel`] (blueprint): the endorsed `default:` only; a
///   Must Fill cell falls through to the `<must-fill>` sentinel.
/// - [`FillSource::Example`] (example document): the illustrative `example:`
///   wins, then the `default:`; a cell with neither falls through to the
///   type-empty zero value.
///
/// `None` means the cell falls back to sentinel / zero rendering in the
/// caller. Endorsement (the `; delete-ok` tag) keys off `field.default` alone,
/// never this source.
fn fill_source(field: &FieldSchema, source: FillSource) -> Option<&QuillValue> {
    match source {
        FillSource::Sentinel => field.default.as_ref(),
        FillSource::Example => field.example.as_ref().or(field.default.as_ref()),
    }
}

impl QuillConfig {
    /// Generate the canonical annotated Markdown blueprint for this quill —
    /// the authoring surface handed to LLMs and humans, with Must Fill cells
    /// carrying the `<must-fill>` sentinel. Its illustrative counterpart is
    /// [`example`](Self::example). See module docs for the annotation grammar;
    /// the function is total over any valid `QuillConfig`.
    ///
    /// The result is guaranteed schema-valid and parseable (every key
    /// present, every value type-correct). It is *not* guaranteed to render
    /// — that is the quill authoring contract on `plate.typ`; see
    /// `prose/canon/BLUEPRINT.md` §Guarantees.
    pub fn blueprint(&self) -> String {
        self.render_blueprint(FillSource::Sentinel)
    }

    /// Generate the `example` reference document for this quill — the
    /// illustrative consolidation "show me a filled-out one." Each field
    /// renders its `example:`, else its `default:`, else the type-empty zero
    /// value, with no `<must-fill>` sentinels. The document is example-*first*
    /// but not guaranteed fully populated: a field with neither an `example:`
    /// nor a `default:` renders at its zero value.
    ///
    /// See `prose/canon/BLUEPRINT.md`. Like
    /// [`blueprint`](Self::blueprint), the result is schema-valid and
    /// parseable; rendering is the quill authoring contract on `plate.typ`.
    pub fn example(&self) -> String {
        self.render_blueprint(FillSource::Example)
    }

    fn render_blueprint(&self, source: FillSource) -> String {
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
            source,
        );
        if self.main.body_enabled() {
            let example = self.main.body.as_ref().and_then(|b| b.example.as_deref());
            let text = example.unwrap_or("Write main body here.");
            out.push_str(&format!("\n{}\n", text));
        }
        for card in &self.card_kinds {
            out.push('\n');
            write_card_fence(&mut out, card, source);
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
/// `~~~\n$quill: …\n$kind: main\n# system metadata; …\n[# desc\n]<fields>~~~\n`.
///
/// The `$quill` system-metadata line leads the block; the role annotation
/// and the optional description follow as own-line comments.
fn write_main_fence(
    out: &mut String,
    card: &CardSchema,
    quill_ref: &str,
    description: Option<&str>,
    source: FillSource,
) {
    out.push_str("~~~\n");
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
    write_card_fields(out, card, source);
    out.push_str("~~~\n");
}

/// Emit a composable card as a `~~~` block declaring `$kind: <kind>`.
/// The `composable (0..N)` role annotation and the optional description are
/// emitted as own-line comments directly under the `$kind` header.
fn write_card_fence(out: &mut String, card: &CardSchema, source: FillSource) {
    out.push_str("~~~\n");
    out.push_str("$kind: ");
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(card.name.clone())));
    out.push('\n');
    out.push_str("# composable (0..N)\n");
    if let Some(desc) = &card.description {
        write_comment(out, desc);
    }
    write_card_fields(out, card, source);
    out.push_str("~~~\n");
}

/// Emit a card's fields, clustered by `ui.group` and ordered by `ui.order`.
fn write_card_fields(out: &mut String, card: &CardSchema, source: FillSource) {
    for (_, fields) in group_fields(card.fields.values()) {
        for field in fields {
            write_field(out, field, 0, source);
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

fn write_field(out: &mut String, field: &FieldSchema, indent: usize, source: FillSource) {
    let pad = "  ".repeat(indent);

    // Typed table: an array whose element is an object with properties.
    // Scalar-element arrays (`string[]`, `integer[]`, `markdown[]`, …) fall
    // through to the uniform scalar rendering below.
    if matches!(field.r#type, FieldType::Array) {
        if let Some(items) = &field.items {
            if matches!(items.r#type, FieldType::Object) {
                if let Some(props) = &items.properties {
                    write_typed_table_field(out, field, props, indent, source);
                    return;
                }
            }
        }
    }

    // Typed dictionary: standalone object with defined properties.
    if matches!(field.r#type, FieldType::Object) {
        if let Some(props) = &field.properties {
            write_typed_object_field(out, field, props, indent, source);
            return;
        }
    }

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    // Markdown fields render as a YAML block scalar so multi-line content has
    // a consistent shape regardless of whether a default is configured.
    if matches!(field.r#type, FieldType::Markdown) {
        let inline = inline_annotation(field, field.default.is_some());
        write_markdown_block(out, field, &pad, &inline, source);
        return;
    }

    let inline = format!("  # {}", inline_annotation(field, field.default.is_some()));
    let value = field_value(field, source);
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

fn write_markdown_block(
    out: &mut String,
    field: &FieldSchema,
    pad: &str,
    inline: &str,
    source: FillSource,
) {
    out.push_str(&format!("{}{}: |-  # {}\n", pad, field.name, inline));
    let body_pad = format!("{}  ", pad);
    // Endorsed cell with content → render the default. Under `Preview`, a
    // Must Fill cell with an `example:` renders the example verbatim. Endorsed
    // cell with empty/absent string default (or empty example) → one indented
    // blank line (the "skippable" markdown cell). Otherwise a Must Fill cell
    // gets the sentinel (Strict) or an empty body line (Preview/TypeEmpty).
    let content = fill_source(field, source).and_then(|v| match v.as_json() {
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
            let body = match source {
                FillSource::Sentinel => MUST_FILL_SENTINEL,
                FillSource::Example => "",
            };
            out.push_str(&format!("{}{}\n", body_pad, body));
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

/// Emit a typed-table field: description + `# e.g.` line (whenever an
/// example is configured), then the field key with its `array<object>`
/// inline annotation, then the rendered rows. Rows come from [`fill_source`]
/// (the `example:` › `default:` for the example document, the `default:` for
/// the blueprint), or a synthetic template row otherwise.
///
/// Cell rule (uniform with scalars): a field with a `default:` is Endorsed
/// — the outer key carries `; delete-ok`. A field without a `default:` is
/// Must Fill — the outer key drops `; delete-ok`. When [`fill_source`] yields
/// no value, the blueprint emits one synthetic row with leaf-level sentinels
/// and the example document recurses to type-empty leaves. See
/// `prose/BOOKMARKS.md` "Typed container empty default loses inline shape
/// documentation" for the rendering-vs-symmetry trade-off.
fn write_typed_table_field(
    out: &mut String,
    field: &FieldSchema,
    item_props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
    source: FillSource,
) {
    let pad = "  ".repeat(indent);

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    // Endorsement keys off `default:` alone; the rendered rows come from the
    // default or — under `Preview` — the `example:`.
    let inline = inline_annotation(field, field.default.is_some());
    let rows = fill_source(field, source).and_then(|v| match v.as_json() {
        serde_json::Value::Array(items) => Some(items.clone()),
        _ => None,
    });

    match rows {
        Some(items) if items.is_empty() => {
            out.push_str(&format!("{}{}: []  # {}\n", pad, field.name, inline));
        }
        Some(items) => {
            out.push_str(&format!("{}{}:  # {}\n", pad, field.name, inline));
            write_array_items(out, &items, &pad);
        }
        None if item_props.is_empty() => {
            // Row type declares no properties: emit an empty (type-valid)
            // array rather than a null synthetic row.
            out.push_str(&format!("{}{}: []  # {}\n", pad, field.name, inline));
        }
        None => {
            out.push_str(&format!("{}{}:  # {}\n", pad, field.name, inline));
            let dash_pad = "  ".repeat(indent + 1);
            out.push_str(&format!("{}-\n", dash_pad));
            for prop in sort_props(item_props) {
                write_field(out, prop, indent + 2, source);
            }
        }
    }
}

/// Emit a typed-dictionary field: description + `# e.g.` line (whenever an
/// example is configured), then the field key with its `object` inline
/// annotation, then the rendered mapping. The mapping comes from
/// [`fill_source`] (the `example:` › `default:` for the example document, the
/// `default:` for the blueprint), or per-property annotations otherwise.
///
/// Cell rule (uniform with scalars): a field with a `default:` is Endorsed
/// — the outer key carries `; delete-ok` and the rendered value is the
/// resolved mapping (a block mapping for non-empty, inline `{}` for `{}`).
/// A field without a `default:` is Must Fill — when [`fill_source`] yields no
/// value, the blueprint recurses to per-property leaf-level sentinels and the
/// example document to type-empty leaves. See `prose/BOOKMARKS.md` "Typed
/// container empty default loses inline shape documentation" for the trade-off.
fn write_typed_object_field(
    out: &mut String,
    field: &FieldSchema,
    props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
    source: FillSource,
) {
    let pad = "  ".repeat(indent);

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    // Endorsement keys off `default:` alone; the rendered mapping comes from
    // the default or — under `Preview` — the `example:`.
    let inline = inline_annotation(field, field.default.is_some());
    let mapping = fill_source(field, source).and_then(|v| match v.as_json() {
        serde_json::Value::Object(map) => Some(map.clone()),
        _ => None,
    });

    match mapping {
        Some(map) if map.is_empty() => {
            out.push_str(&format!("{}{}: {{}}  # {}\n", pad, field.name, inline));
        }
        Some(map) => {
            out.push_str(&format!("{}{}:  # {}\n", pad, field.name, inline));
            let inner_pad = format!("{}  ", pad);
            for (k, v) in &map {
                out.push_str(&format!("{}{}: {}\n", inner_pad, k, render_scalar(v)));
            }
        }
        None if props.is_empty() => {
            // Empty typed object: no leaves to fill, so the value cell is an
            // empty (type-valid) object rather than a bare null.
            out.push_str(&format!("{}{}: {{}}  # {}\n", pad, field.name, inline));
        }
        None => {
            out.push_str(&format!("{}{}:  # {}\n", pad, field.name, inline));
            for prop in sort_props(props) {
                write_field(out, prop, indent + 1, source);
            }
        }
    }
}

/// Build the inline annotation body (without the leading `# `).
///
/// `force_array_object` is `true` for typed-table outer fields, which
/// always render as `array<object>`; plain arrays render as `array<string>`.
/// `endorsed` is uniformly `field.default.is_some()` — any `default:`
/// (including type-empty `""`, `[]`, `{}`) means the cell is shippable.
fn inline_annotation(field: &FieldSchema, endorsed: bool) -> String {
    let type_expr = type_expression(field);
    if endorsed {
        format!("{}; delete-ok", type_expr)
    } else {
        type_expr
    }
}

fn type_expression(field: &FieldSchema) -> String {
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
        // The element type comes from `items`; a scalar element gives
        // `array<string>`/`array<integer>`/`array<markdown>`, an object
        // element gives `array<object>`.
        FieldType::Array => {
            let item = field
                .items
                .as_ref()
                .map(|it| type_expression(it))
                .unwrap_or_else(|| "string".into());
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

fn field_value(field: &FieldSchema, source: FillSource) -> FieldValue {
    // Endorsed/illustrative value: the `default:` (blueprint), or `example:` ›
    // `default:` (example document), when present.
    if let Some(v) = fill_source(field, source) {
        return json_to_value(v.as_json());
    }
    // Otherwise the cell falls through: the `<must-fill>` sentinel (blueprint)
    // or the type-empty zero value (example document). Markdown is
    // special-cased in `write_markdown_block` and never reaches this path.
    match source {
        FillSource::Sentinel => FieldValue::Inline(MUST_FILL_SENTINEL.to_string()),
        FillSource::Example => json_to_value(zero_value(field).as_json()),
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
    fn must_fill_string_renders_sentinel_with_no_delete_ok() {
        // No `default:` → Must Fill. Sentinel sits in the value cell; the
        // inline annotation drops `; delete-ok`.
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
        assert!(t.contains("# e.g. final\nstatus: draft  # string; delete-ok\n"));
    }

    #[test]
    fn endorsed_empty_default_renders_value_with_delete_ok_and_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: string, default: "", example: CONFIDENTIAL }
"#)
        .blueprint();
        assert!(t.contains("# e.g. CONFIDENTIAL\nclassification: \"\"  # string; delete-ok\n"));
    }

    #[test]
    fn must_fill_array_example_renders_as_flow_sequence_with_context_quoting() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    recipient:
      type: array
      items: { type: string }
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
        assert!(t.contains("format: standard  # enum<standard | informal>; delete-ok\n"));
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
      items: { type: string }
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
        // Endorsed cells carry `; delete-ok`; Must Fill cells carry the
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
    refs: { type: array, default: [], items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("title: <must-fill>  # string\n"));
        assert!(t.contains("size: 11  # number; delete-ok\n"));
        assert!(t.contains("flag: false  # boolean; delete-ok\n"));
        assert!(t.contains("issued: <must-fill>  # date<YYYY-MM-DD>\n"));
        assert!(t.contains("published: <must-fill>  # datetime<ISO 8601>\n"));
        assert!(t.contains("refs: []  # array<string>; delete-ok\n"));
    }

    #[test]
    fn scalar_array_annotation_reflects_element_type() {
        // The element type drives the format slot: `array<integer>`,
        // `array<markdown>`, … rather than a hardcoded `array<string>`.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    counts:   { type: array, items: { type: integer } }
    sections: { type: array, items: { type: markdown } }
    tags:     { type: array, items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("counts: <must-fill>  # array<integer>\n"), "{t}");
        assert!(t.contains("sections: <must-fill>  # array<markdown>\n"), "{t}");
        assert!(t.contains("tags: <must-fill>  # array<string>\n"), "{t}");
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
    fn endorsed_empty_markdown_renders_blank_line_with_delete_ok() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown, default: "" }
"#)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown; delete-ok\n  \n"));
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
        assert!(t.contains("bio: |-  # markdown; delete-ok\n  ## About me\n  \n  Hello.\n"));
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
            "~~~\n$quill: taro@0.1.0\n$kind: main\n# system metadata; verbatim\n# x\n"
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
            "~~~\n$kind: note\n# composable (0..N)\n# A short note appended to the document.\n"
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
      items: { type: array, items: { type: string } }
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
    memo_for: { type: array, items: { type: string }, ui: { group: Addressing } }
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
        // Must Fill container → outer key has no `; delete-ok` (state is a
        // leaf concern). Property leaves carry their own cell signals.
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
          org: { type: string, description: Citing organization. }
          year: { type: integer, default: 0, description: Publication year. }
"#)
        .blueprint();
        assert!(t.contains("# Cited works.\nreferences:  # array<object>\n  -\n"));
        assert!(t.contains("    # Citing organization.\n    org: <must-fill>  # string\n"));
        assert!(t.contains("    # Publication year.\n    year: 0  # integer; delete-ok\n"));
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
      items:
        type: object
        properties:
          org: { type: string }
          year: { type: integer, default: 0 }
"#)
        .blueprint();
        assert!(t.contains("# e.g. [{org: ACME, year: 2020}]\n"));
        assert!(t.contains("refs:  # array<object>\n  -\n"));
        assert!(t.contains("    org: <must-fill>  # string\n"));
        assert!(t.contains("    year: 0  # integer; delete-ok\n"));
    }

    #[test]
    fn typed_table_endorsed_renders_default_rows_with_delete_ok() {
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
          org: { type: string }
"#)
        .blueprint();
        assert!(t.contains("refs:  # array<object>; delete-ok\n  - org: ACME\n"));
        assert!(!t.contains("refs:  # array<object>; delete-ok\n  -\n"));
    }

    #[test]
    fn typed_table_with_empty_default_renders_inline_and_delete_ok() {
        // `default: []` means shippable as-is — the outer cell carries
        // `; delete-ok` uniformly with scalar cells and the value renders
        // inline as `[]`. Inline row shape under an empty default belongs
        // in `example:`; see prose/BOOKMARKS.md.
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
          org: { type: string }
"#)
        .blueprint();
        assert!(
            t.contains("refs: []  # array<object>; delete-ok\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("<must-fill>"), "no sentinels expected: {t}");
    }

    #[test]
    fn typed_dict_with_empty_default_renders_inline_and_delete_ok() {
        // Same uniform rule as typed tables: `default: {}` is Endorsed and
        // renders inline as `{}` with `; delete-ok`.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      default: {}
      properties:
        street: { type: string }
"#)
        .blueprint();
        assert!(
            t.contains("address: {}  # object; delete-ok\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("<must-fill>"), "no sentinels expected: {t}");
    }

    #[test]
    fn typed_dict_must_fill_emits_per_property_annotations() {
        // Must Fill container → outer key has no `; delete-ok`; per-property
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
        assert!(t.contains("  zip: \"\"  # string; delete-ok\n"));
    }

    #[test]
    fn typed_dict_endorsed_renders_block_mapping_with_delete_ok() {
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
        assert!(t.contains("address:  # object; delete-ok\n"));
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
        assert!(t.contains("  city: \"\"  # string; delete-ok\n"));
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
      items: { type: string }
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

    #[test]
    fn example_prioritizes_example_over_default() {
        // A field with BOTH a `default:` and an `example:`: the example
        // document renders the illustrative example (example › default), while
        // the blueprint renders the default. The `; delete-ok` tag keys off
        // the default's presence in both.
        let config = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, default: draft, example: final }
"#);
        let example = config.example();
        assert!(
            example.contains("status: final  # string; delete-ok\n"),
            "example should render the example value (example › default): {example}"
        );
        let blueprint = config.blueprint();
        assert!(
            blueprint.contains("status: draft  # string; delete-ok\n"),
            "blueprint should render the default value: {blueprint}"
        );
    }

    #[test]
    fn example_without_examples_falls_back_to_type_empty() {
        // No `example:` and no `default:` on any field → the example document
        // renders the leanest type-valid (zero) value.
        let out = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title:     { type: string }
    count:     { type: integer }
    ratio:     { type: number }
    flag:      { type: boolean }
    refs:      { type: array, items: { type: string } }
    issued:    { type: date }
    published: { type: datetime }
    severity:  { type: string, enum: [low, medium, high] }
    bio:       { type: markdown }
    addr:
      type: object
      properties:
        street: { type: string }
        zip:    { type: integer }
    rows:
      type: array
      items:
        type: object
        properties:
          org:  { type: string }
          year: { type: integer }
"#)
        .example();
        assert!(!out.contains("<must-fill>"), "no sentinels expected: {out}");
        assert!(!out.contains("Lorem ipsum"), "no stub strings expected: {out}");
        assert!(out.contains("title: \"\"  # string\n"));
        assert!(out.contains("count: 0  # integer\n"));
        assert!(out.contains("ratio: 0  # number\n"));
        assert!(out.contains("flag: false  # boolean\n"));
        assert!(out.contains("refs: []  # array<string>\n"));
        assert!(out.contains("issued: \"\"  # date<YYYY-MM-DD>\n"));
        assert!(out.contains("published: \"\"  # datetime<ISO 8601>\n"));
        assert!(out.contains("severity: low  # enum<low | medium | high>\n"));
        assert!(out.contains("bio: |-  # markdown\n  \n"));
        // Typed object / table leaves fall through to type-empty too.
        // (Object properties indent one level; typed-table rows two.)
        assert!(out.contains("  street: \"\"  # string\n"), "{out}");
        assert!(out.contains("    org: \"\"  # string\n"), "{out}");
    }

    #[test]
    fn example_without_examples_validates_clean() {
        // The example document (zero-filled, no examples) must be schema-valid,
        // not merely parseable. Exercises every scalar Must Fill type plus the
        // nested leaves of a typed dictionary and a typed table.
        let config = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title:    { type: string }
    count:    { type: integer }
    flag:     { type: boolean }
    refs:     { type: array, items: { type: string } }
    issued:   { type: date }
    severity: { type: string, enum: [low, medium, high] }
    bio:      { type: markdown }
    addr:
      type: object
      properties:
        street: { type: string }
        zip:    { type: integer }
    rows:
      type: array
      items:
        type: object
        properties:
          org:  { type: string }
          year: { type: integer }
"#);
        let filled = config.example();
        let doc = Document::from_markdown(&filled)
            .unwrap_or_else(|e| panic!("example document must parse: {e:?}\n---\n{filled}"));
        config.validate_document(&doc).unwrap_or_else(|errs| {
            panic!("example document must validate: {errs:?}\n---\n{filled}")
        });
    }

    #[test]
    fn example_fills_must_fill_cells_from_examples() {
        // Each Must Fill field carries an `example:`; the example document
        // renders it as the value cell (without `; delete-ok`, not endorsed).
        let out = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title:    { type: string, example: Quarterly Report }
    count:    { type: integer, example: 7 }
    issued:   { type: date, example: "2030-06-01" }
    refs:     { type: array, items: { type: string }, example: [alpha, beta] }
"#)
        .example();
        assert!(!out.contains("<must-fill>"), "no sentinels expected: {out}");
        assert!(out.contains("title: Quarterly Report  # string\n"), "{out}");
        assert!(out.contains("count: 7  # integer\n"), "{out}");
        assert!(out.contains("issued: 2030-06-01  # date<YYYY-MM-DD>\n"), "{out}");
        assert!(out.contains("refs:  # array<string>\n  - alpha\n  - beta\n"), "{out}");
    }

    #[test]
    fn example_markdown_block_scalar() {
        // No example → empty body (zero fallback). With an example → the
        // example text, rendered verbatim inside the block scalar.
        let bare = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown }
"#)
        .example();
        assert!(!bare.contains("<must-fill>"), "no sentinels expected: {bare}");
        assert!(bare.contains("bio: |-  # markdown\n  \n"), "{bare}");

        let with_eg = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown, example: "Hello there." }
"#)
        .example();
        assert!(with_eg.contains("bio: |-  # markdown\n  Hello there.\n"), "{with_eg}");
    }

    #[test]
    fn example_renders_endorsed_default_when_no_example() {
        // Fields with a `default:` and no `example:` → the example document
        // renders the default (example › default, example absent).
        let out = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    size:   { type: number, default: 11 }
    flag:   { type: boolean, default: true }
    status: { type: string, default: draft }
"#)
        .example();
        assert!(out.contains("size: 11  # number; delete-ok\n"));
        assert!(out.contains("flag: true  # boolean; delete-ok\n"));
        assert!(out.contains("status: draft  # string; delete-ok\n"));
    }

    #[test]
    fn example_substitutes_typed_table_leaves() {
        // No field-level example and no leaf examples → a synthetic row with
        // type-empty leaves.
        let out = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      items:
        type: object
        properties:
          org:  { type: string }
          year: { type: integer }
"#)
        .example();
        assert!(!out.contains("<must-fill>"), "no sentinels expected: {out}");
        assert!(out.contains("refs:  # array<object>\n  -\n"), "{out}");
        assert!(out.contains("    org: \"\"  # string\n"), "{out}");
        assert!(out.contains("    year: 0  # integer\n"), "{out}");
    }

    #[test]
    fn example_renders_typed_table_field_example_rows() {
        // A field-level `example:` on a Must Fill typed table renders as rows
        // in the example document (no `; delete-ok`, since there is no default).
        let out = cfg(r#"
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
          org:  { type: string }
          year: { type: integer }
"#)
        .example();
        assert!(!out.contains("<must-fill>"), "no sentinels expected: {out}");
        assert!(out.contains("refs:  # array<object>\n"), "{out}");
        assert!(out.contains("  - org: ACME\n"), "{out}");
        assert!(out.contains("    year: 2020\n"), "{out}");
        assert!(!out.contains("; delete-ok"), "not endorsed: {out}");
    }

    #[test]
    fn empty_typed_object_and_table_render_type_valid_and_validate() {
        // `properties: {}` is a legal (if degenerate) schema — the rejection
        // targets *absent* properties (freeform), not empty ones. With no
        // leaves to fill, the value cell must be the empty container (`{}` /
        // `[]`), not a bare null that fails the field's own validation. Holds
        // for both reference documents.
        let config = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    addr: { type: object, properties: {} }
    rows: { type: array,  items: { type: object, properties: {} } }
"#);
        for doc_md in [config.blueprint(), config.example()] {
            assert!(
                doc_md.contains("addr: {}  # object\n"),
                "object cell:\n{doc_md}"
            );
            assert!(
                doc_md.contains("rows: []  # array<object>\n"),
                "table cell:\n{doc_md}"
            );
            let doc = Document::from_markdown(&doc_md)
                .unwrap_or_else(|e| panic!("must parse: {e:?}\n---\n{doc_md}"));
            config
                .validate_document(&doc)
                .unwrap_or_else(|errs| panic!("must validate: {errs:?}\n---\n{doc_md}"));
        }
    }

    #[test]
    fn example_round_trips_to_a_valid_document() {
        // Fields with examples render them; the example-less ones fall back to
        // type-empty. The result must parse and validate.
        let config = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title:     { type: string, example: My Title }
    count:     { type: integer }
    severity:  { type: string, enum: [low, medium, high] }
    bio:       { type: markdown }
    issued:    { type: date }
    refs:      { type: array, items: { type: string }, example: [first, second] }
"#);
        let filled = config.example();
        let doc = Document::from_markdown(&filled)
            .unwrap_or_else(|e| panic!("example document must parse: {e:?}\n---\n{filled}"));
        config.validate_document(&doc).unwrap_or_else(|errs| {
            panic!("example document must validate: {errs:?}\n---\n{filled}")
        });
        let main = doc.main().payload();
        assert_eq!(main.get("title").unwrap().as_str().unwrap(), "My Title");
        assert_eq!(main.get("count").unwrap().as_i64().unwrap(), 0);
        assert_eq!(main.get("severity").unwrap().as_str().unwrap(), "low");
        let refs = main.get("refs").unwrap().as_array().unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].as_str().unwrap(), "first");
        assert_eq!(refs[1].as_str().unwrap(), "second");
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
