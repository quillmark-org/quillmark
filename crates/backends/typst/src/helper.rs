//! Generates the virtual `@local/quillmark-helper:0.1.0` package that provides
//! document data to Typst plates.
//!
//! The backend regenerates `lib.typ` per render as pure source text — no
//! runtime data processing. Richtext fields carry canonical corpus JSON, lowered
//! here to markup **block bindings** (`#let _qm_cN = [ .. ]`) via
//! [`emit_richtext`]; the document data is a Typst **literal**
//! (`#let data = ( .. )`) whose content fields reference those blocks, date
//! fields are `datetime(..)` constructors, `$cards` carry a per-kind-ordinal
//! `$path`, and everything else is a value literal Typst judges equal to the
//! former `json()` parse; the schema address tables are a generated literal
//! `_qm-meta`. See [`generate_lib_typ`].
//!
//! Output is **canonical**: dict keys emit in sorted order at every level (via
//! [`sorted`]), so equal data produces byte-equal source regardless of the
//! caller's field order. `$cards` array order is semantic and preserved.

use std::collections::HashMap;
use std::ops::Range;

use crate::emit::{emit_richtext, escape_string, EmitError, EmittedContent, SegmentMap};
use crate::SchemaMeta;
use quillmark_richtext::serial::from_canonical_value;

pub const HELPER_VERSION: &str = "0.1.0";
pub const HELPER_NAMESPACE: &str = "local";
pub const HELPER_NAME: &str = "quillmark-helper";

const LIB_TYP_TEMPLATE: &str = include_str!("lib.typ.template");

/// A generated content block's source map in the produced `lib.typ`: the
/// bracketed `block` range (`[ .. ]`, brackets included) of a `#let _qm_cN = [
/// .. ]` binding, keyed by the schema address of the content it carries, plus
/// the emitter's per-segment [`SegmentMap`]s rebased so every `gen` range
/// indexes the returned `lib.typ` directly. Every glyph the block places carries
/// a span that resolves into `block`; the world layer pairs it with the helper's
/// `FileId` for the span scan, and (from Phase 3) the segments key regions to
/// corpus ranges.
pub struct ContentMap {
    pub path: String,
    pub block: Range<usize>,
    pub segments: Vec<SegmentMap>,
}

/// Generate `lib.typ` for the quillmark-helper package from the transformed
/// document data (richtext fields carry canonical corpus JSON; no `__meta__`
/// sentinel) plus the session's [`SchemaMeta`]. Content fields are lowered to
/// Typst markup here via [`emit_richtext`] — the codegen tier of the seam, the
/// one place a source map can be produced. Returns the source and each content
/// block's [`ContentMap`]; `Err` only when a corpus exceeds the nesting bound
/// (import capped it, so this fires only on a hand-built corpus).
pub fn generate_lib_typ(
    data: &serde_json::Value,
    meta: &SchemaMeta,
) -> Result<(String, Vec<ContentMap>), EmitError> {
    let mut cg = Codegen::new(meta);
    let empty = serde_json::Map::new();
    let data_obj = data.as_object().unwrap_or(&empty);
    let data_literal = cg.emit_data(data_obj);
    // Surface any lowering failure the recursive (infallible-returning) walk
    // recorded, before assembling the output.
    if let Some(e) = cg.emit_error.take() {
        return Err(e);
    }
    let meta_literal = meta_literal(meta);

    // Every placeholder is located in the *raw template* — trusted static text
    // — never in already-substituted output, so document data containing the
    // literal placeholder text cannot hijack a splice point (the #795 hygiene
    // fix, carried over). Slots are unique and ordered; each `find` starts
    // after the previous slot, scanning only the static template.
    let mut out =
        String::with_capacity(LIB_TYP_TEMPLATE.len() + cg.blocks.len() + data_literal.len());
    let mut cursor = 0usize;
    let mut blocks_at = 0usize;
    for (slot, value) in [
        ("{version}", HELPER_VERSION),
        ("{meta_literal}", meta_literal.as_str()),
        ("{content_blocks}", cg.blocks.as_str()),
        ("{data_literal}", data_literal.as_str()),
    ] {
        let rel = LIB_TYP_TEMPLATE[cursor..]
            .find(slot)
            .unwrap_or_else(|| panic!("lib.typ.template carries the {slot} slot in order"));
        let at = cursor + rel;
        out.push_str(&LIB_TYP_TEMPLATE[cursor..at]);
        if slot == "{content_blocks}" {
            blocks_at = out.len();
        }
        out.push_str(value);
        cursor = at + slot.len();
    }
    out.push_str(&LIB_TYP_TEMPLATE[cursor..]);

    // Rebase every recorded window (block + its segments' `gen`/run ranges) from
    // block-section-relative to `lib.typ`-relative by the block section's start.
    let windows = cg
        .windows
        .into_iter()
        .map(|(path, block, segments)| ContentMap {
            path,
            block: (block.start + blocks_at)..(block.end + blocks_at),
            segments: segments
                .into_iter()
                .map(|s| rebase_segment(s, blocks_at))
                .collect(),
        })
        .collect();
    Ok((out, windows))
}

/// Shift a [`SegmentMap`]'s generated byte offsets (the segment window and each
/// run's `gen` range) by `shift`; corpus (USV) offsets are unaffected.
fn rebase_segment(mut s: SegmentMap, shift: usize) -> SegmentMap {
    s.gen = (s.gen.start + shift)..(s.gen.end + shift);
    for run in &mut s.runs {
        run.1 = (run.1.start + shift)..(run.1.end + shift);
    }
    s
}

/// Accumulates the generated helper across one render: the content block
/// bindings, their brackets-included windows + rebased segment maps (relative to
/// the block section), the `_qm_cN` counter, and the first content-lowering
/// error (a corpus over the nesting bound; import normally prevents it).
struct Codegen<'m> {
    meta: &'m SchemaMeta,
    blocks: String,
    windows: Vec<(String, Range<usize>, Vec<SegmentMap>)>,
    counter: usize,
    emit_error: Option<EmitError>,
}

impl<'m> Codegen<'m> {
    fn new(meta: &'m SchemaMeta) -> Self {
        Self {
            meta,
            blocks: String::new(),
            windows: Vec::new(),
            counter: 0,
            emit_error: None,
        }
    }

    /// Bind an emitted corpus (`ec`) as a `#let _qm_cN = [\n{markup}\n]` block and
    /// return the binding's identifier. The `\n` wrap opens the block at a line
    /// boundary so line-anchored markup (headings, list items) parses. Records
    /// the bracketed byte window (brackets included) plus the emitter's segment
    /// maps, both rebased into `self.blocks` coordinates (`generate_lib_typ`
    /// shifts them once more by the block section's start).
    fn content_block(&mut self, path: &str, ec: EmittedContent) -> String {
        let id = format!("_qm_c{}", self.counter);
        self.counter += 1;
        self.blocks.push_str("#let ");
        self.blocks.push_str(&id);
        self.blocks.push_str(" = ");
        let start = self.blocks.len();
        self.blocks.push_str("[\n");
        // The markup body opens after `[\n`; the emitter's `gen` offsets are
        // relative to it, so rebase by this position.
        let markup_at = self.blocks.len();
        self.blocks.push_str(&ec.markup);
        self.blocks.push_str("\n]");
        let end = self.blocks.len();
        self.blocks.push('\n');
        let segments = ec
            .segments
            .into_iter()
            .map(|s| rebase_segment(s, markup_at))
            .collect();
        self.windows.push((path.to_string(), start..end, segments));
        id
    }

    /// Lower one richtext field's corpus JSON to a `#let` content block, or an
    /// empty string literal for a blank corpus (matching the pre-corpus empty-
    /// content behavior — an empty field emitted `""`, not a block). A value that
    /// is not a valid corpus (never produced by the seam) degrades to its value
    /// literal; no render path re-parses markdown. A nesting-bound violation is
    /// recorded on `self.emit_error` and surfaced by `generate_lib_typ`.
    fn content_field(&mut self, path: &str, value: &serde_json::Value) -> String {
        match from_canonical_value(value) {
            Ok(rt) if !rt.is_blank() => match emit_richtext(&rt) {
                Ok(ec) => self.content_block(path, ec),
                Err(e) => {
                    self.emit_error.get_or_insert(e);
                    "\"\"".to_string()
                }
            },
            Ok(_) => "\"\"".to_string(),
            Err(_) => lit(value),
        }
    }

    /// The top-level `data` dict literal. Content and date fields are emitted
    /// per their schema classification; `$cards` gets the ordinal/`$path` pass;
    /// the `__meta__` sentinel (if any survived) is dropped.
    fn emit_data(&mut self, obj: &serde_json::Map<String, serde_json::Value>) -> String {
        let mut items = Vec::with_capacity(obj.len());
        for (key, value) in sorted(obj) {
            if key == "__meta__" {
                continue;
            }
            if key == "$cards" {
                if let Some(cards) = value.as_array() {
                    items.push(format!("\"$cards\": {}", self.emit_cards(cards)));
                    continue;
                }
            }
            let is_content = self.meta.content_fields.iter().any(|f| f == key);
            let is_date = self.meta.date_fields.iter().any(|f| f == key);
            let expr = self.emit_field(key, value, is_content, is_date);
            items.push(format!("\"{}\": {}", escape_string(key), expr));
        }
        wrap_dict(items)
    }

    /// The `$cards` array literal. Each card of a string `$kind` gets its
    /// per-kind ordinal `$path` prefix injected and its content/date fields
    /// transformed; a card with no string `$kind` passes through as a value
    /// literal (matching the template's former card loop, which skipped it).
    fn emit_cards(&mut self, cards: &[serde_json::Value]) -> String {
        let mut ordinals: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::with_capacity(cards.len());
        for card in cards {
            let obj = match card.as_object() {
                Some(o) => o,
                None => {
                    out.push(lit(card));
                    continue;
                }
            };
            match obj.get("$kind").and_then(|v| v.as_str()) {
                Some(kind) => {
                    let n = ordinals.entry(kind.to_string()).or_insert(0);
                    let prefix = format!("$cards.{kind}.{n}.");
                    *n += 1;
                    out.push(self.emit_card(obj, kind, &prefix));
                }
                None => out.push(lit(card)),
            }
        }
        wrap_array(out)
    }

    /// One card dict literal: the injected `$path` prefix plus each field
    /// classified against the card kind's content/date tables.
    fn emit_card(
        &mut self,
        obj: &serde_json::Map<String, serde_json::Value>,
        kind: &str,
        prefix: &str,
    ) -> String {
        let content = card_names(&self.meta.card_content_fields, kind);
        let dates = card_names(&self.meta.card_date_fields, kind);
        let mut items = Vec::with_capacity(obj.len() + 1);
        // The card's canonical address prefix, so plates compose schema-field
        // addresses — `form-field(.., field: card.at("$path") + "from")` —
        // without reimplementing the kind+ordinal grammar. `$`-prefixed so it
        // cannot collide with a schema field.
        items.push(format!("\"$path\": \"{}\"", escape_string(prefix)));
        for (key, value) in sorted(obj) {
            if key == "$path" {
                continue;
            }
            let is_content = content.iter().any(|f| f == key);
            let is_date = dates.iter().any(|f| f == key);
            let path = format!("{prefix}{key}");
            let expr = self.emit_field(&path, value, is_content, is_date);
            items.push(format!("\"{}\": {}", escape_string(key), expr));
        }
        wrap_dict(items)
    }

    /// A single field's value literal. Content (richtext) fields — a corpus
    /// object, or an array of them — lower to `#let` content blocks via
    /// [`Self::content_field`]; a blank corpus stays an empty string literal.
    /// Date fields become `datetime(..)` constructors (or `none`). Everything
    /// else is a plain value literal.
    fn emit_field(
        &mut self,
        path: &str,
        value: &serde_json::Value,
        is_content: bool,
        is_date: bool,
    ) -> String {
        if is_content {
            match value {
                // A richtext field crosses the seam as canonical corpus JSON (an
                // object); an `array<richtext>` as an array of them. Lower each
                // corpus to a content block here.
                serde_json::Value::Object(_) => self.content_field(path, value),
                serde_json::Value::Array(arr) => {
                    let items = arr
                        .iter()
                        .enumerate()
                        .map(|(i, elem)| match elem {
                            serde_json::Value::Object(_) => {
                                self.content_field(&format!("{path}.{i}"), elem)
                            }
                            other => lit(other),
                        })
                        .collect();
                    wrap_array(items)
                }
                other => lit(other),
            }
        } else if is_date {
            match value {
                serde_json::Value::String(s) => datetime_literal(s),
                serde_json::Value::Null => "none".to_string(),
                other => lit(other),
            }
        } else {
            lit(value)
        }
    }
}

/// The card kind's content/date field-name list from a `SchemaMeta` table.
fn card_names(table: &serde_json::Map<String, serde_json::Value>, kind: &str) -> Vec<String> {
    table
        .get(kind)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The `_qm-meta` literal: the schema address tables `_qm-known-path` validates
/// `form-field` paths against, emitted as a Typst dict literal.
fn meta_literal(meta: &SchemaMeta) -> String {
    let tables = serde_json::json!({
        "fields": meta.fields,
        "card_fields": meta.card_field_names,
        "array_fields": meta.array_fields,
        "card_array_fields": meta.card_array_fields,
    });
    lit(&tables)
}

/// A coerced datetime string as a Typst `datetime(year:, month:, day:)`
/// constructor, date-only (time and offset discarded), reusing the same parse
/// the coercion layer validates with. Empty or (defensively) unparseable ⇒
/// `none` — coercion has already rejected malformed dates upstream.
fn datetime_literal(s: &str) -> String {
    if s.is_empty() {
        return "none".to_string();
    }
    match quillmark_core::quill::parse_date_ymd(s) {
        Some((year, month, day)) => {
            format!("datetime(year: {year}, month: {month}, day: {day})")
        }
        None => "none".to_string(),
    }
}

/// Serialize a JSON value as a Typst literal expression, mirroring the value
/// model `json()` produces: `null` ⇒ `none`; integral `i64` numbers ⇒ int
/// literals; all other numbers ⇒ `float("..")` (Typst float *literals* take no
/// exponent syntax, but the `float(str)` constructor parses every finite
/// `f64`, and `json()` rejects non-finite numbers); strings via
/// [`escape_string`]; arrays and objects via [`wrap_array`] / [`wrap_dict`].
fn lit(v: &serde_json::Value) -> String {
    use serde_json::Value::*;
    match v {
        Null => "none".to_string(),
        Bool(b) => b.to_string(),
        Number(n) => {
            if let Some(i) = n.as_i64() {
                // Typst lexes a negative literal as unary `-` over an unsigned
                // magnitude, and `i64::MIN`'s magnitude (2^63) overflows i64 —
                // so `-9223372036854775808` would not round-trip. Emit it as an
                // int-typed expression instead (both operands fit i64). Every
                // other i64 renders as its own literal.
                if i == i64::MIN {
                    "(-9223372036854775807 - 1)".to_string()
                } else {
                    i.to_string()
                }
            } else {
                let f = n.as_f64().expect("json numbers are finite");
                format!("float(\"{f}\")")
            }
        }
        String(s) => format!("\"{}\"", escape_string(s)),
        Array(a) => wrap_array(a.iter().map(lit).collect()),
        Object(o) => wrap_dict(
            sorted(o)
                .into_iter()
                .map(|(k, v)| format!("\"{}\": {}", escape_string(k), lit(v)))
                .collect(),
        ),
    }
}

/// A map's entries in canonical (sorted-key) order — every dict the generator
/// emits goes through this. The workspace builds `serde_json` with
/// `preserve_order`, so a map's own iteration order is whatever the caller
/// inserted (and the transform pipeline routes through a `std::collections::
/// HashMap`, so it is not even that). Emitting in a canonical order instead
/// makes the generated source a pure function of the data's *values*: a
/// reorder-only `apply` produces byte-identical `lib.typ`, `Source::replace`
/// sees an empty diff, comemo reuses the whole compile, and no content block's
/// spans move (#801).
fn sorted(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<(&String, &serde_json::Value)> {
    let mut entries: Vec<_> = obj.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

/// A Typst array literal from pre-rendered element expressions. The trailing
/// comma keeps a single element an array rather than a parenthesized scalar;
/// empty is `()`.
fn wrap_array(items: Vec<String>) -> String {
    if items.is_empty() {
        "()".to_string()
    } else {
        format!("({},)", items.join(", "))
    }
}

/// A Typst dict literal from pre-rendered `"key": expr` entries; empty is
/// `(:)`.
fn wrap_dict(items: Vec<String>) -> String {
    if items.is_empty() {
        "(:)".to_string()
    } else {
        format!("({},)", items.join(", "))
    }
}

pub fn generate_typst_toml() -> String {
    format!(
        r#"[package]
name = "{name}"
version = "{version}"
namespace = "{namespace}"
entrypoint = "lib.typ"
"#,
        name = HELPER_NAME,
        version = HELPER_VERSION,
        namespace = HELPER_NAMESPACE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::escape_markup;
    use crate::emit::EscapeCtx;
    use quillmark_core::quill::RICHTEXT_MEDIA_TYPE;

    fn meta_from(schema: serde_json::Value) -> SchemaMeta {
        SchemaMeta::from_schema_json(&schema)
    }

    /// The canonical corpus JSON the seam carries for a richtext field.
    fn corpus(markdown: &str) -> serde_json::Value {
        let rt = quillmark_richtext::import::from_markdown(markdown).expect("import");
        quillmark_richtext::serial::to_canonical_value(&rt)
    }

    /// A schema descriptor for a scalar richtext field.
    fn richtext_field() -> serde_json::Value {
        serde_json::json!({ "type": "object", "contentMediaType": RICHTEXT_MEDIA_TYPE })
    }

    /// A richtext field's corpus lowers to a `#let _qm_cN = [ .. ]` block,
    /// referenced from `data`, with the recorded window covering exactly its
    /// bracketed range.
    #[test]
    fn content_field_becomes_a_referenced_block_with_a_bracket_window() {
        let meta = meta_from(serde_json::json!({ "properties": { "intro": richtext_field() } }));
        let data = serde_json::json!({ "intro": corpus("A **bold** intro.") });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();

        // The emitter terminates the paragraph with `\n\n` (as the markdown
        // lowering always did); the block wraps that in `[\n .. \n]`.
        let block = "[\nA #strong[bold] intro.\n\n\n]";
        assert!(lib.contains(&format!("#let _qm_c0 = {block}")));
        assert!(lib.contains("\"intro\": _qm_c0"));
        // No eval, no json() blob, no runtime assembly survive.
        assert!(!lib.contains("eval("));
        assert!(!lib.contains("json(bytes"));
        assert!(!lib.contains("insert-content"));
        assert!(!lib.contains("_parse-date"));

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].path, "intro");
        assert_eq!(&lib[windows[0].block.clone()], block);
    }

    /// The mandated segment-map offset check (PR-E step 4): every recorded
    /// `segment.gen` indexes the generated `lib.typ`, and every run's `gen`
    /// slices the escape of its corpus substring — the source map is byte-
    /// accurate against the emitted source, not just the emitter's own markup.
    #[test]
    fn segment_maps_index_the_generated_lib_typ() {
        let meta = meta_from(serde_json::json!({ "properties": { "intro": richtext_field() } }));
        let rt = quillmark_richtext::import::from_markdown("Hello **bold**.\n\nSecond para.")
            .expect("import");
        let data =
            serde_json::json!({ "intro": quillmark_richtext::serial::to_canonical_value(&rt) });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();

        assert_eq!(windows.len(), 1);
        let cm = &windows[0];
        // Two paragraphs → two segments.
        assert_eq!(cm.segments.len(), 2);
        let chars: Vec<char> = rt.text.chars().collect();
        let mut saw_bold = false;
        for seg in &cm.segments {
            assert!(seg.gen.start <= seg.gen.end && seg.gen.end <= lib.len());
            for (corpus_range, gen, ctx) in &seg.runs {
                // The run's generated bytes equal the escape of its corpus slice.
                let src: String = chars[corpus_range.clone()].iter().collect();
                let expect = match ctx {
                    EscapeCtx::Markup => escape_markup(&src),
                    EscapeCtx::StringLit => crate::emit::escape_string(&src),
                };
                assert_eq!(&lib[gen.clone()], expect, "run inverts to its corpus slice");
                // The run sits inside its segment's window.
                assert!(gen.start >= seg.gen.start && gen.end <= seg.gen.end);
                if src == "bold" {
                    saw_bold = true;
                }
            }
        }
        assert!(saw_bold, "the strong-wrapped run maps back to \"bold\"");
    }

    /// An `array<richtext>` field lowers one block per non-blank element,
    /// windowed as `<field>.<i>`; a blank corpus stays an empty-string literal.
    #[test]
    fn richtext_array_emits_a_block_per_nonblank_element() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "sections": {
                    "type": "array",
                    "items": richtext_field()
                }
            }
        }));
        let data = serde_json::json!({
            "sections": [corpus("one"), corpus(""), corpus("three")]
        });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();

        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, vec!["sections.0", "sections.2"]);
        assert!(lib.contains("\"sections\": (_qm_c0, \"\", _qm_c1,)"));
    }

    /// A date field becomes a date-only `datetime(..)` constructor; null stays
    /// `none`.
    #[test]
    fn date_field_becomes_a_datetime_constructor() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "issued": { "type": "string", "format": "date-time" },
                "signed": { "type": "string", "format": "date-time" }
            }
        }));
        let data = serde_json::json!({ "issued": "2026-07-03T09:30:00Z", "signed": null });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        assert!(lib.contains("\"issued\": datetime(year: 2026, month: 7, day: 3)"));
        assert!(lib.contains("\"signed\": none"));
    }

    /// Cards get a per-kind-ordinal `$path`, and content/date fields resolve
    /// against the card kind's tables. A second card of the same kind gets
    /// ordinal 1.
    #[test]
    fn cards_carry_path_and_per_kind_ordinals() {
        let meta = meta_from(serde_json::json!({
            "properties": {},
            "$defs": {
                "note_card": {
                    "properties": {
                        "$body": richtext_field(),
                        "on": { "type": "string", "format": "date-time" }
                    }
                }
            }
        }));
        let data = serde_json::json!({
            "$cards": [
                { "$kind": "note", "$body": corpus("First body"), "on": "2026-01-02" },
                { "$kind": "note", "$body": corpus("Second body") }
            ]
        });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();
        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, vec!["$cards.note.0.$body", "$cards.note.1.$body"]);
        assert!(lib.contains("\"$path\": \"$cards.note.0.\""));
        assert!(lib.contains("\"$path\": \"$cards.note.1.\""));
        assert!(lib.contains("\"on\": datetime(year: 2026, month: 1, day: 2)"));
    }

    /// The address tables round-trip into the `_qm-meta` literal.
    #[test]
    fn meta_literal_carries_the_address_tables() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "subject": { "type": "string" },
                "refs": { "type": "array", "items": { "type": "string" } }
            }
        }));
        let (lib, _) = generate_lib_typ(&serde_json::json!({}), &meta).unwrap();
        assert!(lib.contains("#let _qm-meta = ("));
        assert!(lib.contains("\"fields\": ("));
        assert!(lib.contains("\"subject\""));
        assert!(lib.contains("\"array_fields\": (\"refs\",)"));
    }

    /// Document data containing the literal slot text must not hijack the
    /// splice — slots are located in the raw template only.
    #[test]
    fn data_containing_placeholder_text_cannot_hijack_the_splice() {
        let meta = SchemaMeta::default();
        let data = serde_json::json!({
            "note": "quoting the template: {content_blocks} and {data_literal}"
        });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        let payload = lib.find("quoting the template").expect("payload present");
        let data_binding = lib.find("#let data =").expect("data binding present");
        assert!(
            payload > data_binding,
            "the payload sits inside the data literal, after the real slot"
        );
    }

    /// The caller's field order must not reach the emitted source: the same
    /// values in any key order — top-level, card, and nested dicts — produce
    /// byte-identical `lib.typ` and identical windows, so a reorder-only
    /// `apply` is a `Source::replace` no-op (#801).
    #[test]
    fn reordered_input_emits_byte_identical_source() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "body": richtext_field(),
                "note": richtext_field(),
                "extra": { "type": "object" }
            },
            "$defs": {
                "note_card": {
                    "properties": {
                        "$body": richtext_field()
                    }
                }
            }
        }));
        // `corpus` is a pure function, so inlining it in both orderings yields
        // the same corpus values — the point is the *key* order differs.
        let a = serde_json::json!({
            "body": corpus("The body."),
            "note": corpus("The note."),
            "extra": { "x": 1, "y": 2 },
            "$cards": [ { "$kind": "note", "$body": corpus("Card body"), "tag": "t" } ]
        });
        let b = serde_json::json!({
            "$cards": [ { "tag": "t", "$body": corpus("Card body"), "$kind": "note" } ],
            "extra": { "y": 2, "x": 1 },
            "note": corpus("The note."),
            "body": corpus("The body.")
        });

        let (lib_a, win_a) = generate_lib_typ(&a, &meta).unwrap();
        let (lib_b, win_b) = generate_lib_typ(&b, &meta).unwrap();
        assert_eq!(lib_a, lib_b, "reordered input must emit identical source");
        let wa: Vec<_> = win_a.iter().map(|w| (&w.path, w.block.clone())).collect();
        let wb: Vec<_> = win_b.iter().map(|w| (&w.path, w.block.clone())).collect();
        assert_eq!(wa, wb, "windows must be identical too");
    }

    #[test]
    fn lit_serializes_the_json_value_model() {
        assert_eq!(lit(&serde_json::json!(null)), "none");
        assert_eq!(lit(&serde_json::json!(true)), "true");
        assert_eq!(lit(&serde_json::json!(42)), "42");
        // i64::MIN cannot be a Typst literal (its magnitude overflows i64);
        // it is emitted as an int-typed expression instead.
        assert_eq!(
            lit(&serde_json::json!(i64::MIN)),
            "(-9223372036854775807 - 1)"
        );
        assert_eq!(lit(&serde_json::json!(1.5)), "float(\"1.5\")");
        assert_eq!(lit(&serde_json::json!("hi")), "\"hi\"");
        assert_eq!(lit(&serde_json::json!([])), "()");
        assert_eq!(lit(&serde_json::json!([1])), "(1,)");
        assert_eq!(lit(&serde_json::json!({})), "(:)");
        assert_eq!(lit(&serde_json::json!({ "a": 1 })), "(\"a\": 1,)");
    }
}
