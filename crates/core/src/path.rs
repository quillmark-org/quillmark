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
//! path   := root segment*
//! root   := "main"                          // the main card
//!         | "cards" "." kind "[" index "]"   // typed card
//!         | "cards" "[" index "]"            // unknown-kind card (the only bare-index root)
//! segment:= "." field | "[" index "]" | ".body"
//! kind   := [a-z_][a-z0-9_]*
//! field  := [A-Za-z_][A-Za-z0-9_]*
//! ```
//!
//! Every document-model path is **rooted**: a main field is `main.<field>`
//! (`main.title`, `main.recipients[0].name`), the main body `main.body`. A card
//! field is kind-qualified — `cards.<kind>[<i>].<field>` — so a consumer
//! receives kind and array index without a second lookup; a card whose `$kind`
//! has no schema — absent, or present but not a declared card kind — stays
//! `cards[<i>]`. Field names and card kinds exclude `.`, `[`, `]`, so the
//! rendered form round-trips.
//!
//! Rooting makes the grammar total against a field named for a root: a main
//! field literally named `cards` or `main` is `main.cards` / `main.main`, which
//! no longer collides. One residual: a field literally named `body` renders
//! `<root>.body` and collides with the body terminal — accepted, not guarded (no
//! fixture field uses the name).
//!
//! This is the **document-model** namespace, distinct from the plate-JSON
//! `data.$cards` array template authors see (`prose/canon/CARDS.md`): sigiled
//! `$cards` is glue delivered to the backend, unsigiled `cards` is a path into
//! the document. Config-space anchors (`$seed.<kind>.<field>`, Quill.yaml
//! schema-literal owner labels) ride the same serializer with their prefix as a
//! leading [`field`](DocPath::field) segment — the one **unrooted** form,
//! config-space not document-model, verbatim and never parsed.

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
    /// The main-card root — heads every main-card address (`main.title`,
    /// `main.body`).
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
    /// The empty base for a config-space / opaque-prefix path (`$seed.<kind>`, a
    /// Quill.yaml schema-literal owner label) — the one unrooted form, not a
    /// document-model address. A document-model path roots at [`main`](Self::main)
    /// or [`card`](Self::card).
    pub fn new() -> Self {
        Self::default()
    }

    /// The main-card root, `main` — the base every main-card address extends
    /// (`main.title`, `main.recipients[0].name`, `main.body`).
    pub fn main() -> Self {
        Self {
            segs: vec![DocSeg::Main],
        }
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

    /// The inverse of [`Display`](std::fmt::Display), total over every emitted path. A
    /// `main` head is the main root — `main.body` the body, `main` alone the bare
    /// root, otherwise a main field chain; a `cards`-headed shape matching a card
    /// root becomes a [`Card`](DocSeg::Card); a trailing `.body` under a root is
    /// [`Body`](DocSeg::Body); an unrooted chain is a config-space anchor
    /// (`$seed.<kind>`).
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

        // A `main` head is the main root. `main.body` is the body; `main` alone
        // the bare root; otherwise a main field chain (`main.recipients[0].name`).
        // A main field literally named `body` renders `main.body` and reads back
        // as the body — the accepted residual collision.
        if matches!(segs.first(), Some(DocSeg::Field { name }) if name == "main") {
            let rest = &segs[1..];
            if matches!(rest, [DocSeg::Field { name }] if name == "body") {
                return Ok(DocPath::main_body());
            }
            let mut out = vec![DocSeg::Main];
            out.extend_from_slice(rest);
            return Ok(DocPath { segs: out });
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

        // An unrooted field chain — a config-space anchor (`$seed.<kind>`, an
        // owner label), never a document-model address.
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
        round_trip(DocPath::main(), "main");
        round_trip(DocPath::main().field("title"), "main.title");
        round_trip(
            DocPath::main().field("recipients").index(0).field("name"),
            "main.recipients[0].name",
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
    fn body_is_reserved_only_as_a_root_terminal() {
        // A non-terminal `body` under a card is an ordinary field named body.
        round_trip(
            DocPath::card(Some("k"), 0).field("body").field("x"),
            "cards.k[0].body.x",
        );
        // A main field chain that is not `main.body` roots at `main`.
        round_trip(DocPath::main().field("x"), "main.x");
    }

    #[test]
    fn main_field_named_for_a_root_no_longer_collides() {
        // Rooting makes `cards` / `main` field names total — they were the
        // bare-form collisions the old grammar could not round-trip.
        round_trip(DocPath::main().field("cards"), "main.cards");
        round_trip(DocPath::main().field("main"), "main.main");
        // A bare `cards.foo` (no index) is a config-space chain, not a card.
        round_trip(DocPath::new().field("cards").field("foo"), "cards.foo");
    }

    #[test]
    fn config_space_anchor_is_the_unrooted_form() {
        // Config-space paths (`$seed` overlays, owner labels) are the one
        // unrooted shape — a leading field, never reclassed to a root.
        round_trip(
            DocPath::new()
                .field("$seed")
                .field("indorsement")
                .field("author"),
            "$seed.indorsement.author",
        );
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
