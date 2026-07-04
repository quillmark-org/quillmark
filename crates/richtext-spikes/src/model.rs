//! The throwaway `RichText` prototype: corpus + lines + marks + islands over a
//! single Unicode-scalar-value coordinate space. Deliberately minimal — enough
//! to exercise the three Phase-0 questions (mark semantics, source-map
//! inversion, seam determinism), not the shipped model. See #831 for the real
//! shape; see `prose/plans/richtext/phase-0-*` for the findings this backs.

use serde::{Deserialize, Serialize};

/// The island slot sentinel: one `U+FFFC OBJECT REPLACEMENT CHARACTER` per
/// island, occupying a single character position in the corpus.
pub const ISLAND_SLOT: char = '\u{FFFC}';

/// A Unicode-scalar-value offset into [`RichText::text`]. Rust `char` indexing
/// is already USV, so a "corpus position" is a `char` count — never a byte
/// count (multibyte-unsafe) and never a UTF-16 unit (the JS-editor tax lives at
/// the binding boundary, see [`crate::usv`]).
pub type Usv = usize;

/// A half-open `[start, end)` char range in the corpus. `start == end` is a
/// zero-width point (an anchor/comment identity mark lives here).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharRange {
    pub start: Usv,
    pub end: Usv,
}

impl CharRange {
    pub fn new(start: Usv, end: Usv) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
    pub fn len(&self) -> usize {
        self.end - self.start
    }
}

/// The mark set frozen by this spike (Spike A). `strong`/`emph`/`underline`/
/// `strike`/`code`/`link` are the round-trippable projection marks; `anchor` is
/// the zero-width identity mark that carries comments/links-to and rebases
/// across edits like any position. The set is **open**: an unknown kind would
/// round-trip opaque (not modelled here — the spike only needs the closed
/// core).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarkKind {
    Strong,
    Emph,
    Underline,
    Strike,
    Code,
    Link { url: String },
    /// Zero-width identity mark. `id` names a comment thread or a stable anchor.
    Anchor { id: String },
}

impl MarkKind {
    /// Canonical ordering discriminant for deterministic serialization.
    fn ord(&self) -> u8 {
        match self {
            MarkKind::Strong => 0,
            MarkKind::Emph => 1,
            MarkKind::Underline => 2,
            MarkKind::Strike => 3,
            MarkKind::Code => 4,
            MarkKind::Link { .. } => 5,
            MarkKind::Anchor { .. } => 6,
        }
    }
    /// Whether two marks of this kind may merge when adjacent/overlapping.
    /// Formatting marks merge (union); identity marks never do (two comments
    /// are two comments even over the same text).
    pub fn merges(&self) -> bool {
        !matches!(self, MarkKind::Anchor { .. })
    }
}

/// A mark over a corpus char range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub range: CharRange,
    #[serde(flatten)]
    pub kind: MarkKind,
}

/// Line-level structure. The tree is *derived* from `text`'s `\n` boundaries,
/// never stored as nesting; `containers` is the ancestor path so a
/// multi-paragraph list item is two `para` lines sharing a `ListItem`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    #[serde(flatten)]
    pub kind: LineKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Container>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "line", rename_all = "snake_case")]
pub enum LineKind {
    Para,
    Heading { level: u8 },
    Code { lang: Option<String> },
    Island,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "container", rename_all = "snake_case")]
pub enum Container {
    ListItem { ordered: bool },
    Quote,
}

/// How faithfully an island survives the markdown projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Loss {
    Lossless,
    Degraded,
    Unrepresentable,
}

/// A structured object with no honest text encoding, occupying one
/// [`ISLAND_SLOT`] position. `id` is minted at creation (the only
/// nondeterminism source — see Spike C).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Island {
    pub id: String,
    #[serde(rename = "type")]
    pub island_type: String,
    pub props: serde_json::Value,
    pub loss: Loss,
}

/// One `RichText` per content field. See module docs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichText {
    pub text: String,
    pub lines: Vec<Line>,
    pub marks: Vec<Mark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub islands: Vec<Island>,
}

impl RichText {
    /// Corpus length in USV (chars), the coordinate space marks index into.
    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    /// Put marks into canonical order: by `(start, end, kind-ord, kind-attrs)`.
    /// Idempotent; a prerequisite for byte-deterministic serialization since
    /// mark discovery order (parser walk) is not canonical.
    pub fn canonicalize_marks(&mut self) {
        self.marks.sort_by(|a, b| {
            (a.range.start, a.range.end, a.kind.ord())
                .cmp(&(b.range.start, b.range.end, b.kind.ord()))
                .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
        });
        self.islands.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Merge adjacent/overlapping same-kind formatting marks into one; leave
    /// identity (anchor) marks untouched. This is the normalization Spike A
    /// froze: **union** for formatting, **free overlap** across kinds, **no
    /// merge** for identity. Assumes [`canonicalize_marks`] ran first.
    pub fn normalize_marks(&mut self) {
        self.canonicalize_marks();
        let mut out: Vec<Mark> = Vec::with_capacity(self.marks.len());
        for m in self.marks.drain(..) {
            if let Some(prev) = out.last_mut() {
                if prev.kind == m.kind
                    && m.kind.merges()
                    && m.range.start <= prev.range.end
                {
                    // Same kind, touching or overlapping → union.
                    prev.range.end = prev.range.end.max(m.range.end);
                    continue;
                }
            }
            out.push(m);
        }
        self.marks = out;
    }
}
