//! Auto-generated Markdown blueprint for a Quill.
//!
//! Produces an annotated reference document dense enough to replace the schema
//! for LLM consumers. The blueprint shows the document's shape — fields,
//! constraints, examples — so a consumer can write a fresh document from it.
//!
//! ## One emitter
//!
//! `blueprint()` does not format YAML itself. It builds a [`Document`] — the
//! same typed model a parsed `.md` produces — and emits it through the
//! canonical [`Document::to_markdown`]. The annotation grammar maps cleanly
//! onto the document model:
//!
//! - **Leading `# …` prose** (`# <description>`, `# e.g. <value>`) becomes
//!   own-line [`PayloadItem::Comment`]s before the field (top-level) or
//!   [`NestedComment`]s at the leaf's slot (typed-container properties).
//! - **Inline `# <type>[<format>]` annotation** becomes the field's *trailing
//!   inline* comment — a one-space ` # …` trailer on the value line.
//! - **`!must_fill`** becomes the field's `fill` flag (top-level) or a nested
//!   fill path on the value tree (per-property leaves).
//! - The `$quill` / `$kind` lines are the document's typed `$` metadata; the
//!   `# keep verbatim` reminder rides `$quill` as an inline comment.
//!
//! Because emission is shared with the parse/round-trip path, the blueprint
//! round-trips through `Document::from_markdown` and back *by construction* —
//! there is no second formatter to keep in sync.
//!
//! ## Rendering choices that follow from sharing `to_markdown`
//!
//! - **Richtext fields** carry no block scalar. An Unendorsed richtext field
//!   is a bare `field: !must_fill # richtext<markdown>`; an Endorsed one renders
//!   its default as an inline (double-quoted, `\n`-escaped) string. `to_markdown`
//!   emits no `|`/`>` block forms, so neither does the blueprint.
//! - **Arrays** render in block style at every level — including an
//!   Unendorsed array's `example`, which rides the `!must_fill` marker as
//!   block items rather than an inline flow sequence.
//! - **Typed dictionaries** with `default: {}` expand to the field's
//!   zero-filled shape (every key present, type-empty value, all unmarked) —
//!   so an empty endorsed object shows its structure instead of a bare `{}`.
//!   A *non-empty* partial default is rendered verbatim (a deliberate
//!   "already handled" signal); only `{}` expands.
//!
//! Declaration order controls field ordering. `ui.group` clusters fields
//! together within the document but emits no banner.

use indexmap::IndexMap;

use super::{zero_value, CardSchema, FieldSchema, FieldType, QuillConfig};
use crate::document::emit::{saphyr_emit_flow, saphyr_emit_scalar};
use crate::document::prescan::NestedComment;
use crate::document::{Card, Document, Payload, PayloadItem};
use crate::value::{PathSegment, QuillValue};
use serde_json::{Map as JsonMap, Value as JsonValue};

impl QuillConfig {
    /// Generate the canonical annotated Markdown blueprint for this quill —
    /// the authoring surface handed to LLMs and humans, with Unendorsed cells
    /// carrying the `!must_fill` marker. See module docs for the annotation
    /// grammar; the function is total over any valid `QuillConfig`.
    ///
    /// The "filled-out" twin of the blueprint is **seeding**
    /// ([`Quill::seed_document`](crate::Quill::seed_document)) — a committed
    /// [`Document`] rather than an annotated string. See
    /// `prose/canon/BLUEPRINT.md`.
    ///
    /// The result is guaranteed schema-valid and parseable (every key
    /// present, every value type-correct). It is *not* guaranteed to render
    /// — that is the quill authoring contract on `plate.typ`; see
    /// `prose/canon/BLUEPRINT.md` §Guarantees.
    ///
    /// [`Document`]: crate::Document
    pub fn blueprint(&self) -> String {
        let main_desc = self
            .main
            .description
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| Some(self.description.as_str()).filter(|s| !s.is_empty()));

        let main = build_main_card(
            &self.main,
            &format!("{}@{}", self.name, self.version),
            main_desc,
        );
        let cards = self.card_kinds.iter().map(build_card).collect();

        Document::from_main_and_cards(main, cards).to_markdown()
    }
}

/// Whitespace-collapse a description into a single line; `None` when it
/// collapses to empty.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapse_opt(text: &Option<String>) -> Option<String> {
    text.as_deref()
        .map(collapse)
        .filter(|clean| !clean.is_empty())
}

/// The text after `# ` for a body region (`Write … here.` placeholder or the
/// configured `body.example`), wrapped so `to_markdown` emits it verbatim
/// after the closing fence. Empty when the card has no body.
fn body_text(card: &CardSchema, fallback_kind: &str) -> String {
    if !card.body_enabled() {
        return String::new();
    }
    let example = card.body.as_ref().and_then(|b| b.example.as_deref());
    let fallback = format!("Write {} body here.", fallback_kind);
    let text = example.unwrap_or(fallback.as_str());
    format!("\n{}\n", text)
}

/// Build the root card: `$quill` (with the `# keep verbatim` inline reminder),
/// `$kind: main`, the optional description own-line comment, then the fields.
fn build_main_card(card: &CardSchema, quill_ref: &str, description: Option<&str>) -> Card {
    let reference = quill_ref
        .parse()
        .expect("quill name@version is always a valid QuillReference");
    let mut items = vec![
        PayloadItem::Quill { reference },
        PayloadItem::comment_inline("keep verbatim"),
        PayloadItem::Kind {
            value: "main".into(),
        },
    ];
    if let Some(desc) = description {
        items.push(PayloadItem::comment(collapse(desc)));
    }
    append_fields(&mut items, card);
    Card::from_parts(
        Payload::from_items(items),
        // The blueprint's output *is* the markdown surface, so it imports the
        // body text (a trusted example or a generated placeholder) here and
        // re-emits it via `to_markdown`. The empty-corpus fallback is defensive —
        // a placeholder or a load-validated example never over-nests.
        crate::document::import_body(&body_text(card, "main"))
            .unwrap_or_else(|_| quillmark_richtext::RichText::empty()),
    )
}

/// Build a composable card: `$kind: <kind>`, the `composable (0..N)` role
/// comment, the optional description, then the fields.
fn build_card(card: &CardSchema) -> Card {
    let mut items = vec![
        PayloadItem::Kind {
            value: card.name.clone(),
        },
        PayloadItem::comment("composable (0..N)"),
    ];
    if let Some(desc) = collapse_opt(&card.description) {
        items.push(PayloadItem::comment(desc));
    }
    append_fields(&mut items, card);
    Card::from_parts(
        Payload::from_items(items),
        crate::document::import_body(&body_text(card, &card.name))
            .unwrap_or_else(|_| quillmark_richtext::RichText::empty()),
    )
}

/// Append every field of a card as payload items, clustered by `ui.group` and
/// ordered by declaration order. Group order follows the card's `ui.groups`
/// registry when present.
fn append_fields(items: &mut Vec<PayloadItem>, card: &CardSchema) {
    let registry: Option<Vec<&str>> = card
        .ui
        .as_ref()
        .and_then(|u| u.groups.as_ref())
        .map(|r| r.0.iter().map(|g| g.id.as_str()).collect());
    for field in group_fields(card.fields.values(), registry.as_deref()) {
        append_field(items, field);
    }
}

/// Order fields by `ui.group` (ungrouped lead, then grouped clusters),
/// preserving declaration order within each cluster (`fields` arrives in
/// declaration order and the clustering is stable). Grouped clusters follow
/// the `registry` declaration order when a registry is present, else first-
/// appearance order (the deprecated implicit-group fallback); for a migrated
/// quill the two are identical. Grouping is purely positional (no banner); the
/// clusters are flattened into a single field stream.
fn group_fields<'a, I: IntoIterator<Item = &'a FieldSchema>>(
    fields: I,
    registry: Option<&[&str]>,
) -> Vec<&'a FieldSchema> {
    let mut groups: Vec<(Option<&str>, Vec<&FieldSchema>)> = Vec::new();
    for field in fields {
        let group = field.ui.as_ref().and_then(|u| u.group.as_deref());
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some(slot) => slot.1.push(field),
            None => groups.push((group, vec![field])),
        }
    }
    // Ungrouped is the implicit leading pseudo-group (rank 0). Grouped clusters
    // sort by registry position shifted past it (`pos + 1`); with no registry
    // every group ties at `usize::MAX` and the stable sort preserves first
    // appearance.
    groups.sort_by_key(|(g, _)| match g {
        None => 0,
        Some(id) => registry
            .and_then(|order| order.iter().position(|o| o == id))
            .map(|pos| pos + 1)
            .unwrap_or(usize::MAX),
    });
    groups.into_iter().flat_map(|(_, fields)| fields).collect()
}

/// Append one top-level field. Dispatches typed tables (`array<object>`) and
/// typed dictionaries (`object` with `properties`) to their per-property
/// builders; everything else is a scalar/array cell.
fn append_field(items: &mut Vec<PayloadItem>, field: &FieldSchema) {
    // Typed table: an array whose element is an object with properties.
    if matches!(field.r#type, FieldType::Array) {
        if let Some(elem) = &field.items {
            if matches!(elem.r#type, FieldType::Object) {
                if let Some(props) = &elem.properties {
                    append_typed_table(items, field, props);
                    return;
                }
            }
        }
    }

    // Typed dictionary: standalone object with defined properties.
    if matches!(field.r#type, FieldType::Object) {
        if let Some(props) = &field.properties {
            append_typed_dict(items, field, props);
            return;
        }
    }

    append_scalar(items, field);
}

/// Push the leading prose comments for a *top-level* field: the description,
/// then the `# e.g.` hint. `eg_when` gates the hint — scalars surface it only
/// when Endorsed (an Unendorsed example inlines as the marker value instead),
/// while typed containers always surface it (their example never inlines).
fn push_leading(items: &mut Vec<PayloadItem>, field: &FieldSchema, eg_when: bool) {
    if let Some(desc) = collapse_opt(&field.description) {
        items.push(PayloadItem::comment(desc));
    }
    if eg_when {
        if let Some(eg) = field.example.as_ref() {
            items.push(PayloadItem::comment(format!("e.g. {}", eg_hint(eg))));
        }
    }
}

/// The cell's `(value, fill)` for a scalar/array/richtext leaf, per the value
/// cascade. Endorsed → the default, no marker. Unendorsed → the `example` (when
/// present) carried by the marker, else a bare null marker. A richtext leaf never
/// inlines an example: an Unendorsed richtext leaf is always a bare marker, but
/// its `example:` still surfaces as a `# e.g.` hint (see `append_scalar`).
fn scalar_cell(field: &FieldSchema) -> (JsonValue, bool) {
    if let Some(default) = &field.default {
        return (default.as_json().clone(), false);
    }
    if matches!(field.r#type, FieldType::RichText { .. }) {
        return (JsonValue::Null, true);
    }
    match field.example.as_ref() {
        Some(eg) => (eg.as_json().clone(), true),
        None => (JsonValue::Null, true),
    }
}

/// Append a scalar / scalar-array / richtext field as a single payload field
/// plus its trailing inline type annotation.
fn append_scalar(items: &mut Vec<PayloadItem>, field: &FieldSchema) {
    // A richtext field never inlines its `example:` as the marker value, so —
    // unlike other Unendorsed scalars — the example would vanish entirely.
    // Surface it as a `# e.g.` hint instead (no-ops when no `example:` is set).
    let eg_when = field.default.is_some() || matches!(field.r#type, FieldType::RichText { .. });
    push_leading(items, field, eg_when);
    let (json, fill) = scalar_cell(field);
    items.push(PayloadItem::Field {
        key: field.name.clone(),
        value: QuillValue::from_json(json),
        fill,
        nested_comments: Vec::new(),
    });
    items.push(PayloadItem::comment_inline(type_expression(field)));
}

/// Build the per-property body of an Unendorsed typed container: the value
/// mapping (each property at its own cell, in declaration order), the nested
/// comments (description + `# e.g.` + inline type annotation, addressed by
/// `container_path`/slot), and the nested fill paths. `prefix` is the container
/// path of the mapping relative to the field value (`[]` for a typed dict,
/// `[Index(0)]` for a typed table's synthetic row).
fn build_property_mapping(
    props: &IndexMap<String, Box<FieldSchema>>,
    prefix: &[PathSegment],
) -> (
    JsonMap<String, JsonValue>,
    Vec<NestedComment>,
    Vec<Vec<PathSegment>>,
) {
    let mut map = JsonMap::new();
    let mut nested = Vec::new();
    let mut fills = Vec::new();
    for (slot, prop) in props.values().map(|b| b.as_ref()).enumerate() {
        if let Some(desc) = collapse_opt(&prop.description) {
            nested.push(NestedComment {
                container_path: prefix.to_vec(),
                position: slot,
                text: desc,
                inline: false,
            });
        }
        // `# e.g.` only when Endorsed (see `push_leading`).
        if prop.default.is_some() {
            if let Some(eg) = prop.example.as_ref() {
                nested.push(NestedComment {
                    container_path: prefix.to_vec(),
                    position: slot,
                    text: format!("e.g. {}", eg_hint(eg)),
                    inline: false,
                });
            }
        }
        let (json, fill) = scalar_cell(prop);
        map.insert(prop.name.clone(), json);
        if fill {
            let mut path = prefix.to_vec();
            path.push(PathSegment::Key(prop.name.clone()));
            fills.push(path);
        }
        nested.push(NestedComment {
            container_path: prefix.to_vec(),
            position: slot,
            text: type_expression(prop),
            inline: true,
        });
    }
    (map, nested, fills)
}

/// Append a typed-dictionary field (`object` with `properties`). Endorsed:
/// render the default mapping (`{}` expands to the zero-filled shape; a
/// non-empty partial default is rendered verbatim, all unmarked). Unendorsed:
/// recurse per property with leaf-level markers and annotations; the container
/// key itself is untagged.
fn append_typed_dict(
    items: &mut Vec<PayloadItem>,
    field: &FieldSchema,
    props: &IndexMap<String, Box<FieldSchema>>,
) {
    push_leading(items, field, true);

    let (value, nested, fills) = match field.default.as_ref().map(|d| d.as_json()) {
        // `default: {}` → expand to the field's zero-filled shape so every key
        // is shown; all leaves are Endorsed-by-the-container, hence unmarked
        // and unannotated (uniform with a concrete default).
        Some(JsonValue::Object(map)) if map.is_empty() => {
            (zero_value(field).into_json(), Vec::new(), Vec::new())
        }
        // Concrete default (object or otherwise) → rendered verbatim, unmarked.
        Some(default) => (default.clone(), Vec::new(), Vec::new()),
        // Unendorsed → per-property recursion at the mapping root.
        None => {
            let (map, nested, fills) = build_property_mapping(props, &[]);
            (JsonValue::Object(map), nested, fills)
        }
    };

    push_container_field(items, &field.name, value, nested, fills, field);
}

/// Append a typed-table field (`array<object>`). Endorsed: render the default
/// rows verbatim (including `default: []`, which stays inline `[]` — arrays do
/// not expand). Unendorsed: emit one synthetic row carrying each property's
/// leaf-level marker and annotation; the container key itself is untagged.
fn append_typed_table(
    items: &mut Vec<PayloadItem>,
    field: &FieldSchema,
    item_props: &IndexMap<String, Box<FieldSchema>>,
) {
    push_leading(items, field, true);

    let (value, nested, fills) = match field.default.as_ref().map(|d| d.as_json()) {
        // Any default (including `[]`) is shippable as-is, rendered verbatim.
        Some(default) => (default.clone(), Vec::new(), Vec::new()),
        // Row type declares no properties (schema-invalid in practice): emit a
        // type-valid empty array rather than a null synthetic row.
        None if item_props.is_empty() => (JsonValue::Array(Vec::new()), Vec::new(), Vec::new()),
        // Unendorsed → one synthetic row, per-property markers at `[Index(0)]`.
        None => {
            let (row, nested, fills) = build_property_mapping(item_props, &[PathSegment::Index(0)]);
            (
                JsonValue::Array(vec![JsonValue::Object(row)]),
                nested,
                fills,
            )
        }
    };

    push_container_field(items, &field.name, value, nested, fills, field);
}

/// Push a typed-container field (value + nested comments + nested fills) and
/// its trailing inline type annotation. The top-level `fill` flag is always
/// `false`: typed containers are tagged on their leaves, never the container.
fn push_container_field(
    items: &mut Vec<PayloadItem>,
    key: &str,
    value: JsonValue,
    nested_comments: Vec<NestedComment>,
    fills: Vec<Vec<PathSegment>>,
    field: &FieldSchema,
) {
    let mut quill_value = QuillValue::from_json(value);
    for path in &fills {
        quill_value.set_fill_at(path);
    }
    items.push(PayloadItem::Field {
        key: key.to_string(),
        value: quill_value,
        fill: false,
        nested_comments,
    });
    items.push(PayloadItem::comment_inline(type_expression(field)));
}

/// Build the inline annotation body (without the leading `# `): purely the
/// structural type expression `<type>[<format>]`. Shippability is carried by
/// the value cell alone — a concrete value is shippable as-is, a `!must_fill`
/// marker asks to be filled — so the annotation needs no cell-state tag.
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
        // The type names the role; the `<markdown>` format slot names the
        // surface encoding an author writes (and `to_markdown` re-emits).
        FieldType::RichText { inline: false } => "richtext<markdown>".into(),
        FieldType::RichText { inline: true } => "richtext(inline)<markdown>".into(),
        // The `<plain>` format slot names the literal codec (`from_plaintext`/
        // `to_plaintext`) — content the author navigates but which takes no
        // markup, distinct from richtext's `<markdown>` surface.
        FieldType::PlainText { inline: false } => "plaintext<plain>".into(),
        FieldType::PlainText { inline: true } => "plaintext(inline)<plain>".into(),
        // `enum` fields always carry `enum_values`, so the early return above
        // handles them; this arm is the defensive fallback for a valueless enum.
        FieldType::Enum => "enum".into(),
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

/// Format an example value as a compact one-line hint. Arrays and objects
/// render as YAML flow collections (`[a, b, c]`, `{k: v}`) so multi-element
/// shape information is preserved without expanding into multiple comment
/// lines.
fn eg_hint(example: &QuillValue) -> String {
    match example.as_json() {
        v @ (serde_json::Value::Array(_) | serde_json::Value::Object(_)) => saphyr_emit_flow(v),
        val => saphyr_emit_scalar(val),
    }
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;
    use crate::Document;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid yaml")
    }

    #[test]
    fn must_fill_markdown_example_surfaces_as_eg_hint_not_inline_value() {
        // Markdown never inlines its example as the marker value, but the
        // `example:` must still surface as a `# e.g.` hint (regression: #805).
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext, example: "Hello world" }
"#)
        .blueprint();
        assert!(t.contains("# e.g. Hello world\nbio: !must_fill # richtext<markdown>\n"));
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
        assert!(t.contains("# e.g. final\nstatus: draft # string\n"));
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
        assert!(t.contains("# e.g. CONFIDENTIAL\nclassification: \"\" # string\n"));
    }

    #[test]
    fn must_fill_array_example_renders_as_block_sequence_with_context_quoting() {
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
        // Unendorsed field with an example: the example rides the `!must_fill`
        // marker as block-style items (no inline flow), so no separate `# e.g.`.
        assert!(t.contains(
            "recipient: !must_fill # array<string>\n  - Mr. John Doe\n  - 123 Main St\n  - Anytown, USA\n"
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
        assert!(t.contains("format: standard # enum<standard | informal>\n"));
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
        assert!(t.contains("severity: !must_fill # enum<low | medium | high>\n"));
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
        assert!(t.contains("# Be brief and clear.\nsubject: !must_fill # string\n"));
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
        assert!(t.contains("title: !must_fill # string\n"));
        assert!(t.contains("size: 11 # number\n"));
        assert!(t.contains("flag: false # boolean\n"));
        assert!(t.contains("issued: !must_fill # datetime<YYYY-MM-DD[Thh:mm:ss]>\n"));
        assert!(t.contains("published: !must_fill # datetime<YYYY-MM-DD[Thh:mm:ss]>\n"));
        assert!(t.contains("refs: [] # array<string>\n"));
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
    sections: { type: array, items: { type: richtext } }
    tags:     { type: array, items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("counts: !must_fill # array<integer>\n"), "{t}");
        assert!(
            t.contains("sections: !must_fill # array<richtext<markdown>>\n"),
            "{t}"
        );
        assert!(t.contains("tags: !must_fill # array<string>\n"), "{t}");
    }

    #[test]
    fn must_fill_markdown_renders_bare_marker() {
        // Unendorsed markdown → bare `!must_fill` on the value line (no block
        // scalar). Null ≡ absent, so it zero-fills to an empty body at render.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext }
"#)
        .blueprint();
        assert!(t.contains("bio: !must_fill # richtext<markdown>\n"));
        assert!(!t.contains("|-"));
    }

    #[test]
    fn endorsed_empty_markdown_renders_empty_string() {
        // Endorsed empty markdown default → an inline empty-string cell (no
        // block scalar): the "skippable" markdown cell, shippable as-is.
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext, default: "" }
"#)
        .blueprint();
        assert!(t.contains("bio: \"\" # richtext<markdown>\n"));
        assert!(!t.contains("|-"));
        assert!(!t.contains("!must_fill"));
    }

    #[test]
    fn endorsed_markdown_default_inlines_quoted() {
        // Endorsed multi-line markdown default → an inline double-quoted scalar
        // with `\n` escapes (no block scalar): the canonical `to_markdown` form.
        let t = cfg(r###"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio:
      type: richtext
      default: "## About me\n\nHello."
"###)
        .blueprint();
        assert!(t.contains("bio: \"## About me\\n\\nHello.\" # richtext<markdown>\n"));
        assert!(!t.contains("|-"));
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
        // The root `$quill` line carries the inline "keep verbatim" reminder;
        // `$kind: main` then goes straight to the description with no own-line
        // role comment (the root has no `composable` cardinality).
        assert!(t.starts_with("~~~\n$quill: taro@0.1.0 # keep verbatim\n$kind: main\n# x\n"));
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
            "# Cited works.\nreferences: # array<object>\n  # Citing organization.\n  - org: !must_fill # string\n"
        ));
        assert!(t.contains("    # Publication year.\n    year: 0 # integer\n"));
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
        assert!(t.contains("refs: # array<object>\n  - org: !must_fill # string\n"));
        assert!(t.contains("    year: 0 # integer\n"));
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
        assert!(t.contains("refs: # array<object>\n  - org: ACME\n"));
        assert!(!t.contains("refs: # array<object>\n  -\n"));
    }

    #[test]
    fn typed_table_with_empty_default_renders_inline() {
        // `default: []` means shippable as-is — the value renders inline as `[]`
        // (no marker). Inline row shape under an empty default belongs in
        // `example:` (borb-sh/quillmark#736).
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
            t.contains("refs: [] # array<object>\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("!must_fill"), "no markers expected: {t}");
    }

    #[test]
    fn typed_dict_with_empty_default_expands_to_zero_filled() {
        // `default: {}` is Endorsed (the whole object ships as-is) and expands
        // to the field's zero-filled shape: every key shown with its type-empty
        // value, all unmarked and unannotated — so the structure is visible
        // instead of a bare `{}`. (Arrays do not expand; only `{}` does.)
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      default: {}
      properties:
        street: { type: string }
        zip:    { type: integer }
"#)
        .blueprint();
        assert!(
            t.contains("address: # object\n  street: \"\"\n  zip: 0\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("{}"), "no bare empty object expected: {t}");
        assert!(!t.contains("!must_fill"), "no markers expected: {t}");
        // No per-property annotations on the endorsed (expanded) form.
        assert!(!t.contains("# string"), "no leaf annotations expected: {t}");
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
        assert!(t.contains("# Mailing address.\naddress: # object\n"));
        assert!(t.contains("  # Street line.\n  street: !must_fill # string\n"));
        assert!(t.contains("  city: !must_fill # string\n"));
        assert!(t.contains("  zip: \"\" # string\n"));
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
        assert!(t.contains("address: # object\n"));
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
        assert!(t.contains("address: # object\n"));
        assert!(
            t.contains("# e.g. {street: 1 Infinite Loop, city: Cupertino}\n")
                || t.contains("# e.g. {city: Cupertino, street: 1 Infinite Loop}\n")
        );
        assert!(t.contains("  street: !must_fill # string\n"));
        assert!(t.contains("  city: \"\" # string\n"));
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
        // An Unendorsed array with an example (carried as block items under the
        // `!must_fill` marker), a bare-marker scalar, and a bare-marker datetime
        // must all round-trip through parse → emit → parse, and the `fill` flag
        // must survive on each.
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
            assert!(
                payload.is_fill(key),
                "`{key}` must carry the fill marker:\n{bp}"
            );
        }
        // The example rode along as the suggested value, fill-free in JSON.
        assert_eq!(
            payload
                .get("recipient")
                .and_then(|v| v.as_json().as_array().map(|a| a.len())),
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
