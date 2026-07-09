//! Canonical JSON serialization — the phase-1 freeze.
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
    sorted_value, Container, Island, Line, LineKind, Loss, Mark, MarkKind, RichText,
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
/// key-sorted tree [`content_key`] renders to bytes. A storage layer embeds
/// this as a nested object (never an escaped string): serializing the returned
/// value with `serde_json` is byte-identical to [`content_key`]
/// (`to_canonical_value(rt).to_string() == content_key(rt)`), independent of the
/// consumer's `preserve_order` feature. Normalizes a copy first, so the value is
/// canonical whatever the caller's mark/island order.
pub fn to_canonical_value(rt: &RichText) -> Value {
    let mut rt = rt.clone();
    rt.normalize();
    sorted_value(&rt.to_value())
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

fn line_to_value(line: &Line) -> Value {
    let mut m = Map::new();
    match &line.kind {
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
    let kind = match o.get("kind").and_then(Value::as_str) {
        Some("para") => LineKind::Para,
        Some("heading") => LineKind::Heading {
            level: o
                .get("level")
                .and_then(Value::as_u64)
                .ok_or(ParseError::Shape("heading level"))? as u8,
        },
        Some("code") => LineKind::Code {
            lang: o.get("lang").and_then(Value::as_str).map(str::to_string),
        },
        Some("island") => LineKind::Island,
        Some("rule") => LineKind::Rule,
        _ => return Err(ParseError::Shape("line kind")),
    };
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

fn container_to_value(c: &Container) -> Value {
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

fn container_from_value(v: &Value) -> Result<Container, ParseError> {
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

fn mark_to_value(mark: &Mark) -> Value {
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

fn mark_from_value(v: &Value) -> Result<Mark, ParseError> {
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

/// Canonicalize a table island's cell marks in place: re-run mark normalization
/// (sort, same-kind union, drop zero-width) on each cell so equal cells
/// serialize to equal bytes. Cells hold no `\n`, so there is no line-boundary
/// edge-trim. Called by [`RichText::normalize`] for `type=="table"` islands.
pub(crate) fn normalize_table_cell_marks(props: &mut Value) {
    fn canon(cell: &mut Value) {
        let (text, marks) = parse_cell(cell);
        *cell = cell_to_value(&text, &crate::model::normalize_marks(marks));
    }
    if let Some(h) = props.get_mut("header").and_then(Value::as_array_mut) {
        h.iter_mut().for_each(canon);
    }
    if let Some(rows) = props.get_mut("rows").and_then(Value::as_array_mut) {
        for row in rows.iter_mut() {
            if let Some(r) = row.as_array_mut() {
                r.iter_mut().for_each(canon);
            }
        }
    }
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

/// Content-hash key: the canonical JSON, byte-for-byte. Exposed so a phase-2
/// storage layer hashes the same bytes it serializes. `_` prefix-free name kept
/// stable — part of the determinism contract.
pub fn content_key(rt: &RichText) -> String {
    rt.to_canonical_json()
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
    fn round_trips_and_is_fixed_point() {
        let rt = sample();
        let json = rt.to_canonical_json();
        let back = RichText::from_canonical_json(&json).unwrap();
        // Re-serializing the parsed value yields identical bytes (fixed point).
        assert_eq!(back.to_canonical_json(), json);
        assert_eq!(back.validate(), Ok(()));
    }

    #[test]
    fn byte_deterministic_regardless_of_input_order() {
        let a = sample();
        let mut b = sample();
        b.marks.reverse(); // different discovery order
        assert_eq!(a.to_canonical_json(), b.to_canonical_json());
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
