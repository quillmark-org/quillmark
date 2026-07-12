//! Canonical JSON serialization — the freeze.
//!
//! Byte-deterministic within this schema: equal [`RichText`] values (by
//! `PartialEq` after [`RichText::normalize`]) serialize to byte-equal JSON,
//! insensitive to the order marks/islands were discovered in. Three order
//! sources are closed here and in `normalize`: mark order (canonical sort),
//! island order (slot position), and object-key order inside island `props` /
//! unknown-mark `attrs` (recursively sorted). `deserialize ∘ serialize` is a
//! fixed point on canonical bytes.
//!
//! The seam encoding (Option A) and the storage encoding are the *same*
//! canonical form — one serializer, not two to keep aligned.

use crate::model::{
    sort_keys_owned, sorted_value, Container, Invariant, Island, Line, LineKind, Loss, Mark,
    MarkKind, RichText,
};
use serde_json::{Map, Value};

/// Why canonical-JSON parsing failed. Structural only — a well-formed producer
/// (this crate's serializer, the seam, storage) never trips these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Top-level JSON was not an object, or a required key was missing/mistyped.
    Shape(&'static str),
    /// The JSON itself did not parse.
    Json(String),
    /// The value parsed but violates a corpus invariant.
    Invalid(crate::model::Invariant),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Shape(s) => write!(f, "richtext json shape: {s}"),
            ParseError::Json(s) => write!(f, "richtext json parse: {s}"),
            ParseError::Invalid(inv) => write!(f, "richtext invariant: {inv:?}"),
        }
    }
}
impl std::error::Error for ParseError {}

impl RichText {
    /// Serialize to canonical JSON bytes. Normalizes a copy first, so the output
    /// is canonical regardless of the caller's mark/island order. Every object
    /// key is sorted recursively so the bytes do **not** depend on
    /// `serde_json`'s `preserve_order` feature being enabled in the consumer's
    /// crate graph — the canonical form is feature-independent.
    pub fn to_canonical_json(&self) -> String {
        to_canonical_value(self).to_string()
    }

    /// Parse canonical JSON, normalize (idempotent), and validate. Returns
    /// [`ParseError::Invalid`] for a corpus that violates its invariants, so
    /// storage cannot silently round-trip a malformed value.
    /// `from_canonical_json(to_canonical_json(x))` round-trips to a canonical
    /// value and re-serializes to identical bytes.
    pub fn from_canonical_json(s: &str) -> Result<RichText, ParseError> {
        let v: Value = serde_json::from_str(s).map_err(|e| ParseError::Json(e.to_string()))?;
        from_canonical_value(&v)
    }

    fn to_value(&self) -> Value {
        let mut root = Map::new();
        root.insert("text".into(), Value::String(self.text.clone()));
        root.insert(
            "lines".into(),
            Value::Array(self.lines.iter().map(line_to_value).collect()),
        );
        root.insert(
            "marks".into(),
            Value::Array(self.marks.iter().map(mark_to_value).collect()),
        );
        root.insert(
            "islands".into(),
            Value::Array(self.islands.iter().map(island_to_value).collect()),
        );
        Value::Object(root)
    }

    fn from_value(v: &Value) -> Result<RichText, ParseError> {
        let obj = v.as_object().ok_or(ParseError::Shape("root not object"))?;
        let text = obj
            .get("text")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("text"))?
            .to_string();
        let lines = arr(obj, "lines")?
            .iter()
            .map(line_from_value)
            .collect::<Result<_, _>>()?;
        let marks = arr(obj, "marks")?
            .iter()
            .map(mark_from_value)
            .collect::<Result<_, _>>()?;
        let islands = arr(obj, "islands")?
            .iter()
            .map(island_from_value)
            .collect::<Result<_, _>>()?;
        Ok(RichText {
            text,
            lines,
            marks,
            islands,
        })
    }
}

/// The canonical richtext form as a structural [`Value`] — the recursively
/// key-sorted tree [`RichText::to_canonical_json`] renders to bytes. A storage
/// layer embeds this as a nested object (never an escaped string): serializing
/// the returned value with `serde_json` is byte-identical to that JSON
/// (`to_canonical_value(rt).to_string() == rt.to_canonical_json()`), independent
/// of the consumer's `preserve_order` feature. Normalizes a copy first, so the
/// value is canonical whatever the caller's mark/island order.
pub fn to_canonical_value(rt: &RichText) -> Value {
    let mut rt = rt.clone();
    rt.normalize();
    sort_keys_owned(rt.to_value())
}

/// Parse the canonical richtext form from a structural [`Value`], normalize
/// (idempotent), and validate — the [`Value`]-input counterpart to
/// [`RichText::from_canonical_json`]. Returns [`ParseError::Invalid`] for a
/// corpus that violates its invariants, so a storage layer parsing the embedded
/// object rejects a malformed value at load rather than round-tripping it.
pub fn from_canonical_value(v: &Value) -> Result<RichText, ParseError> {
    let mut rt = RichText::from_value(v)?;
    rt.normalize();
    rt.validate().map_err(ParseError::Invalid)?;
    Ok(rt)
}

fn arr<'a>(obj: &'a Map<String, Value>, key: &'static str) -> Result<&'a Vec<Value>, ParseError> {
    obj.get(key)
        .and_then(Value::as_array)
        .ok_or(ParseError::Shape(key))
}

// ---- Line ----

/// Encode a [`LineKind`] into its canonical `kind` fields (`"para"`,
/// `{"kind":"heading","level":n}`, …). Public so the mark/line **op** wire
/// ([`crate::ops`]) reuses the exact discriminant a `RichTextLine` carries,
/// rather than forking the encoding.
pub fn line_kind_to_value(kind: &LineKind) -> Value {
    let mut m = Map::new();
    match kind {
        LineKind::Para => {
            m.insert("kind".into(), "para".into());
        }
        LineKind::Heading { level } => {
            m.insert("kind".into(), "heading".into());
            m.insert("level".into(), Value::from(*level));
        }
        LineKind::Code { lang } => {
            m.insert("kind".into(), "code".into());
            if let Some(l) = lang {
                m.insert("lang".into(), Value::String(l.clone()));
            }
        }
        LineKind::Island => {
            m.insert("kind".into(), "island".into());
        }
        LineKind::Rule => {
            m.insert("kind".into(), "rule".into());
        }
    }
    Value::Object(m)
}

/// Decode a [`LineKind`] from an object carrying the canonical `kind` fields.
/// The inverse of [`line_kind_to_value`]; the shared line-kind reader for
/// [`line_from_value`] and the line-op wire.
pub fn line_kind_from_value(v: &Value) -> Result<LineKind, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("line"))?;
    match o.get("kind").and_then(Value::as_str) {
        Some("para") => Ok(LineKind::Para),
        Some("heading") => {
            let level = o
                .get("level")
                .and_then(Value::as_u64)
                .ok_or(ParseError::Shape("heading level"))?;
            if !(1..=6).contains(&level) {
                return Err(ParseError::Shape("heading level"));
            }
            Ok(LineKind::Heading { level: level as u8 })
        }
        Some("code") => Ok(LineKind::Code {
            lang: o.get("lang").and_then(Value::as_str).map(str::to_string),
        }),
        Some("island") => Ok(LineKind::Island),
        Some("rule") => Ok(LineKind::Rule),
        _ => Err(ParseError::Shape("line kind")),
    }
}

fn line_to_value(line: &Line) -> Value {
    let Value::Object(mut m) = line_kind_to_value(&line.kind) else {
        unreachable!("line_kind_to_value always returns an object")
    };
    m.insert(
        "containers".into(),
        Value::Array(line.containers.iter().map(container_to_value).collect()),
    );
    // Omitted when false (the common case) — deterministic since presence is a
    // pure function of the value.
    if line.continues {
        m.insert("continues".into(), Value::Bool(true));
    }
    Value::Object(m)
}

fn line_from_value(v: &Value) -> Result<Line, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("line"))?;
    let kind = line_kind_from_value(v)?;
    let containers = o
        .get("containers")
        .and_then(Value::as_array)
        .ok_or(ParseError::Shape("containers"))?
        .iter()
        .map(container_from_value)
        .collect::<Result<_, _>>()?;
    let continues = o.get("continues").and_then(Value::as_bool).unwrap_or(false);
    Ok(Line {
        kind,
        containers,
        continues,
    })
}

/// Encode a [`Container`] into its canonical wire object. Public so the line-op
/// wire ([`crate::ops`]) reuses the same container shape a `RichTextLine`
/// carries.
pub fn container_to_value(c: &Container) -> Value {
    let mut m = Map::new();
    match c {
        Container::ListItem {
            ordered,
            start,
            ordinal,
        } => {
            m.insert("container".into(), "list_item".into());
            m.insert("ordered".into(), Value::Bool(*ordered));
            m.insert("start".into(), Value::from(*start));
            m.insert("ordinal".into(), Value::from(*ordinal));
        }
        Container::Quote => {
            m.insert("container".into(), "quote".into());
        }
    }
    Value::Object(m)
}

/// Decode a [`Container`] from its canonical wire object. The inverse of
/// [`container_to_value`].
pub fn container_from_value(v: &Value) -> Result<Container, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("container"))?;
    match o.get("container").and_then(Value::as_str) {
        Some("list_item") => Ok(Container::ListItem {
            ordered: o.get("ordered").and_then(Value::as_bool).unwrap_or(false),
            start: o.get("start").and_then(Value::as_u64).unwrap_or(1),
            ordinal: o.get("ordinal").and_then(Value::as_u64).unwrap_or(0),
        }),
        Some("quote") => Ok(Container::Quote),
        _ => Err(ParseError::Shape("container kind")),
    }
}

// ---- Mark ----

/// Encode a [`Mark`] (`{start, end, type, …}`) into its canonical wire object.
/// Public so the mark-op wire ([`crate::ops`]) reuses the exact `type`
/// discriminant a `RichTextMark` carries.
pub fn mark_to_value(mark: &Mark) -> Value {
    let mut m = Map::new();
    m.insert("start".into(), Value::from(mark.start));
    m.insert("end".into(), Value::from(mark.end));
    match &mark.kind {
        MarkKind::Strong => {
            m.insert("type".into(), "strong".into());
        }
        MarkKind::Emph => {
            m.insert("type".into(), "emph".into());
        }
        MarkKind::Underline => {
            m.insert("type".into(), "underline".into());
        }
        MarkKind::Strike => {
            m.insert("type".into(), "strike".into());
        }
        MarkKind::Code => {
            m.insert("type".into(), "code".into());
        }
        MarkKind::Link { url } => {
            m.insert("type".into(), "link".into());
            m.insert("url".into(), Value::String(url.clone()));
        }
        MarkKind::Anchor { id } => {
            m.insert("type".into(), "anchor".into());
            m.insert("id".into(), Value::String(id.clone()));
        }
        MarkKind::Unknown { tag, attrs } => {
            m.insert("type".into(), Value::String(tag.clone()));
            m.insert("attrs".into(), sorted_value(attrs));
        }
    }
    Value::Object(m)
}

/// Decode a [`Mark`] from its canonical wire object. The inverse of
/// [`mark_to_value`]; the shared mark reader for the corpus decoder and the
/// mark-op wire.
pub fn mark_from_value(v: &Value) -> Result<Mark, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("mark"))?;
    let start = o
        .get("start")
        .and_then(Value::as_u64)
        .ok_or(ParseError::Shape("mark start"))? as usize;
    let end = o
        .get("end")
        .and_then(Value::as_u64)
        .ok_or(ParseError::Shape("mark end"))? as usize;
    let ty = o
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("mark type"))?;
    let kind = match ty {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "underline" => MarkKind::Underline,
        "strike" => MarkKind::Strike,
        "code" => MarkKind::Code,
        "link" => MarkKind::Link {
            url: o
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        "anchor" => MarkKind::Anchor {
            id: o
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        // Open set: any other type name is an unknown mark, round-tripped opaque
        // with whatever `attrs` it carried.
        other => MarkKind::Unknown {
            tag: other.to_string(),
            attrs: o.get("attrs").cloned().unwrap_or(Value::Null),
        },
    };
    Ok(Mark { start, end, kind })
}

// ---- Table cell {text, marks} ----
//
// A pipe-table cell is inline-only: its own plain `text` plus `marks` whose
// ranges are USV offsets into that text (0..cell_len). The marks ride the SAME
// wire shape prose marks use (`mark_to_value`/`mark_from_value`), so nothing
// forks the encoding. Import builds cells, export/emit render them, and
// `RichText::normalize`/`validate` canonicalize/check the marks — all through
// these helpers.

/// Parse a table-cell object `{text, marks}` leniently: its plain text plus the
/// marks over it. A malformed mark is skipped rather than failing — cells are
/// flat inline, so this never recurses. Public so the typst emitter renders a
/// cell through the same parse the codecs use.
pub fn parse_cell(v: &Value) -> (String, Vec<Mark>) {
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let marks = v
        .get("marks")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|m| mark_from_value(m).ok()).collect())
        .unwrap_or_default();
    (text, marks)
}

/// Build a table-cell object `{text, marks}` — the inverse of [`parse_cell`],
/// reusing [`mark_to_value`]. Key order is fixed by the recursive
/// [`sorted_value`] pass in [`RichText::normalize`], not here.
pub(crate) fn cell_to_value(text: &str, marks: &[Mark]) -> Value {
    let mut m = Map::new();
    m.insert("text".into(), Value::String(text.to_string()));
    m.insert(
        "marks".into(),
        Value::Array(marks.iter().map(mark_to_value).collect()),
    );
    Value::Object(m)
}

/// Every cell's `(text, marks)` in a table island's props — header then each
/// body row, in order. For [`RichText::validate`]'s cell-mark invariant checks.
pub(crate) fn table_cells(props: &Value) -> Vec<(String, Vec<Mark>)> {
    let mut out = Vec::new();
    if let Some(h) = props.get("header").and_then(Value::as_array) {
        out.extend(h.iter().map(parse_cell));
    }
    if let Some(rows) = props.get("rows").and_then(Value::as_array) {
        for row in rows {
            if let Some(r) = row.as_array() {
                out.extend(r.iter().map(parse_cell));
            }
        }
    }
    out
}

/// Islands whose props carry inline `{text, marks}` cells — the set that
/// participates in mark normalization and cell-mark validation. Adding a
/// mark-carrying island type is a single edit here;
/// [`normalize_island_cell_marks`] and [`island_cell_marks`] both route through
/// it, so neither `normalize` nor `validate` can silently skip a new type
/// (which would void the canonical-bytes guarantee for its cells).
fn island_is_mark_carrying(island_type: &str) -> bool {
    matches!(island_type, "table")
}

/// Repair a mark-carrying island's structure in place (a no-op for a
/// non-mark-carrying type) — the normalize-side island-type dispatch, kept
/// beside the table codec rather than as a bare `"table"` match in `model`.
pub(crate) fn normalize_island_structure(island: &mut Island) {
    if island_is_mark_carrying(&island.island_type) {
        normalize_table_props(&mut island.props);
    }
}

/// A mark-carrying island's `(text, marks)` cells for validation (empty for a
/// non-mark-carrying type) — the validate-side twin of
/// [`normalize_island_structure`].
pub(crate) fn island_cell_marks(island: &Island) -> Vec<(String, Vec<Mark>)> {
    if island_is_mark_carrying(&island.island_type) {
        table_cells(&island.props)
    } else {
        Vec::new()
    }
}

/// A mark-carrying island's shape violation, if any (`None` for a well-formed
/// or non-mark-carrying island) — the validate-side twin of the shape repair in
/// [`normalize_island_structure`]. `normalize` guarantees this returns `None`.
pub(crate) fn island_shape_error(island: &Island) -> Option<Invariant> {
    if island_is_mark_carrying(&island.island_type) {
        table_shape_error(&island.props)
    } else {
        None
    }
}

/// Repair a table island's props in place to the canonical shape:
///
/// - **One column count.** `cols` is the widest of the header, any body row, and
///   `aligns`; the header, each row, and `aligns` are padded up to it (padding
///   only grows — no cell is ever truncated). Materializing the count into the
///   header means the markdown projection (header-derived) and the Typst
///   projection (widest-row) agree on one number.
/// - **Single-line cells.** Any `\n`/`\r` in a cell's text becomes a space (the
///   same rule import applies to soft/hard breaks). A 1:1 replacement keeps char
///   offsets stable, so the cell's marks stay in range.
/// - **Canonical cell marks.** Each cell's marks are re-normalized (sort,
///   same-kind union, drop zero-width) so equal cells serialize to equal bytes.
fn normalize_table_props(props: &mut Value) {
    let cols = table_cols(props);
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let header = obj.entry("header").or_insert_with(|| Value::Array(vec![]));
    // A non-array header (a bare string, say) carries no cells; rewrite it to an
    // empty array so it canonicalizes to a zero-column, content-free table
    // rather than retaining opaque garbage that `validate` would then reject.
    if !header.is_array() {
        *header = Value::Array(vec![]);
    }
    pad_row(header, cols);
    if let Some(h) = header.as_array_mut() {
        h.iter_mut().for_each(canon_cell);
    }
    let aligns = obj.entry("aligns").or_insert_with(|| Value::Array(vec![]));
    if let Some(a) = aligns.as_array_mut() {
        while a.len() < cols {
            a.push(Value::String("none".into()));
        }
    }
    if let Some(rows) = obj.get_mut("rows").and_then(Value::as_array_mut) {
        for row in rows.iter_mut() {
            pad_row(row, cols);
            if let Some(r) = row.as_array_mut() {
                r.iter_mut().for_each(canon_cell);
            }
        }
    }
}

/// A table's canonical column count: the widest of its header, any body row, and
/// its `aligns` array. Padding (never truncation) brings every part up to it.
fn table_cols(props: &Value) -> usize {
    let arr_len = |k: &str| props.get(k).and_then(Value::as_array).map(|a| a.len());
    let header = arr_len("header").unwrap_or(0);
    let aligns = arr_len("aligns").unwrap_or(0);
    let widest_row = props
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|r| r.as_array().map(|a| a.len()).unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    header.max(aligns).max(widest_row)
}

/// Pad a cell array (header or body row) up to `cols` with empty cells. Never
/// shrinks — `cols` is the widest, so a shorter array only grows.
fn pad_row(v: &mut Value, cols: usize) {
    if let Some(arr) = v.as_array_mut() {
        while arr.len() < cols {
            arr.push(cell_to_value("", &[]));
        }
    }
}

/// De-newline a cell's text (each `\n`/`\r` → a space, 1:1 so mark offsets hold)
/// and re-normalize its marks. Reached per-cell from [`normalize_table_props`].
fn canon_cell(cell: &mut Value) {
    let (text, marks) = parse_cell(cell);
    let text = if text.contains(['\n', '\r']) {
        text.replace(['\n', '\r'], " ")
    } else {
        text
    };
    *cell = cell_to_value(&text, &crate::model::normalize_marks(marks));
}

/// A table island's shape violation, if any — the widths the header, `aligns`,
/// and each body row must share (the header width), plus the `\n`-free-cell rule.
/// The validate-side twin of [`normalize_table_props`].
fn table_shape_error(props: &Value) -> Option<Invariant> {
    // A present-but-non-array header can't carry column cells — `normalize`
    // rewrites it to an empty array, so an un-normalized one is a hand-built
    // degenerate island. (An absent header is a zero-column table, which is
    // well-formed: `empty_table_is_valid`.)
    if props.get("header").is_some_and(|h| !h.is_array()) {
        return Some(Invariant::TableHeaderNotArray);
    }
    let cols = props
        .get("header")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let aligns = props
        .get("aligns")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    if aligns != cols {
        return Some(Invariant::TableAlignsMismatch { aligns, cols });
    }
    if let Some(rows) = props.get("rows").and_then(Value::as_array) {
        for (i, row) in rows.iter().enumerate() {
            let width = row.as_array().map(|a| a.len()).unwrap_or(0);
            if width != cols {
                return Some(Invariant::TableRaggedRow {
                    row: i,
                    width,
                    cols,
                });
            }
        }
    }
    for (i, (text, _)) in table_cells(props).iter().enumerate() {
        if text.contains('\n') || text.contains('\r') {
            return Some(Invariant::TableCellNewline { cell: i });
        }
    }
    None
}

// ---- Island ----

fn island_to_value(island: &Island) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(island.id.clone()));
    m.insert("type".into(), Value::String(island.island_type.clone()));
    m.insert("props".into(), sorted_value(&island.props));
    m.insert("loss".into(), loss_to_str(island.loss).into());
    Value::Object(m)
}

fn island_from_value(v: &Value) -> Result<Island, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("island"))?;
    Ok(Island {
        id: o
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("island id"))?
            .to_string(),
        island_type: o
            .get("type")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("island type"))?
            .to_string(),
        props: o.get("props").cloned().unwrap_or(Value::Null),
        loss: loss_from_str(o.get("loss").and_then(Value::as_str).unwrap_or("lossless")),
    })
}

fn loss_to_str(loss: Loss) -> &'static str {
    match loss {
        Loss::Lossless => "lossless",
        Loss::Degraded => "degraded",
        Loss::Unrepresentable => "unrepresentable",
    }
}

fn loss_from_str(s: &str) -> Loss {
    match s {
        "lossless" => Loss::Lossless,
        "degraded" => Loss::Degraded,
        // Unknown/future loss class defaults to the *safe* end: never claim a
        // value the reader can't interpret "carries faithfully".
        _ => Loss::Unrepresentable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Line, LineKind};

    fn sample() -> RichText {
        RichText {
            text: "hello world".into(),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: vec![],
                continues: false,
            }],
            marks: vec![
                Mark {
                    start: 6,
                    end: 11,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 0,
                    end: 5,
                    kind: MarkKind::Emph,
                },
            ],
            islands: vec![],
        }
    }

    #[test]
    fn island_props_key_order_does_not_leak() {
        let mut one = RichText::empty();
        one.text = "\u{FFFC}".into();
        one.lines = vec![Line {
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }];
        one.islands = vec![Island {
            id: "i1".into(),
            island_type: "table".into(),
            props: serde_json::json!({"b": 1, "a": 2}),
            loss: Loss::Lossless,
        }];
        let mut two = one.clone();
        two.islands[0].props = serde_json::json!({"a": 2, "b": 1}); // keys reversed
        assert_eq!(one.to_canonical_json(), two.to_canonical_json());
    }

    #[test]
    fn golden_bytes_are_feature_independent() {
        // Pins the exact canonical form. Every object key is sorted, so the
        // bytes do not depend on serde_json's preserve_order feature. If this
        // string changes, the freeze changed — bump the schema version.
        let rt = sample();
        assert_eq!(
            rt.to_canonical_json(),
            r#"{"islands":[],"lines":[{"containers":[],"kind":"para"}],"marks":[{"end":5,"start":0,"type":"emph"},{"end":11,"start":6,"type":"strong"}],"text":"hello world"}"#
        );
    }

    #[test]
    fn from_canonical_json_rejects_invalid() {
        // lines.len() != segment count — must not silently round-trip.
        let bad =
            r#"{"text":"a\nb","lines":[{"kind":"para","containers":[]}],"marks":[],"islands":[]}"#;
        assert!(matches!(
            RichText::from_canonical_json(bad),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn reserved_unknown_tag_rejected() {
        // An Unknown mark may not reuse a built-in type name (would parse back
        // as the built-in, dropping attrs — non-injective).
        let mut rt = RichText::empty();
        rt.text = "abcd".into();
        rt.marks = vec![Mark {
            start: 0,
            end: 4,
            kind: MarkKind::Unknown {
                tag: "strong".into(),
                attrs: serde_json::json!({}),
            },
        }];
        assert!(matches!(
            rt.validate(),
            Err(crate::model::Invariant::ReservedUnknownTag(_))
        ));
    }

    #[test]
    fn unknown_loss_class_defaults_unrepresentable() {
        let json = r#"{"text":"￼","lines":[{"kind":"island","containers":[]}],"marks":[],"islands":[{"id":"i1","type":"widget","props":{},"loss":"future_class"}]}"#;
        let rt = RichText::from_canonical_json(json).unwrap();
        assert_eq!(rt.islands[0].loss, Loss::Unrepresentable);
    }

    #[test]
    fn unknown_mark_round_trips_opaque() {
        let mut rt = RichText::empty();
        rt.text = "abcd".into();
        rt.marks = vec![Mark {
            start: 0,
            end: 4,
            kind: MarkKind::Unknown {
                tag: "highlight".into(),
                attrs: serde_json::json!({"color": "yellow"}),
            },
        }];
        let json = rt.to_canonical_json();
        let back = RichText::from_canonical_json(&json).unwrap();
        assert_eq!(back.marks[0].kind, rt.marks[0].kind);
    }
}
