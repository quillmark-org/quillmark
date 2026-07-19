//! Generates the virtual `@local/quillmark-helper:0.1.0` package that provides
//! document data to Typst plates.
//!
//! The backend regenerates `lib.typ` per render as pure source text — no
//! runtime data processing. Richtext fields carry canonical content JSON, lowered
//! here to markup **block bindings** (`#let _qm_cN = [ .. ]`) via
//! [`emit_content`]; the document data is a Typst **literal**
//! (`#let data = ( .. )`) whose content fields reference those blocks, present
//! date fields reference a `date` value-object block (`#let _qm_dN = { let v =
//! datetime(..); (value: v, display: (..args) => text(v.display(..args))) }`,
//! blank ⇒ `none`), `$cards` carry a per-kind-ordinal `$path`, and everything
//! else is a value literal Typst judges equal to the former `json()` parse
//! (date cells excepted — their block carries the `datetime`, not the data
//! literal); the schema address tables are a generated literal
//! `_qm-meta`, and each content field's plaintext projection a generated literal
//! `_qm-plaintext` (backing the `plaintext(field)` helper, #873). See
//! [`generate_lib_typ`].
//!
//! Output is **canonical**: dict keys emit in sorted order at every level (via
//! [`sorted`]), so equal data produces byte-equal source regardless of the
//! caller's field order. `$cards` array order is semantic and preserved.

use std::collections::HashMap;
use std::ops::Range;

use crate::emit::{
    emit_content, emit_content_inline, escape_string, EmitError, Emission, SegmentMap,
};
use crate::SchemaMeta;
use quillmark_content::serial::from_canonical_value;

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
/// `FileId` for the span scan, and the segments key regions to content ranges.
pub struct ContentMap {
    pub path: String,
    pub block: Range<usize>,
    pub segments: Vec<SegmentMap>,
}

/// Generate `lib.typ` for the quillmark-helper package from the transformed
/// document data (richtext fields carry canonical content JSON; no `__meta__`
/// sentinel) plus the session's [`SchemaMeta`]. Content fields are lowered to
/// Typst markup here via [`emit_content`] — the codegen tier of the seam, the
/// one place a source map can be produced. Returns the source and each content
/// block's [`ContentMap`]; `Err` only when a content exceeds the nesting bound
/// (import capped it, so this fires only on a hand-built content).
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
    let plaintext_literal = plaintext_literal(&cg.plaintext);

    // Every placeholder is located in the *raw template* — trusted static text
    // — never in already-substituted output, so document data containing the
    // literal placeholder text cannot hijack a splice point (the #795 hygiene
    // fix, carried over). Slots are unique and ordered; each `find` starts
    // after the previous slot, scanning only the static template.
    let mut out = String::with_capacity(
        LIB_TYP_TEMPLATE.len() + cg.blocks.len() + data_literal.len() + plaintext_literal.len(),
    );
    let mut cursor = 0usize;
    let mut blocks_at = 0usize;
    for (slot, value) in [
        ("{version}", HELPER_VERSION),
        ("{meta_literal}", meta_literal.as_str()),
        ("{content_blocks}", cg.blocks.as_str()),
        ("{data_literal}", data_literal.as_str()),
        ("{plaintext_literal}", plaintext_literal.as_str()),
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
/// run's `gen` range) by `shift`; content (USV) offsets are unaffected.
fn rebase_segment(mut s: SegmentMap, shift: usize) -> SegmentMap {
    s.gen = (s.gen.start + shift)..(s.gen.end + shift);
    for run in &mut s.runs {
        run.1 = (run.1.start + shift)..(run.1.end + shift);
    }
    s
}

/// Accumulates the generated helper across one render: the content block
/// bindings, their brackets-included windows + rebased segment maps (relative to
/// the block section), the `_qm_cN` counter, the first content-lowering error (a
/// content over the nesting bound; import normally prevents it), and each content
/// field's plaintext projection (for the `plaintext(field)` helper, #873).
struct Codegen<'m> {
    meta: &'m SchemaMeta,
    blocks: String,
    windows: Vec<(String, Range<usize>, Vec<SegmentMap>)>,
    counter: usize,
    /// The `_qm_dN` date value-object counter. Separate from `counter` only to
    /// number date blocks densely (`_qm_d0`, `_qm_d1`, …) rather than sharing
    /// the content blocks' sequence; the `_qm_c`/`_qm_d` prefixes keep the ID
    /// spaces distinct either way, and both counters are a pure function of
    /// emission order (sorted keys, semantic card order), so the #801
    /// byte-identical-source invariant holds regardless.
    date_counter: usize,
    emit_error: Option<EmitError>,
    /// `(schema address, plaintext)` per non-blank content field — the content
    /// text with island slots stripped and marks dropped, keyed by the same
    /// address the content-block window uses (`subject`, `refs.2`,
    /// `$cards.note.0.$body`). Backs the generated `_qm-plaintext` table.
    plaintext: Vec<(String, String)>,
}

impl<'m> Codegen<'m> {
    fn new(meta: &'m SchemaMeta) -> Self {
        Self {
            meta,
            blocks: String::new(),
            windows: Vec::new(),
            counter: 0,
            date_counter: 0,
            emit_error: None,
            plaintext: Vec::new(),
        }
    }

    /// Bind an emitted content (`ec`) as a `#let _qm_cN = [\n{markup}\n]` block and
    /// return the binding's identifier. The `\n` wrap opens the block at a line
    /// boundary so line-anchored markup (headings, list items) parses. Records
    /// the bracketed byte window (brackets included) plus the emitter's segment
    /// maps, both rebased into `self.blocks` coordinates (`generate_lib_typ`
    /// shifts them once more by the block section's start).
    fn content_block(&mut self, path: &str, ec: Emission) -> String {
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

    /// Bind a present date/datetime as a **value-object** block and return its
    /// identifier — the date sibling of [`content_block`](Self::content_block).
    /// The object exposes two projections over the date encoded once in `v`:
    ///
    /// - `value` — the native `datetime` (`v`) for math, comparison, and
    ///   datetime-consuming packages; and
    /// - `display` — a closure `(..args) => text(v.display(..args))`. It returns
    ///   *content*, not a string, so its glyphs are born at **this `text(..)`
    ///   node's** lexical site inside the generated helper, per emission —
    ///   pinning per-instance identity a shared wrapper would collapse. Wrapping
    ///   `v.display` (not a re-literalized date) inherits `v`'s type, so a
    ///   date-only `v` throws Typst's native `[hour]`-pattern error.
    ///
    /// Records a **segment-less** window over that `text(..)` node keyed by
    /// `path` — one whole-placement region per instance when the date renders,
    /// so a card date defeats the loop-variable blindness `scalar_windows` does
    /// not chase (its ink resolves to this per-instance node, not the shared
    /// `card.<field>` reference site). Rebased into `self.blocks` coordinates
    /// like [`content_block`](Self::content_block); the whole date cell in the
    /// data literal is just the returned `_qm_dN` reference.
    ///
    /// Plates call it as `(data.<field>.display)(..)` — the paren form, since
    /// Typst reserves dict-key method sugar (`d.display(..)`) for built-in dict
    /// methods (pinned in `span_scan`'s spike 1).
    fn date_object(&mut self, path: &str, constructor: &str) -> String {
        let id = format!("_qm_d{}", self.date_counter);
        self.date_counter += 1;
        self.blocks.push_str("#let ");
        self.blocks.push_str(&id);
        self.blocks.push_str(" = { let v = ");
        self.blocks.push_str(constructor);
        self.blocks.push_str("; (value: v, display: (..args) => ");
        // The window covers exactly the `text(..)` call — the node whose span
        // the produced glyphs carry (span_scan spike 2). Empty segments make it
        // a whole-placement region, like a scalar site.
        let text_start = self.blocks.len();
        self.blocks.push_str("text(v.display(..args))");
        let text_end = self.blocks.len();
        self.blocks.push_str(") }\n");
        self.windows
            .push((path.to_string(), text_start..text_end, Vec::new()));
        id
    }

    /// Lower one richtext field's content JSON to a `#let` content block, or an
    /// empty string literal (`""`, not a block) for a blank content. A value that
    /// is not a valid content (never produced by the seam) degrades to its value
    /// literal; no render path re-parses markdown. A nesting-bound violation is
    /// recorded on `self.emit_error` and surfaced by `generate_lib_typ`.
    ///
    /// `inline` selects the lowering: an `inline` field (`richtext(inline)`)
    /// lowers to pure inline markup (no trailing `parbreak`, #872) via
    /// [`emit_content_inline`]; every other field keeps the block lowering.
    fn content_field(&mut self, path: &str, value: &serde_json::Value, inline: bool) -> String {
        let emit = if inline {
            emit_content_inline
        } else {
            emit_content
        };
        match from_canonical_value(value) {
            Ok(rt) if !rt.is_blank() => {
                // Record the plaintext projection (content text minus island
                // slots, marks dropped) for `plaintext(field)`, keyed by the same
                // address the content block windows on. Blank content values are skipped
                // — `plaintext` defaults them to `""`.
                let plain = quillmark_content::export::to_plaintext(&rt);
                if !plain.is_empty() {
                    self.plaintext.push((path.to_string(), plain));
                }
                match emit(&rt) {
                    Ok(ec) => self.content_block(path, ec),
                    Err(e) => {
                        self.emit_error.get_or_insert(e);
                        "\"\"".to_string()
                    }
                }
            }
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
            let date_kind = date_kind_of(&self.meta.date_fields, &self.meta.datetime_fields, key);
            let is_inline = self.meta.inline_fields.iter().any(|f| f == key);
            let expr = self.emit_field(key, value, is_content, date_kind, is_inline);
            items.push(format!("\"{}\": {}", escape_string(key), expr));
        }
        wrap_dict(items)
    }

    /// The `$cards` array literal. Each card of a string `$kind` gets its
    /// per-kind ordinal `$path` prefix injected and its content/date fields
    /// transformed; a card with no string `$kind` passes through untouched as
    /// a value literal, assigned no ordinal or `$path`.
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
        let datetimes = card_names(&self.meta.card_datetime_fields, kind);
        let inlines = card_names(&self.meta.card_inline_fields, kind);
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
            let date_kind = date_kind_of(&dates, &datetimes, key);
            let is_inline = inlines.iter().any(|f| f == key);
            let path = format!("{prefix}{key}");
            let expr = self.emit_field(&path, value, is_content, date_kind, is_inline);
            items.push(format!("\"{}\": {}", escape_string(key), expr));
        }
        wrap_dict(items)
    }

    /// A single field's value literal. Content (richtext) fields — a content
    /// object, or an array of them — lower to `#let` content blocks via
    /// [`Self::content_field`]; a blank content stays an empty string literal.
    /// `is_inline` picks pure-inline lowering (no `parbreak`) for a
    /// `richtext(inline)` field, per element for an `array<richtext(inline)>`.
    /// A `date_kind` field references a value-object block via
    /// [`date_object`](Self::date_object) (or `none` when blank) — the `datetime`
    /// is three-component for a `date`, six-component for a `datetime`.
    /// Everything else is a plain value literal.
    fn emit_field(
        &mut self,
        path: &str,
        value: &serde_json::Value,
        is_content: bool,
        date_kind: Option<DateKind>,
        is_inline: bool,
    ) -> String {
        if is_content {
            match value {
                // A richtext field crosses the seam as canonical content JSON (an
                // object); an `array<richtext>` as an array of them. Lower each
                // content to a content block here.
                serde_json::Value::Object(_) => self.content_field(path, value, is_inline),
                serde_json::Value::Array(arr) => {
                    let items = arr
                        .iter()
                        .enumerate()
                        .map(|(i, elem)| match elem {
                            serde_json::Value::Object(_) => {
                                self.content_field(&format!("{path}.{i}"), elem, is_inline)
                            }
                            other => lit(other),
                        })
                        .collect();
                    wrap_array(items)
                }
                other => lit(other),
            }
        } else if let Some(kind) = date_kind {
            match value {
                // A present date lowers to a value-object block; blank or
                // (defensively) unparseable ⇒ `none`, so `!= none` guards are
                // untouched. Coercion has already rejected malformed values.
                serde_json::Value::String(s) => match datetime_constructor(s, kind) {
                    Some(ctor) => self.date_object(path, &ctor),
                    None => "none".to_string(),
                },
                serde_json::Value::Null => "none".to_string(),
                other => lit(other),
            }
        } else {
            lit(value)
        }
    }
}

/// Which date type a field is, if any — selects the `datetime(..)` arity in
/// [`datetime_literal`]. A field name never appears in both tables (a schema
/// field has one type), so `date` is checked first and `datetime` second.
fn date_kind_of(dates: &[String], datetimes: &[String], key: &str) -> Option<DateKind> {
    if dates.iter().any(|f| f == key) {
        Some(DateKind::Date)
    } else if datetimes.iter().any(|f| f == key) {
        Some(DateKind::DateTime)
    } else {
        None
    }
}

/// The two date field types, distinguished by their Typst `datetime(..)` arity.
#[derive(Clone, Copy)]
enum DateKind {
    /// `type: date` — `datetime(year:, month:, day:)`.
    Date,
    /// `type: datetime` — `datetime(year:, month:, day:, hour:, minute:, second:)`.
    DateTime,
}

/// The card kind's field-name list from a `SchemaMeta` card table (content,
/// date, datetime, …). Shared with `validate_date_fields` in `lib.rs`.
pub(crate) fn card_names(
    table: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
) -> Vec<String> {
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

/// The `_qm-plaintext` literal: each content field's plaintext projection
/// (island slots stripped, marks dropped) keyed by schema address, emitted as a
/// Typst dict literal for the `plaintext(field)` helper (#873). Keys are sorted
/// so the output is a pure function of the data's values — a reorder-only
/// `apply` still produces byte-identical source (same #801 invariant as the
/// content blocks). Content addresses are unique, so no key collides.
fn plaintext_literal(entries: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    wrap_dict(
        sorted
            .into_iter()
            .map(|(path, text)| {
                format!("\"{}\": \"{}\"", escape_string(path), escape_string(text))
            })
            .collect(),
    )
}

/// The `datetime(..)` constructor for a coerced date/datetime string — the `v`
/// a [`date_object`](Codegen::date_object) captures — or `None` when the string
/// does not parse (the empty string included, since both parsers reject it),
/// which the caller lowers to `none`. Reuses the same parse the coercion layer
/// validates with, so a value that reached here parses; `None` is the defensive
/// arm. A `date` lowers to the three-component date-only form; a `datetime` to
/// the six-component wall-clock form, seconds zero-filled — carrying the
/// authored time-of-day through rather than truncating it.
fn datetime_constructor(s: &str, kind: DateKind) -> Option<String> {
    match kind {
        DateKind::Date => quillmark_core::quill::parse_date(s)
            .map(|(year, month, day)| format!("datetime(year: {year}, month: {month}, day: {day})")),
        DateKind::DateTime => {
            quillmark_core::quill::parse_datetime(s).map(|(year, month, day, hour, minute, second)| {
                format!(
                    "datetime(year: {year}, month: {month}, day: {day}, \
                     hour: {hour}, minute: {minute}, second: {second})"
                )
            })
        }
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
    use quillmark_core::quill::CONTENT_MEDIA_TYPE;

    fn meta_from(schema: serde_json::Value) -> SchemaMeta {
        SchemaMeta::from_schema_json(&schema)
    }

    /// The canonical content JSON the seam carries for a richtext field.
    fn content(markdown: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(markdown).expect("import");
        quillmark_content::serial::to_canonical_value(&rt)
    }

    /// A schema descriptor for a scalar richtext field.
    fn richtext_field() -> serde_json::Value {
        serde_json::json!({ "type": "object", "contentMediaType": CONTENT_MEDIA_TYPE })
    }

    /// A schema descriptor for a scalar `richtext(inline)` field.
    fn inline_richtext_field() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "contentMediaType": CONTENT_MEDIA_TYPE,
            "quillmark:inline": true
        })
    }

    /// An `inline` richtext field's block carries pure inline markup (no
    /// `parbreak`), while a plain richtext field keeps its `\n\n` terminator —
    /// so the inline value composes in `par(..)` without warning (#872).
    #[test]
    fn inline_field_lowers_without_parbreak() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "subject": inline_richtext_field(),
            "intro": richtext_field(),
        }}));
        let data = serde_json::json!({
            "subject": content("A **bold** subject"),
            "intro": content("An intro."),
        });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        // Inline: no parbreak inside the block.
        assert!(
            lib.contains("[\nA #strong[bold] subject\n]"),
            "inline block should carry no parbreak: {lib}"
        );
        // Block: the paragraph still terminates with `\n\n`.
        assert!(
            lib.contains("[\nAn intro.\n\n\n]"),
            "block field keeps its terminator: {lib}"
        );
    }

    /// Each element of an `array<richtext(inline)>` lowers inline (per the
    /// `quillmark:inline` flag on `items`), so no element carries a parbreak.
    #[test]
    fn inline_array_lowers_each_element_without_parbreak() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "cc": { "type": "array", "items": inline_richtext_field() }
        }}));
        let data = serde_json::json!({ "cc": [content("First"), content("Second **bold**")] });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        assert!(lib.contains("[\nFirst\n]"), "{lib}");
        assert!(lib.contains("[\nSecond #strong[bold]\n]"), "{lib}");
    }

    /// A richtext field's content lowers to a `#let _qm_cN = [ .. ]` block,
    /// referenced from `data`, with the recorded window covering exactly its
    /// bracketed range.
    #[test]
    fn content_field_becomes_a_referenced_block_with_a_bracket_window() {
        let meta = meta_from(serde_json::json!({ "properties": { "intro": richtext_field() } }));
        let data = serde_json::json!({ "intro": content("A **bold** intro.") });
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

    /// The segment-map offset check: every recorded `segment.gen` indexes the
    /// generated `lib.typ`, and every run's `gen` slices the escape of its
    /// content substring — the source map is byte-accurate against the emitted
    /// source, not just the emitter's own markup.
    #[test]
    fn segment_maps_index_the_generated_lib_typ() {
        let meta = meta_from(serde_json::json!({ "properties": { "intro": richtext_field() } }));
        let rt = quillmark_content::import::from_markdown("Hello **bold**.\n\nSecond para.")
            .expect("import");
        let data =
            serde_json::json!({ "intro": quillmark_content::serial::to_canonical_value(&rt) });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();

        assert_eq!(windows.len(), 1);
        let cm = &windows[0];
        // Two paragraphs → two segments.
        assert_eq!(cm.segments.len(), 2);
        let chars: Vec<char> = rt.text.chars().collect();
        let mut saw_bold = false;
        for seg in &cm.segments {
            assert!(seg.gen.start <= seg.gen.end && seg.gen.end <= lib.len());
            for (content_range, gen, ctx) in &seg.runs {
                // The run's generated bytes equal the escape of its content slice.
                let src: String = chars[content_range.clone()].iter().collect();
                let expect = match ctx {
                    EscapeCtx::Markup => escape_markup(&src),
                    EscapeCtx::StringLit => crate::emit::escape_string(&src),
                };
                assert_eq!(&lib[gen.clone()], expect, "run inverts to its content slice");
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
    /// windowed as `<field>.<i>`; a blank content stays an empty-string literal.
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
            "sections": [content("one"), content(""), content("three")]
        });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();

        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, vec!["sections.0", "sections.2"]);
        assert!(lib.contains("\"sections\": (_qm_c0, \"\", _qm_c1,)"));
    }

    /// Every non-blank content field gets a `_qm-plaintext` entry keyed by its
    /// schema address — the content text with marks dropped and island slots
    /// stripped — so `plaintext(field)` returns the field's string (#873). A
    /// blank field is absent (it defaults to `""`).
    #[test]
    fn content_fields_populate_the_plaintext_table() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "subject": richtext_field(),
            "refs": { "type": "array", "items": richtext_field() },
            "blank": richtext_field(),
        }}));
        let data = serde_json::json!({
            "subject": content("A **bold** subject"),
            "refs": [content("first ref"), content("second _ref_")],
            "blank": content("   "),
        });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        // Scope to the `_qm-plaintext` table: `data` also carries these keys
        // (bound to content blocks / the empty-string literal), so match the
        // table, not the whole file.
        let table = plaintext_table(&lib);
        // Marks are dropped: the plaintext is the content text, not the markup.
        assert!(table.contains(r#""subject": "A bold subject""#), "{table}");
        assert!(table.contains(r#""refs.0": "first ref""#), "{table}");
        assert!(table.contains(r#""refs.1": "second ref""#), "{table}");
        // The blank field carries no entry in the table.
        assert!(!table.contains("blank"), "{table}");
        assert!(lib.contains("#let plaintext(field) = _qm-plaintext.at(field"));
    }

    /// An image/table island contributes no plaintext: `to_plaintext` strips the
    /// island slot, so a field that is only an island yields no entry — the
    /// table is empty (`(:)`).
    #[test]
    fn island_only_content_has_no_plaintext_entry() {
        let meta = meta_from(serde_json::json!({ "properties": { "fig": richtext_field() } }));
        let data = serde_json::json!({ "fig": content("![alt](x.png)") });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        assert_eq!(
            plaintext_table(&lib),
            "(:)",
            "island-only field yields an empty plaintext table"
        );
    }

    /// A card content field's plaintext is keyed by the full card address
    /// (`$cards.<kind>.<n>.<field>`), so a plate composes it from the card's
    /// `$path` prefix — `plaintext(card.at("$path") + "$body")`.
    #[test]
    fn card_content_plaintext_keyed_by_card_address() {
        let meta = meta_from(serde_json::json!({
            "properties": {},
            "$defs": { "note_card": { "properties": { "$body": richtext_field() } } }
        }));
        let data = serde_json::json!({
            "$cards": [ { "$kind": "note", "$body": content("Note **body**") } ]
        });
        let (lib, _) = generate_lib_typ(&data, &meta).unwrap();
        assert!(
            plaintext_table(&lib).contains(r#""$cards.note.0.$body": "Note body""#),
            "{lib}"
        );
    }

    /// The `_qm-plaintext = ( .. )` dict literal, sliced out of a generated
    /// `lib.typ` so plaintext assertions don't collide with the `data` dict
    /// (which reuses the same keys, bound to content blocks) or the following
    /// `plaintext` helper's doc comment. The literal is single-line (`\n`s in
    /// values are escaped), so it ends at the first newline.
    fn plaintext_table(lib: &str) -> &str {
        let start = lib
            .find("#let _qm-plaintext = ")
            .expect("plaintext table present")
            + "#let _qm-plaintext = ".len();
        let rest = &lib[start..];
        let end = rest.find('\n').unwrap_or(rest.len());
        &rest[..end]
    }

    /// A present `date`/`datetime` field lowers to a value-object block whose
    /// `v` is the `datetime(..)` (three-component for a `date`, six-component
    /// wall-clock for a `datetime`, seconds zero-filled), referenced from the
    /// data literal; a null field stays `none` inline, with no block.
    #[test]
    fn date_and_datetime_fields_become_value_object_blocks() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "issued": { "type": "string", "format": "date" },
                "at": { "type": "string", "format": "date-time" },
                "signed": { "type": "string", "format": "date" }
            }
        }));
        let data = serde_json::json!({
            "issued": "2026-07-03",
            "at": "2026-07-03T09:30",
            "signed": null
        });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();
        // Keys emit in sorted order (`at` < `issued` < `signed`), so `_qm_d0`
        // is the six-component `datetime` field and `_qm_d1` the three-component
        // date. Each captures its `datetime` once, with the region-bearing
        // `text(v.display(..))` closure.
        assert!(
            lib.contains(
                "#let _qm_d0 = { let v = datetime(year: 2026, month: 7, day: 3, \
                 hour: 9, minute: 30, second: 0); \
                 (value: v, display: (..args) => text(v.display(..args))) }"
            ),
            "{lib}"
        );
        assert!(
            lib.contains(
                "#let _qm_d1 = { let v = datetime(year: 2026, month: 7, day: 3); \
                 (value: v, display: (..args) => text(v.display(..args))) }"
            ),
            "{lib}"
        );
        // The data literal references the blocks; the null field stays inline.
        assert!(lib.contains("\"at\": _qm_d0"), "{lib}");
        assert!(lib.contains("\"issued\": _qm_d1"), "{lib}");
        assert!(lib.contains("\"signed\": none"), "{lib}");
        // Present dates surface a window keyed by their schema path; the null
        // field contributes none.
        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert!(paths.contains(&"issued"), "{paths:?}");
        assert!(paths.contains(&"at"), "{paths:?}");
        assert!(!paths.contains(&"signed"), "{paths:?}");
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
                        "on": { "type": "string", "format": "date" }
                    }
                }
            }
        }));
        let data = serde_json::json!({
            "$cards": [
                { "$kind": "note", "$body": content("First body"), "on": "2026-01-02" },
                { "$kind": "note", "$body": content("Second body") }
            ]
        });
        let (lib, windows) = generate_lib_typ(&data, &meta).unwrap();
        // Each card instance gets its own per-instance windows: the body block
        // and — for the first card's present date — a value-object window keyed
        // by the card's full address, the per-instance identity that defeats the
        // shared-loop-variable blindness.
        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert!(paths.contains(&"$cards.note.0.$body"), "{paths:?}");
        assert!(paths.contains(&"$cards.note.1.$body"), "{paths:?}");
        assert!(paths.contains(&"$cards.note.0.on"), "{paths:?}");
        assert!(lib.contains("\"$path\": \"$cards.note.0.\""));
        assert!(lib.contains("\"$path\": \"$cards.note.1.\""));
        // The first card's date is a value-object block referenced from its cell.
        assert!(
            lib.contains("let v = datetime(year: 2026, month: 1, day: 2);"),
            "{lib}"
        );
        assert!(lib.contains("\"on\": _qm_d0"), "{lib}");
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
                "issued": { "type": "string", "format": "date" },
                "extra": { "type": "object" }
            },
            "$defs": {
                "note_card": {
                    "properties": {
                        "$body": richtext_field(),
                        "on": { "type": "string", "format": "date" }
                    }
                }
            }
        }));
        // `content` is a pure function, so inlining it in both orderings yields
        // the same content values — the point is the *key* order differs. The
        // date value-object path is deterministic too: its `_qm_dN` IDs key on
        // sorted emission order, so a reorder must not renumber them.
        let a = serde_json::json!({
            "body": content("The body."),
            "note": content("The note."),
            "issued": "2026-01-02",
            "extra": { "x": 1, "y": 2 },
            "$cards": [ { "$kind": "note", "$body": content("Card body"), "on": "2027-03-04", "tag": "t" } ]
        });
        let b = serde_json::json!({
            "$cards": [ { "tag": "t", "on": "2027-03-04", "$body": content("Card body"), "$kind": "note" } ],
            "extra": { "y": 2, "x": 1 },
            "issued": "2026-01-02",
            "note": content("The note."),
            "body": content("The body.")
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
