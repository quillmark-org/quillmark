//! Canonical document-model paths.
//!
//! [`DocPath`] is the workspace's one serializer and parser for
//! [`Diagnostic::path`](crate::error::Diagnostic::path) — the anchor into a
//! typed [`Document`](crate::document::Document). Every emit site (schema
//! validation, `!must_fill` collection, coercion) constructs a `DocPath` and
//! renders it once through [`Display`](std::fmt::Display); no site assembles a path with
//! `format!`, and no consumer regexes one back apart — the exported
//! [`FromStr`] parser is the inverse.
//!
//! # Grammar
//!
//! ```text
//! path   := root? segment*
//! root   := "main"                         // only as the main body head
//!         | "cards" "." kind "[" index "]"  // typed card
//!         | "cards" "[" index "]"           // unknown-kind card (the only bare-index root)
//! segment:= "." field | "[" index "]" | ".body"
//! kind   := [a-z_][a-z0-9_]*
//! field  := [A-Za-z_][A-Za-z0-9_]*
//! ```
//!
//! A bare main field carries no root (`recipient`, `recipients[0].name`); the
//! main body is `main.body`. A card field is kind-qualified —
//! `cards.<kind>[<i>].<field>` — so a consumer receives kind and array index
//! without a second lookup; a card whose `$kind` has no schema stays
//! `cards[<i>]`. Field names and card kinds exclude `.`, `[`, `]`, so the
//! rendered form round-trips.
//!
//! `cards` and `main` are reserved roots and `body` a reserved terminal: a
//! main field literally named `cards`/`main`, or a scalar field named `body`,
//! collides with a root/terminal and does not round-trip. No fixture field
//! uses these names; the collision is accepted, not guarded.
//!
//! This is the **document-model** namespace, distinct from the plate-JSON
//! `data.$cards` array template authors see (`prose/canon/CARDS.md`): sigiled
//! `$cards` is glue delivered to the backend, unsigiled `cards` is a path into
//! the document. Config-space anchors (`$seed.<kind>.<field>`, Quill.yaml
//! schema-literal owner labels) ride the same serializer with their prefix as
//! a leading [`field`](DocPath::field) segment — verbatim, never parsed.

use crate::value::PathSegment;
use std::fmt;
use std::str::FromStr;

/// One segment of a [`DocPath`].
///
/// Serde-tagged (`{ "seg": "field", "name": "x" }`) so the WASM parser hands
/// the editor a structured array it routes on, never a string it splits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "seg", rename_all = "lowercase")]
pub enum DocSeg {
    /// The main card — only ever the head of `main.body`; a bare main field
    /// carries no root segment.
    Main,
    /// A composable card by document-array index. `kind: None` is the
    /// unknown-kind whole-card form (`cards[<i>]`), the only bare-index root.
    Card { kind: Option<String>, index: usize },
    /// An object field or map key.
    Field { name: String },
    /// An array index.
    Index { index: usize },
    /// A card or main body (`.body`), always terminal.
    Body,
}

/// A canonical document-model path — an ordered [`DocSeg`] list with one
/// [`Display`](std::fmt::Display) serializer and one [`FromStr`] parser. See the [module
/// docs](self) for the grammar.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DocPath {
    segs: Vec<DocSeg>,
}

impl DocPath {
    /// The empty path — the base a bare main field extends (`title`).
    pub fn new() -> Self {
        Self::default()
    }

    /// The main body anchor, `main.body`.
    pub fn main_body() -> Self {
        Self {
            segs: vec![DocSeg::Main, DocSeg::Body],
        }
    }

    /// A composable card root. `kind: None` is the unknown-kind whole-card
    /// form `cards[<i>]`; `Some(k)` is `cards.<k>[<i>]`.
    pub fn card(kind: Option<&str>, index: usize) -> Self {
        Self {
            segs: vec![DocSeg::Card {
                kind: kind.map(str::to_owned),
                index,
            }],
        }
    }

    /// This path extended by a field segment. The name is stored verbatim —
    /// callers pass validated field names, or a config-space prefix
    /// (`$seed.<kind>`) as an opaque head.
    pub fn field(&self, name: &str) -> Self {
        self.pushing(DocSeg::Field {
            name: name.to_owned(),
        })
    }

    /// This path extended by an array index segment.
    pub fn index(&self, index: usize) -> Self {
        self.pushing(DocSeg::Index { index })
    }

    /// This path extended by the terminal body segment.
    pub fn body(&self) -> Self {
        self.pushing(DocSeg::Body)
    }

    /// This path extended by a value-relative [`PathSegment`] — the bridge
    /// from the value-tree walk (`!must_fill` collection): [`Key`] becomes a
    /// field, [`Index`] an index.
    ///
    /// [`Key`]: PathSegment::Key
    /// [`Index`]: PathSegment::Index
    pub fn segment(&self, seg: &PathSegment) -> Self {
        match seg {
            PathSegment::Key(k) => self.field(k),
            PathSegment::Index(i) => self.index(*i),
        }
    }

    /// The segments, head first.
    pub fn segs(&self) -> &[DocSeg] {
        &self.segs
    }

    fn pushing(&self, seg: DocSeg) -> Self {
        let mut segs = self.segs.clone();
        segs.push(seg);
        Self { segs }
    }
}

impl fmt::Display for DocPath {
    /// The one document-model path serializer. A `Field` takes a leading `.`
    /// unless it heads the path; `Index` and `Body` never do; the card and
    /// main roots are self-contained heads.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.segs.iter().enumerate() {
            match seg {
                DocSeg::Main => f.write_str("main")?,
                DocSeg::Card { kind: Some(k), index } => write!(f, "cards.{k}[{index}]")?,
                DocSeg::Card { kind: None, index } => write!(f, "cards[{index}]")?,
                DocSeg::Field { name } => {
                    if i != 0 {
                        f.write_str(".")?;
                    }
                    f.write_str(name)?;
                }
                DocSeg::Index { index } => write!(f, "[{index}]")?,
                DocSeg::Body => f.write_str(".body")?,
            }
        }
        Ok(())
    }
}

/// A [`DocPath`] parse failure. Carries the offending input for a diagnostic
/// message; the parser is total over every path [`Display`](std::fmt::Display) emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocPathParseError {
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for DocPathParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid document path '{}': {}", self.input, self.reason)
    }
}

impl std::error::Error for DocPathParseError {}

impl FromStr for DocPath {
    type Err = DocPathParseError;

    /// The inverse of [`Display`](std::fmt::Display), total over every emitted path. `main.body`
    /// is the main body; a `cards`-headed shape matching a card root becomes a
    /// [`Card`](DocSeg::Card); a trailing `.body` under a root is [`Body`](DocSeg::Body);
    /// everything else is a bare field chain (`recipients[0].name`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |reason: &'static str| DocPathParseError {
            input: s.to_owned(),
            reason,
        };
        if s.is_empty() {
            return Err(err("empty path"));
        }

        // The head word scans as a `Field`; a `main`/`cards` head is reclassed
        // into its root below, otherwise it stays the field it names.
        let segs = scan(s).map_err(err)?;

        // `main.body` is the sole `main`-headed form.
        if let [DocSeg::Field { name: a }, DocSeg::Field { name: b }] = segs.as_slice() {
            if a == "main" && b == "body" {
                return Ok(DocPath::main_body());
            }
        }

        // A `cards` head that matches a card-root shape is a Card; the tail
        // (a lone `body`, or fields/indices) follows. A `cards` word that does
        // not fit — no index — is an ordinary field named `cards`.
        if matches!(segs.first(), Some(DocSeg::Field { name }) if name == "cards") {
            if let Some((card, rest)) = parse_card_root(&segs) {
                let mut segs = vec![card];
                segs.extend(tail_segs(rest));
                return Ok(DocPath { segs });
            }
        }

        // A bare field chain: the scanned segments already stand.
        Ok(DocPath { segs })
    }
}

/// Scan a path into segments: a leading word, then a run of `.word` (a `Field`)
/// or `[index]` (an `Index`). Root/terminal words (`main`/`cards`/`body`) scan
/// as fields and are reclassed by the caller. The round-trip charsets are
/// enforced here only as "no empty word, digits inside brackets".
fn scan(s: &str) -> Result<Vec<DocSeg>, &'static str> {
    let mut segs = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    // Head word (paths never open with `.` or `[`).
    if bytes[0] == b'.' || bytes[0] == b'[' {
        return Err("path must start with a name");
    }
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                let end = s[i..].find(']').map(|o| i + o).ok_or("unclosed '['")?;
                let digits = &s[i + 1..end];
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("index is not a number");
                }
                let index = digits.parse().map_err(|_| "index out of range")?;
                segs.push(DocSeg::Index { index });
                i = end + 1;
            }
            b'.' => {
                let start = i + 1;
                i = word_end(bytes, start);
                if i == start {
                    return Err("empty segment after '.'");
                }
                segs.push(DocSeg::Field { name: s[start..i].to_owned() });
            }
            _ => {
                let start = i;
                i = word_end(bytes, start);
                segs.push(DocSeg::Field { name: s[start..i].to_owned() });
            }
        }
    }
    Ok(segs)
}

/// The index just past a word — the run up to the next `.` or `[`.
fn word_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
        i += 1;
    }
    i
}

/// Match a `cards` head against the two card-root shapes, returning the root
/// segment and the remaining segments. `None` when the shape does not fit —
/// then `cards` is a field, not a root.
fn parse_card_root(segs: &[DocSeg]) -> Option<(DocSeg, &[DocSeg])> {
    match segs {
        // cards[<i>] …
        [DocSeg::Field { .. }, DocSeg::Index { index }, rest @ ..] => {
            Some((DocSeg::Card { kind: None, index: *index }, rest))
        }
        // cards.<kind>[<i>] …
        [DocSeg::Field { .. }, DocSeg::Field { name: kind }, DocSeg::Index { index }, rest @ ..] => {
            Some((
                DocSeg::Card {
                    kind: Some(kind.clone()),
                    index: *index,
                },
                rest,
            ))
        }
        _ => None,
    }
}

/// A card-root tail: a lone `body` is the card body; otherwise the scanned
/// field/index chain stands (`.signature_block`, `.recipients[0].name`).
fn tail_segs(rest: &[DocSeg]) -> Vec<DocSeg> {
    match rest {
        [DocSeg::Field { name }] if name == "body" => vec![DocSeg::Body],
        _ => rest.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every form [`Display`](std::fmt::Display) emits round-trips through [`FromStr`].
    fn round_trip(path: DocPath, rendered: &str) {
        assert_eq!(path.to_string(), rendered, "serialize");
        assert_eq!(
            rendered.parse::<DocPath>().expect("parse"),
            path,
            "parse back"
        );
    }

    #[test]
    fn main_field_and_nested() {
        round_trip(DocPath::new().field("title"), "title");
        round_trip(
            DocPath::new().field("recipients").index(0).field("name"),
            "recipients[0].name",
        );
    }

    #[test]
    fn main_body() {
        round_trip(DocPath::main_body(), "main.body");
    }

    #[test]
    fn card_roots() {
        round_trip(DocPath::card(Some("indorsement"), 0), "cards.indorsement[0]");
        round_trip(DocPath::card(None, 3), "cards[3]");
    }

    #[test]
    fn card_field_and_body() {
        round_trip(
            DocPath::card(Some("indorsement"), 0).field("signature_block"),
            "cards.indorsement[0].signature_block",
        );
        round_trip(
            DocPath::card(Some("skills"), 2).body(),
            "cards.skills[2].body",
        );
        round_trip(
            DocPath::card(Some("indorsement"), 0)
                .field("recipients")
                .index(1)
                .field("name"),
            "cards.indorsement[0].recipients[1].name",
        );
    }

    #[test]
    fn body_and_main_are_reserved_only_at_the_edges() {
        // A non-terminal `body` under a card is an ordinary field named body.
        round_trip(
            DocPath::card(Some("k"), 0).field("body").field("x"),
            "cards.k[0].body.x",
        );
        // A `main`-headed chain that is not `main.body` is a field chain.
        round_trip(DocPath::new().field("main").field("x"), "main.x");
        round_trip(DocPath::new().field("main"), "main");
    }

    #[test]
    fn cards_as_a_field_name_when_no_index_follows() {
        // `cards.foo` has no index → not a card root → a field chain.
        round_trip(DocPath::new().field("cards").field("foo"), "cards.foo");
    }

    #[test]
    fn segment_bridge() {
        let base = DocPath::card(Some("k"), 0);
        assert_eq!(
            base.segment(&PathSegment::Key("addr".into()))
                .segment(&PathSegment::Index(2))
                .to_string(),
            "cards.k[0].addr[2]",
        );
    }

    #[test]
    fn parse_rejects_malformed() {
        for bad in ["", ".foo", "[0]", "foo[", "foo[a]", "foo[]", "a..b", "a."] {
            assert!(bad.parse::<DocPath>().is_err(), "expected error for {bad:?}");
        }
    }

    #[test]
    fn serde_round_trips_as_tagged_array() {
        let path = DocPath::card(Some("indorsement"), 0).field("sig");
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(
            json,
            r#"[{"seg":"card","kind":"indorsement","index":0},{"seg":"field","name":"sig"}]"#
        );
        assert_eq!(serde_json::from_str::<DocPath>(&json).unwrap(), path);
    }
}
