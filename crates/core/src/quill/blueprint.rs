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
//! - **Inline `# …` annotation** on the value line is structural and purely
//!   type information: `# <type>[<format>]`. Type is mandatory on every field.
//!   Format slot uses angle brackets (`array<string>`, `datetime<YYYY-MM-DD[Thh:mm:ss]>`,
//!   `enum<a | b | c>`). Shippability is carried by the value cell alone: an
//!   **Endorsed** cell (the field has a `default:`) renders that concrete value,
//!   shippable as-is; an **Unendorsed** cell (no `default:`) carries the
//!   `!must_fill` marker on the value line (`field: !must_fill`, or
//!   `field: !must_fill <example>` when an example supplies a suggested value).
//! - **Metadata annotation.** The root `$quill` line carries an inline
//!   `# keep verbatim — do not drop` reminder: an in-band nudge against the
//!   `parse::missing_quill` failure where an LLM author omits the line
//!   (experimental — see issue #734). `$kind` carries no such reminder; an
//!   omitted root `$kind: main` is synthesised at parse time, so it is not a
//!   hard requirement. A composable card emits its `composable (0..N)` role
//!   as an own-line `# …` comment directly under the `$kind` line.
//! - **Body regions** are signalled by `Write main body here.` after the main
//!   fence and `Write <card kind> body here.` after each card fence. When
//!   `body.example` is set, the example text is embedded verbatim instead.
//!   Absent when `body.enabled` is false.
//!
//! `ui.order` controls field ordering. `ui.group` clusters fields together
//! within the document but emits no banner.

use std::collections::BTreeMap;

use super::{CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::document::emit::{saphyr_emit_flow, saphyr_emit_scalar};
use crate::value::QuillValue;
use serde_json::Value as JsonValue;

impl QuillConfig {
    /// Generate the canonical annotated Markdown blueprint for this quill —
    /// the authoring surface handed to LLMs and humans, with Unendorsed cells
    /// carrying the `!must_fill` marker. See module docs for the annotation
    /// grammar; the function is total over any valid `QuillConfig`.
    ///
    /// The "filled-out" twin of the blueprint is **seeding** (`seed_document`
    /// in the `quillmark` crate) — a committed [`Document`] rather than an
    /// annotated string. See `prose/canon/BLUEPRINT.md`.
    ///
    /// The result is guaranteed schema-valid and parseable (every key
    /// present, every value type-correct). It is *not* guaranteed to render
    /// — that is the quill authoring contract on `plate.typ`; see
    /// `prose/canon/BLUEPRINT.md` §Guarantees.
    ///
    /// [`Document`]: crate::Document
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
/// `~~~\n$quill: …  # keep verbatim — do not drop\n$kind: main\n[# desc\n]<fields>~~~\n`.
///
/// The `$quill` system-metadata line leads the block, carrying an inline
/// `# keep verbatim — do not drop` reminder against the `parse::missing_quill`
/// failure where an author drops it (experimental — see issue #734). The
/// optional description follows as an own-line comment.
fn write_main_fence(
    out: &mut String,
    card: &CardSchema,
    quill_ref: &str,
    description: Option<&str>,
) {
    out.push_str("~~~\n");
    out.push_str("$quill: ");
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(
        quill_ref.to_string(),
    )));
    out.push_str("  # keep verbatim — do not drop\n");
    out.push_str("$kind: main\n");
    if let Some(desc) = description {
        write_comment(out, desc);
    }
    write_card_fields(out, card);
    out.push_str("~~~\n");
}

/// Emit a composable card as a `~~~` block declaring `$kind: <kind>`.
/// The `composable (0..N)` role annotation and the optional description are
/// emitted as own-line comments directly under the `$kind` header.
fn write_card_fence(out: &mut String, card: &CardSchema) {
    out.push_str("~~~\n");
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
    for field in group_fields(card.fields.values()) {
        write_field(out, field, 0);
    }
}

/// Order fields by `ui.group` (ungrouped lead, then groups in first-appearance
/// order) with `ui.order` sorting within each cluster. The grouping is purely
/// positional now — no banner is emitted — so the clusters are flattened into a
/// single field stream.
fn group_fields<'a, I: IntoIterator<Item = &'a FieldSchema>>(fields: I) -> Vec<&'a FieldSchema> {
    let mut sorted: Vec<&FieldSchema> = fields.into_iter().collect();
    sorted.sort_by_key(|f| f.ui_order());
    let mut groups: Vec<(Option<&str>, Vec<&FieldSchema>)> = Vec::new();
    for field in sorted {
        let group = field.ui.as_ref().and_then(|u| u.group.as_deref());
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some(slot) => slot.1.push(field),
            None => groups.push((group, vec![field])),
        }
    }
    groups.sort_by_key(|(g, _)| g.is_some());
    groups.into_iter().flat_map(|(_, fields)| fields).collect()
}

fn write_field(out: &mut String, field: &FieldSchema, indent: usize) {
    let pad = "  ".repeat(indent);

    // Typed table: an array whose element is an object with properties.
    // Scalar-element arrays (`string[]`, `integer[]`, `markdown[]`, …) fall
    // through to the uniform scalar rendering below.
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

    // Typed dictionary: standalone object with defined properties.
    if matches!(field.r#type, FieldType::Object) {
        if let Some(props) = &field.properties {
            write_typed_object_field(out, field, props, indent);
            return;
        }
    }

    write_description(out, field, &pad);
    // For Unendorsed scalar/array fields the `example` is inlined as the
    // `!must_fill` value (a reviewable suggested value), so a separate `# e.g.`
    // line would duplicate it. Endorsed fields render their default, so their
    // `example` still surfaces as a distinct hint.
    if field.default.is_some() {
        write_eg_comment(out, field, &pad);
    }

    // Markdown fields render as a YAML block scalar so multi-line content has
    // a consistent shape regardless of whether a default is configured.
    if matches!(field.r#type, FieldType::Markdown) {
        let inline = inline_annotation(field);
        write_markdown_block(out, field, &pad, &inline);
        return;
    }

    let inline = format!("  # {}", inline_annotation(field));
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
    let content = field.default.as_ref().and_then(|v| match v.as_json() {
        serde_json::Value::String(s) => Some(s),
        _ => None,
    });
    let body_pad = format!("{}  ", pad);
    match content {
        // Endorsed cell with content → render the default as a block scalar.
        Some(text) if !text.is_empty() => {
            out.push_str(&format!("{}{}: |-  # {}\n", pad, field.name, inline));
            for line in text.lines() {
                out.push_str(&format!("{}{}\n", body_pad, line));
            }
        }
        // Endorsed cell with empty default → block scalar with one indented
        // blank line (the "skippable" markdown cell).
        Some(_) => {
            out.push_str(&format!("{}{}: |-  # {}\n", pad, field.name, inline));
            out.push_str(&format!("{}\n", body_pad));
        }
        // Unendorsed cell (no `default:`) → the bare `!must_fill` marker on the
        // value line. Null ≡ absent, so it zero-fills to an empty body at render.
        None => {
            out.push_str(&format!("{}{}: !must_fill  # {}\n", pad, field.name, inline));
        }
    }
}

/// Emit the first property of a synthetic typed-table row onto the dash line,
/// matching `to_markdown`'s canonical object-sequence-item shape: own-line
/// comments sit at the dash indent *above* the dash, and the property's
/// `key: value  # type` rides the dash (`  - key: …`). Without this the
/// blueprint emits the dash on its own line, which re-emits differently and
/// breaks the round-trip guarantee (see `prose/canon/BLUEPRINT.md` §Guarantees).
fn write_synthetic_row_head(out: &mut String, first: &FieldSchema, indent: usize) {
    let dash_pad = "  ".repeat(indent + 1);
    let prop_pad = "  ".repeat(indent + 2);
    let mut buf = String::new();
    write_field(&mut buf, first, indent + 2);
    let mut on_dash = false;
    for line in buf.lines() {
        if !on_dash && line.trim_start().starts_with('#') {
            // Leading own-line comment (description / `# e.g.`) → dash indent.
            out.push_str(&format!("{}{}\n", dash_pad, line.trim_start()));
        } else if !on_dash {
            // First non-comment line is the field line → splice in the dash.
            let body = line.strip_prefix(prop_pad.as_str()).unwrap_or(line);
            out.push_str(&format!("{}- {}\n", dash_pad, body));
            on_dash = true;
        } else {
            // Trailing lines of a multi-line value keep their original indent.
            out.push_str(line);
            out.push('\n');
        }
    }
}

fn sort_props(props: &BTreeMap<String, Box<FieldSchema>>) -> Vec<&FieldSchema> {
    let mut v: Vec<&FieldSchema> = props.values().map(|b| b.as_ref()).collect();
    v.sort_by_key(|f| f.ui_order());
    v
}

/// Emit a typed-table field: description + `# e.g.` line (whenever an
/// example is configured), then the field key with its `array<object>`
/// inline annotation, then the rendered rows. Rows come from the `default:`,
/// or a synthetic template row otherwise.
///
/// Cell rule (uniform with scalars): a field with a `default:` is Endorsed
/// — it renders concrete rows. A field without a `default:` is Unendorsed —
/// the blueprint emits one synthetic row with leaf-level `!must_fill` markers.
/// The container key itself is never tagged (you mark the leaves, not the
/// container). See `prose/BOOKMARKS.md` "Typed container empty default loses
/// inline shape documentation" for the rendering-vs-symmetry trade-off.
fn write_typed_table_field(
    out: &mut String,
    field: &FieldSchema,
    item_props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    // Endorsement keys off `default:` alone, which also supplies the rendered
    // rows.
    let inline = inline_annotation(field);
    let rows = field.default.as_ref().and_then(|v| match v.as_json() {
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
            let props = sort_props(item_props);
            let dash_pad = "  ".repeat(indent + 1);
            match props.split_first() {
                None => out.push_str(&format!("{}-\n", dash_pad)),
                Some((first, rest)) => {
                    write_synthetic_row_head(out, first, indent);
                    for prop in rest {
                        write_field(out, prop, indent + 2);
                    }
                }
            }
        }
    }
}

/// Emit a typed-dictionary field: description + `# e.g.` line (whenever an
/// example is configured), then the field key with its `object` inline
/// annotation, then the rendered mapping. The mapping comes from the
/// `default:`, or per-property annotations otherwise.
///
/// Cell rule (uniform with scalars): a field with a `default:` is Endorsed
/// — the rendered value is the resolved mapping (a block mapping for non-empty,
/// inline `{}` for `{}`). A field without a `default:` is Unendorsed — the
/// blueprint recurses to per-property leaf-level `!must_fill` markers; the
/// container key itself is never tagged. See `prose/BOOKMARKS.md` "Typed
/// container empty default loses inline shape documentation" for the trade-off.
fn write_typed_object_field(
    out: &mut String,
    field: &FieldSchema,
    props: &BTreeMap<String, Box<FieldSchema>>,
    indent: usize,
) {
    let pad = "  ".repeat(indent);

    write_description(out, field, &pad);
    write_eg_comment(out, field, &pad);

    // Endorsement keys off `default:` alone, which also supplies the rendered
    // mapping.
    let inline = inline_annotation(field);
    let mapping = field.default.as_ref().and_then(|v| match v.as_json() {
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
                write_field(out, prop, indent + 1);
            }
        }
    }
}

/// Build the inline annotation body (without the leading `# `): purely the
/// structural type expression `# <type>[<format>]`. Shippability is carried by
/// the value cell alone — a concrete value is shippable as-is, a `!must_fill`
/// marker asks to be filled — so the annotation needs no cell-state tag.
fn inline_annotation(field: &FieldSchema) -> String {
    type_expression(field)
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
        FieldType::DateTime => "datetime<YYYY-MM-DD[Thh:mm:ss]>".into(),
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
/// Unendorsed cells render the `!must_fill` marker on the value line.
enum FieldValue {
    Inline(String),
    Block(Vec<serde_json::Value>),
}

fn field_value(field: &FieldSchema) -> FieldValue {
    // Endorsed value: the `default:`, when present.
    if let Some(v) = &field.default {
        return json_to_value(v.as_json());
    }
    // Unendorsed cell → the `!must_fill` marker. When the field carries an
    // `example`, it rides along as a reviewable suggested value
    // (`field: !must_fill <example>`); otherwise the marker is bare
    // (`field: !must_fill`). Either way the marker — not the value — is what a
    // consumer (and the non-fatal `validation::must_fill` warning) keys on.
    // Markdown is special-cased in `write_markdown_block` and never reaches here.
    let marker = match field.example.as_ref() {
        Some(eg) => format!("!must_fill {}", eg_hint(eg)),
        None => "!must_fill".to_string(),
    };
    FieldValue::Inline(marker)
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
    fn must_fill_string_renders_bare_marker() {
        // No `default:` → Unendorsed. The `!must_fill` marker sits on the value
        // line; the inline annotation is type-only.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    author: { type: string }
"#)
        .blueprint();
        assert!(t.contains("author: !must_fill  # string\n"));
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
        assert!(t.contains("# e.g. final\nstatus: draft  # string\n"));
    }

    #[test]
    fn endorsed_empty_default_renders_value_and_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: string, default: "", example: CONFIDENTIAL }
"#)
        .blueprint();
        assert!(t.contains("# e.g. CONFIDENTIAL\nclassification: \"\"  # string\n"));
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
        // Unendorsed field with an example: the example inlines as the
        // `!must_fill` suggested value, so no separate `# e.g.` line.
        assert!(t.contains(
            "recipient: !must_fill [Mr. John Doe, 123 Main St, \"Anytown, USA\"]  # array<string>\n"
        ));
        assert!(!t.contains("# e.g."));
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
        assert!(t.contains("format: standard  # enum<standard | informal>\n"));
        assert!(!t.contains("e.g."));
    }

    #[test]
    fn enum_must_fill_renders_bare_marker() {
        // An enum field with no `default:` renders `!must_fill` rather than
        // the first enum value — the cell is Unendorsed regardless.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    severity: { type: string, enum: [low, medium, high] }
"#)
        .blueprint();
        assert!(t.contains("severity: !must_fill  # enum<low | medium | high>\n"));
    }

    #[test]
    fn must_fill_array_with_example_inlines_example_as_marker_value() {
        // Plain (non-typed-table) Unendorsed array with an example: the example
        // rides along as the `!must_fill` suggested value (flow form), no `# e.g.`.
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
            "memo_from: !must_fill [ORG/SYMBOL, City ST 12345]  # array<string>\n"
        ));
        assert!(!t.contains("# e.g."));
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
        assert!(t.contains("# Be brief and clear.\nsubject: !must_fill  # string\n"));
    }

    #[test]
    fn every_field_carries_inline_type_and_cell_signal() {
        // Endorsed cells render a concrete value; Unendorsed cells carry the
        // `!must_fill` marker on the value line.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    issued: { type: datetime }
    published: { type: datetime }
    refs: { type: array, default: [], items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("title: !must_fill  # string\n"));
        assert!(t.contains("size: 11  # number\n"));
        assert!(t.contains("flag: false  # boolean\n"));
        assert!(t.contains("issued: !must_fill  # datetime<YYYY-MM-DD[Thh:mm:ss]>\n"));
        assert!(t.contains("published: !must_fill  # datetime<YYYY-MM-DD[Thh:mm:ss]>\n"));
        assert!(t.contains("refs: []  # array<string>\n"));
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
        assert!(t.contains("counts: !must_fill  # array<integer>\n"), "{t}");
        assert!(
            t.contains("sections: !must_fill  # array<markdown>\n"),
            "{t}"
        );
        assert!(t.contains("tags: !must_fill  # array<string>\n"), "{t}");
    }

    #[test]
    fn must_fill_markdown_renders_bare_marker() {
        // Unendorsed markdown → bare `!must_fill` on the value line (no block
        // scalar). Null ≡ absent, so it zero-fills to an empty body at render.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown }
"#)
        .blueprint();
        assert!(t.contains("bio: !must_fill  # markdown\n"));
        assert!(!t.contains("|-"));
    }

    #[test]
    fn endorsed_empty_markdown_renders_blank_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: markdown, default: "" }
"#)
        .blueprint();
        assert!(t.contains("bio: |-  # markdown\n  \n"));
        assert!(!t.contains("!must_fill"));
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
        assert!(t.contains("bio: |-  # markdown\n  ## About me\n  \n  Hello.\n"));
    }

    #[test]
    fn root_header_carries_quill_reminder_and_no_role_comment() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        // The root `$quill` line carries the inline "keep verbatim" reminder
        // (issue #734); `$kind: main` then goes straight to the description with
        // no own-line role comment (the root has no `composable` cardinality).
        assert!(t.starts_with(
            "~~~\n$quill: taro@0.1.0  # keep verbatim — do not drop\n$kind: main\n# x\n"
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
    fn typed_table_must_fill_emits_synthetic_row_with_leaf_markers() {
        // Unendorsed container → outer key untagged (markers live on the leaves).
        // Property leaves carry their own cell signals.
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
        // The first property rides the dash line (matching `to_markdown`), with
        // its description lifted to the dash indent above it.
        assert!(t.contains(
            "# Cited works.\nreferences:  # array<object>\n  # Citing organization.\n  - org: !must_fill  # string\n"
        ));
        assert!(t.contains("    # Publication year.\n    year: 0  # integer\n"));
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
        assert!(t.contains("refs:  # array<object>\n  - org: !must_fill  # string\n"));
        assert!(t.contains("    year: 0  # integer\n"));
    }

    #[test]
    fn typed_table_endorsed_renders_default_rows() {
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
        assert!(t.contains("refs:  # array<object>\n  - org: ACME\n"));
        assert!(!t.contains("refs:  # array<object>\n  -\n"));
    }

    #[test]
    fn typed_table_with_empty_default_renders_inline() {
        // `default: []` means shippable as-is — the value renders inline as `[]`
        // (no marker). Inline row shape under an empty default belongs in
        // `example:`; see prose/BOOKMARKS.md.
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
            t.contains("refs: []  # array<object>\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("!must_fill"), "no markers expected: {t}");
    }

    #[test]
    fn typed_dict_with_empty_default_renders_inline() {
        // Same uniform rule as typed tables: `default: {}` is Endorsed and
        // renders inline as `{}` (no marker).
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
            t.contains("address: {}  # object\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("!must_fill"), "no markers expected: {t}");
    }

    #[test]
    fn typed_dict_must_fill_emits_per_property_annotations() {
        // Unendorsed container → outer key untagged; per-property recursion
        // with leaf markers.
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
        assert!(t.contains("  # Street line.\n  street: !must_fill  # string\n"));
        assert!(t.contains("  city: !must_fill  # string\n"));
        assert!(t.contains("  zip: \"\"  # string\n"));
    }

    #[test]
    fn typed_dict_endorsed_renders_block_mapping() {
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
        assert!(t.contains("address:  # object\n"));
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
        assert!(t.contains("  street: !must_fill  # string\n"));
        assert!(t.contains("  city: \"\"  # string\n"));
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
      type: datetime
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
    fn typed_table_synthetic_row_blueprint_round_trips() {
        // Regression: an Unendorsed typed-table synthetic row emits the first
        // property on the dash line (canonical `to_markdown` shape), so the
        // generated blueprint round-trips idempotently.
        let bp = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string, description: Citing organization. }
          year: { type: integer, default: 0, description: Publication year. }
"#)
        .blueprint();
        let doc1 = Document::from_markdown(&bp).expect("blueprint must parse");
        let doc2 = Document::from_markdown(&doc1.to_markdown()).expect("re-emit must parse");
        assert_eq!(doc1, doc2, "typed-table blueprint must round-trip");
    }

    #[test]
    fn must_fill_markers_round_trip_and_survive_as_fill() {
        // An Unendorsed array with an example (inlined as `!must_fill [flow]`),
        // a bare-marker scalar, and a bare-marker datetime must all round-trip
        // through parse → emit → parse, and the `fill` flag must survive on each.
        let bp = cfg(r#"
quill: { name: letter, version: 1.0.0, backend: typst, description: A letter. }
main:
  fields:
    recipient:
      type: array
      items: { type: string }
      example: [Mr. John Doe, "Anytown, USA"]
    subject: { type: string }
    date: { type: datetime }
"#)
        .blueprint();

        let doc1 = Document::from_markdown(&bp).expect("blueprint must parse");
        // Every Unendorsed field parsed back as a `!must_fill` marker.
        let payload = doc1.main().payload();
        for key in ["recipient", "subject", "date"] {
            assert!(payload.is_fill(key), "`{key}` must carry the fill marker:\n{bp}");
        }
        // The example rode along as the suggested value, fill-free in JSON.
        assert_eq!(
            payload.get("recipient").and_then(|v| v.as_json().as_array().map(|a| a.len())),
            Some(2),
            "recipient suggested value should survive: {bp}"
        );

        // Idempotent round-trip.
        let md2 = doc1.to_markdown();
        let doc2 = Document::from_markdown(&md2).expect("re-emitted markdown must parse");
        assert_eq!(doc1, doc2, "blueprint must round-trip idempotently");
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

    #[test]
    fn blueprint_renders_default_and_surfaces_example_as_hint() {
        // A field with BOTH a `default:` and an `example:`: the blueprint
        // value cell renders the default (Endorsed), while the
        // example surfaces only as a leading `# e.g.` hint, never the value.
        let blueprint = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, default: draft, example: final }
"#)
        .blueprint();
        assert!(
            blueprint.contains("status: draft  # string\n"),
            "blueprint should render the default value: {blueprint}"
        );
        assert!(
            blueprint.contains("# e.g. final\n"),
            "blueprint should surface the example as a hint: {blueprint}"
        );
    }

    #[test]
    fn empty_typed_object_and_table_rejected() {
        // `properties: {}` is rejected — an object with no properties is
        // useless and almost certainly a mistake.
        for yaml in [
            r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    addr: { type: object, properties: {} }
"#,
            r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    rows: { type: array, items: { type: object, properties: {} } }
"#,
        ] {
            let errs = QuillConfig::from_yaml_with_warnings(yaml)
                .expect_err("expected error for empty properties");
            assert!(
                errs.iter()
                    .any(|e| e.code.as_deref() == Some("quill::object_empty_properties")),
                "expected quill::object_empty_properties, got: {errs:?}"
            );
        }
    }

    /// String defaults that look numeric/boolean/null must be quoted so
    /// the schema-validated payload still types as `string` after
    /// round-trip — defaults like `1.0`, `on`, `01234`, or `null` must
    /// not be emitted bare and re-parsed as the wrong YAML type.
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
