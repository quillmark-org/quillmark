//! The `RichText` corpus model — one text sequence per field carrying line
//! attributes, anchored marks, and embedded islands, over a single coordinate
//! space of Unicode scalar values (Rust `char`).
//!
//! This is the phase-1 freeze (issue #831): the mark set, the three
//! normalization rules, and the invariants are what canonical serialization
//! commits to. Everything an editor disagrees on (edge-expand,
//! adjacent-merge-at-insertion) is *not* encoded — the model only ever stores
//! the resulting range, so the stored form is identical whatever the editor
//! did. See `prose/plans/richtext/phase-0.md` § Spike A.

use serde_json::Value as JsonValue;

/// A position in a [`RichText`], counted in Unicode scalar values (USV) — never
/// bytes, never UTF-16 units. One astral char is 1 USV / 4 UTF-8 bytes / 2
/// UTF-16 units. Conversions to/from the JS (UTF-16) and Rust (UTF-8)
/// boundaries live in [`crate::usv`].
pub type Usv = usize;

/// U+FFFC OBJECT REPLACEMENT CHARACTER — the single-USV slot an island occupies
/// in the corpus. One slot per island; every slot has a backing island. A stray
/// slot (or a slot with no island) is an invariant violation.
pub const ISLAND_SLOT: char = '\u{FFFC}';

/// One content field as a corpus: the text plus the structure that rides on it.
///
/// Invariants (established once by import normalization, checked by
/// [`RichText::validate`]): the text holds no `\r` and no bidi controls; the
/// count of [`ISLAND_SLOT`] equals `islands.len()`; `lines.len()` equals the
/// number of `\n`-separated segments; marks are normalized (sorted, unioned).
#[derive(Debug, Clone, PartialEq)]
pub struct RichText {
    /// The corpus. `\n` is a line boundary; [`ISLAND_SLOT`] is an island slot.
    pub text: String,
    /// One entry per `\n`-separated segment of `text`, in order. The line tree
    /// is *derived* from this flat list plus each line's `containers` path — it
    /// is never stored, so a split/join is a single-char edit with no identity
    /// crisis (there are no paragraph IDs).
    pub lines: Vec<Line>,
    /// Marks over char ranges, kept normalized: sorted by
    /// `(start, end, kind-ord, attrs)`, same-kind formatting marks unioned.
    pub marks: Vec<Mark>,
    /// One entry per [`ISLAND_SLOT`], in slot order (ascending char position).
    pub islands: Vec<Island>,
}

/// A line's attributes: its block role plus the container path it sits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// Ancestor containers, outermost first. A multi-paragraph list item is two
    /// `Para` lines sharing one `[ListItem]` path; a paragraph in a quote in a
    /// list item is `[ListItem, Quote]`.
    pub containers: Vec<Container>,
    /// Whether this line continues the previous line's *block* across a hard
    /// line break (no paragraph break between), rather than starting a new
    /// block. `false` = a new block (paragraph spacing on either side); `true` =
    /// a within-block line break (markdown hard break; consecutive lines of one
    /// code fence). The first line is always `false`. This is what keeps a hard
    /// break (backend `#linebreak()`) distinct from a paragraph boundary through
    /// the freeze, and what groups a code fence's lines without an
    /// adjacency heuristic.
    pub continues: bool,
}

/// The block role of a line. The tree between lines is inferred: two adjacent
/// lines with equal `kind`+`containers` are two blocks of that role (e.g. two
/// paragraphs), never one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Para,
    /// ATX/Setext heading, level 1..=6.
    Heading { level: u8 },
    /// A line of a code block. `lang` is the (sanitized) info string, shared by
    /// every line of the same block.
    Code { lang: Option<String> },
    /// A block-level island: the line's sole content is one [`ISLAND_SLOT`].
    Island,
}

/// A container a line nests inside. The ancestor path is a `Vec<Container>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Container {
    /// A list item. `ordered` distinguishes `1.` from `-`; `start` is the list's
    /// first number (1 by default); `ordinal` is this item's 0-based index in
    /// its list. Two *adjacent* lines belong to the same item iff their whole
    /// container path (ordinals included) is equal — so a multi-paragraph item
    /// is two lines sharing one `ListItem`, while the next item differs by
    /// `ordinal`. (Identity is path **plus contiguity**: two sibling inner lists
    /// under one outer item can produce equal first-item paths, distinguished
    /// only by the non-adjacency of their runs.) Positional and deterministic —
    /// no minted ids.
    ListItem {
        ordered: bool,
        start: u64,
        ordinal: u64,
    },
    /// A block quote. Adjacent lines sharing `[Quote]` are one multi-paragraph
    /// quote; phase 1 does not distinguish two adjacent separate quotes (they
    /// merge on round-trip — a documented canonicalization).
    Quote,
}

/// A mark over a char range `[start, end)`. `start == end` (zero-width) is legal
/// only for [`MarkKind::Anchor`]; normalization drops zero-width formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub start: Usv,
    pub end: Usv,
    pub kind: MarkKind,
}

/// The mark set — **open**: an unknown kind round-trips as [`MarkKind::Unknown`],
/// absorbed as a new *type*, never a changed semantics of a known one. Two
/// algebra classes: formatting is a property of a range (two coincident are
/// redundant); identity is a handle (two over the same range are two things).
#[derive(Debug, Clone, PartialEq)]
pub enum MarkKind {
    // Formatting — round-trippable projection marks. `is_formatting()`.
    Strong,
    Emph,
    Underline,
    Strike,
    Code,
    Link {
        url: String,
    },
    // Identity — a handle, not a property. Never merged, may be zero-width.
    /// A comment thread or stable anchor, carried by id and rebased across
    /// edits like any position. No markdown projection (omitted on export;
    /// survives via diff-rebase).
    Anchor {
        id: String,
    },
    // Open-set escape hatch — an unknown mark type, round-tripped opaque.
    Unknown {
        tag: String,
        attrs: JsonValue,
    },
}

/// A structured object with no honest text encoding — a table, figure, or future
/// embed — occupying one [`ISLAND_SLOT`] in the corpus.
#[derive(Debug, Clone, PartialEq)]
pub struct Island {
    /// Minted, `$id`-style stable id. The single source of hash
    /// nondeterminism once islands mint (phase 4); text stays deterministic.
    pub id: String,
    /// Island type discriminator (`"table"`, `"image"`, …). Unknown types
    /// round-trip opaque.
    pub island_type: String,
    /// Typed payload. Recursively key-sorted by normalization so it hashes
    /// deterministically despite `serde_json`'s `preserve_order`.
    pub props: JsonValue,
    /// How faithfully the markdown projection can carry this island.
    pub loss: Loss,
}

/// The markdown-projection loss class of an island.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loss {
    /// Markdown carries it faithfully (round-trips identically).
    Lossless,
    /// Markdown carries an approximation (round-trips visibly, not identically).
    Degraded,
    /// No markdown encoding; export emits a placeholder only.
    Unrepresentable,
}

impl MarkKind {
    /// Formatting marks are a property of a range and union when coincident;
    /// identity/unknown marks are handles and never merge (Spike-A rules).
    pub fn is_formatting(&self) -> bool {
        matches!(
            self,
            MarkKind::Strong
                | MarkKind::Emph
                | MarkKind::Underline
                | MarkKind::Strike
                | MarkKind::Code
                | MarkKind::Link { .. }
        )
    }

    /// Total order over kinds for the canonical sort tie-break, after
    /// `(start, end)`. Stable across releases — part of the freeze.
    pub fn ord(&self) -> u8 {
        match self {
            MarkKind::Strong => 0,
            MarkKind::Emph => 1,
            MarkKind::Underline => 2,
            MarkKind::Strike => 3,
            MarkKind::Code => 4,
            MarkKind::Link { .. } => 5,
            MarkKind::Anchor { .. } => 6,
            MarkKind::Unknown { .. } => 7,
        }
    }

    /// Attribute tie-break string, appended after `ord` in the canonical sort so
    /// two marks that differ only in attrs order deterministically. Also the
    /// grouping key for same-kind union (two formatting marks union only when
    /// this matches — e.g. two `link`s union only at the same url).
    pub fn attrs_key(&self) -> String {
        match self {
            MarkKind::Link { url } => url.clone(),
            MarkKind::Anchor { id } => id.clone(),
            MarkKind::Unknown { tag, attrs } => {
                // Attrs sorted so the key is order-insensitive.
                format!("{}\u{0}{}", tag, canonical_json_string(attrs))
            }
            _ => String::new(),
        }
    }
}

/// A `serde_json::Value` rendered to a string with object keys recursively
/// sorted — order-insensitive, so it is a stable comparison/grouping key.
pub(crate) fn canonical_json_string(v: &JsonValue) -> String {
    serde_json::to_string(&sorted_value(v)).unwrap_or_default()
}

/// Rebuild `v` with every object's keys sorted, recursively. Pins island
/// `props` (and unknown-mark attrs) against `preserve_order` leaking insertion
/// order into the canonical bytes / content hash (Spike C carry-forward).
pub(crate) fn sorted_value(v: &JsonValue) -> JsonValue {
    match v {
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(sorted_value).collect()),
        JsonValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::with_capacity(map.len());
            for k in keys {
                out.insert(k.clone(), sorted_value(&map[k]));
            }
            JsonValue::Object(out)
        }
        other => other.clone(),
    }
}

/// The bidi formatting chars import normalization strips; their presence in a
/// corpus is an invariant violation. Mirrors [`crate::normalize`]'s set.
fn is_bidi_char(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'
            | '\u{202B}'
            | '\u{202C}'
            | '\u{202D}'
            | '\u{202E}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
    )
}

/// Ways a [`RichText`] can violate its invariants. Returned by
/// [`RichText::validate`]; import normalization guarantees none of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invariant {
    /// `\r` in the text (line endings must be normalized to `\n`).
    CarriageReturn,
    /// A bidi formatting control in the text.
    BidiControl(char),
    /// `island_slot_count != islands.len()`.
    IslandSlotMismatch { slots: usize, islands: usize },
    /// `lines.len() != newline_segment_count`.
    LineCountMismatch { lines: usize, segments: usize },
    /// A mark range runs past the corpus or is inverted (`start > end`).
    MarkOutOfRange { start: Usv, end: Usv, len: Usv },
    /// A zero-width formatting mark survived normalization.
    ZeroWidthFormatting { at: Usv },
    /// A heading level outside 1..=6.
    BadHeadingLevel(u8),
    /// The first line has `continues: true` (nothing precedes it to continue).
    FirstLineContinues,
    /// An [`MarkKind::Unknown`] reused a reserved built-in `type` name.
    ReservedUnknownTag(String),
    /// A formatting mark edge sits on a `\n` (normalization should have trimmed
    /// it) — a hand-built corpus that skipped `normalize`.
    MarkEdgeOnNewline { at: Usv },
}

impl RichText {
    /// An empty corpus: one empty `Para` line, no marks, no islands.
    pub fn empty() -> Self {
        RichText {
            text: String::new(),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: Vec::new(),
                continues: false,
            }],
            marks: Vec::new(),
            islands: Vec::new(),
        }
    }

    /// Total length in USV.
    pub fn len_usv(&self) -> Usv {
        self.text.chars().count()
    }

    /// Number of `\n`-separated segments — the required `lines.len()`.
    pub fn segment_count(&self) -> usize {
        self.text.chars().filter(|c| *c == '\n').count() + 1
    }

    /// Normalize marks in place: drop zero-width formatting, union same-kind
    /// formatting that is adjacent or overlapping, recursively key-sort island
    /// props and unknown-mark attrs, then sort marks canonically. Idempotent —
    /// the fixed point the canonical serialization commits to.
    pub fn normalize(&mut self) {
        // Islands: canonicalize props key order.
        for island in &mut self.islands {
            island.props = sorted_value(&island.props);
        }
        for mark in &mut self.marks {
            if let MarkKind::Unknown { attrs, .. } = &mut mark.kind {
                *attrs = sorted_value(attrs);
            }
        }
        // A formatting mark's edges never sit on a line boundary: markdown can't
        // bold a `\n`, so two producers that disagree only about whether the
        // boundary is "inside" the mark must canonicalize to the same bounds.
        // Trim leading/trailing `\n` (interior boundaries are kept — a mark may
        // legitimately span lines). Zero-width results are dropped below.
        let chars: Vec<char> = self.text.chars().collect();
        for m in &mut self.marks {
            if m.kind.is_formatting() {
                while m.start < m.end && chars.get(m.start) == Some(&'\n') {
                    m.start += 1;
                }
                while m.end > m.start && chars.get(m.end - 1) == Some(&'\n') {
                    m.end -= 1;
                }
            }
        }
        self.marks = normalize_marks(std::mem::take(&mut self.marks));
    }

    /// Mark `type` names the projection reserves; an [`MarkKind::Unknown`] may
    /// not reuse one (its serialization would parse back as the built-in,
    /// silently dropping its attrs — non-injective). Checked by [`validate`].
    pub const RESERVED_MARK_TYPES: [&'static str; 7] =
        ["strong", "emph", "underline", "strike", "code", "link", "anchor"];

    /// Check every invariant. `Ok(())` on a well-formed corpus. Import
    /// guarantees this; a hand-built corpus should be run through it in tests.
    pub fn validate(&self) -> Result<(), Invariant> {
        let mut slots = 0usize;
        for c in self.text.chars() {
            if c == '\r' {
                return Err(Invariant::CarriageReturn);
            }
            if is_bidi_char(c) {
                return Err(Invariant::BidiControl(c));
            }
            if c == ISLAND_SLOT {
                slots += 1;
            }
        }
        if slots != self.islands.len() {
            return Err(Invariant::IslandSlotMismatch {
                slots,
                islands: self.islands.len(),
            });
        }
        let segments = self.segment_count();
        if self.lines.len() != segments {
            return Err(Invariant::LineCountMismatch {
                lines: self.lines.len(),
                segments,
            });
        }
        if self.lines.first().is_some_and(|l| l.continues) {
            return Err(Invariant::FirstLineContinues);
        }
        let len = self.len_usv();
        let chars: Vec<char> = self.text.chars().collect();
        for m in &self.marks {
            if m.start > m.end || m.end > len {
                return Err(Invariant::MarkOutOfRange {
                    start: m.start,
                    end: m.end,
                    len,
                });
            }
            if m.start == m.end && m.kind.is_formatting() {
                return Err(Invariant::ZeroWidthFormatting { at: m.start });
            }
            if m.kind.is_formatting() {
                if chars.get(m.start) == Some(&'\n') {
                    return Err(Invariant::MarkEdgeOnNewline { at: m.start });
                }
                if m.end > m.start && chars.get(m.end - 1) == Some(&'\n') {
                    return Err(Invariant::MarkEdgeOnNewline { at: m.end - 1 });
                }
            }
            if let MarkKind::Unknown { tag, .. } = &m.kind {
                if Self::RESERVED_MARK_TYPES.contains(&tag.as_str()) {
                    return Err(Invariant::ReservedUnknownTag(tag.clone()));
                }
            }
        }
        for line in &self.lines {
            if let LineKind::Heading { level } = line.kind {
                if !(1..=6).contains(&level) {
                    return Err(Invariant::BadHeadingLevel(level));
                }
            }
        }
        Ok(())
    }
}

/// Apply the three Spike-A rules and the canonical sort to a flat mark list.
///
/// 1. Same-kind formatting marks union when adjacent *or* overlapping.
/// 2. Different-kind marks overlap freely (never split into runs).
/// 3. Identity (and unknown) marks never merge.
///
/// Zero-width formatting marks are dropped (no-ops); zero-width anchors survive.
pub(crate) fn normalize_marks(marks: Vec<Mark>) -> Vec<Mark> {
    use std::collections::BTreeMap;

    // Partition: formatting marks group by (ord, attrs_key) for union; identity
    // and unknown pass through untouched (but zero-width formatting is dropped).
    let mut groups: BTreeMap<(u8, String), Vec<(Usv, Usv)>> = BTreeMap::new();
    let mut kind_of: BTreeMap<(u8, String), MarkKind> = BTreeMap::new();
    let mut passthrough: Vec<Mark> = Vec::new();

    for m in marks {
        if m.kind.is_formatting() {
            if m.start >= m.end {
                continue; // drop zero-width / inverted formatting
            }
            let key = (m.kind.ord(), m.kind.attrs_key());
            kind_of.entry(key.clone()).or_insert_with(|| m.kind.clone());
            groups.entry(key).or_default().push((m.start, m.end));
        } else {
            passthrough.push(m);
        }
    }

    let mut out: Vec<Mark> = Vec::new();
    for (key, mut ranges) in groups {
        ranges.sort_unstable();
        let kind = kind_of.remove(&key).expect("kind recorded with group");
        let mut cur = ranges[0];
        for &(s, e) in &ranges[1..] {
            if s <= cur.1 {
                // adjacent (s == cur.1) or overlapping — union
                cur.1 = cur.1.max(e);
            } else {
                out.push(Mark {
                    start: cur.0,
                    end: cur.1,
                    kind: kind.clone(),
                });
                cur = (s, e);
            }
        }
        out.push(Mark {
            start: cur.0,
            end: cur.1,
            kind,
        });
    }
    out.extend(passthrough);

    // Canonical sort: (start, end, kind-ord, attrs).
    out.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then(a.end.cmp(&b.end))
            .then(a.kind.ord().cmp(&b.kind.ord()))
            .then(a.kind.attrs_key().cmp(&b.kind.attrs_key()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(start: Usv, end: Usv, kind: MarkKind) -> Mark {
        Mark { start, end, kind }
    }

    #[test]
    fn same_kind_adjacent_unions() {
        // [0,3) strong + [3,6) strong -> [0,6) strong (rule 1, adjacency).
        let got = normalize_marks(vec![
            f(3, 6, MarkKind::Strong),
            f(0, 3, MarkKind::Strong),
        ]);
        assert_eq!(got, vec![f(0, 6, MarkKind::Strong)]);
    }

    #[test]
    fn same_kind_overlapping_unions() {
        let got = normalize_marks(vec![f(0, 4, MarkKind::Emph), f(2, 7, MarkKind::Emph)]);
        assert_eq!(got, vec![f(0, 7, MarkKind::Emph)]);
    }

    #[test]
    fn different_kinds_overlap_freely() {
        // Strong and emph over overlapping ranges stay two marks (rule 2).
        let got = normalize_marks(vec![f(0, 5, MarkKind::Strong), f(2, 7, MarkKind::Emph)]);
        assert_eq!(
            got,
            vec![f(0, 5, MarkKind::Strong), f(2, 7, MarkKind::Emph)]
        );
    }

    #[test]
    fn links_union_only_at_same_url() {
        let a = MarkKind::Link { url: "a".into() };
        let b = MarkKind::Link { url: "b".into() };
        // Same url adjacent -> union; different url -> distinct.
        let got = normalize_marks(vec![
            f(0, 2, a.clone()),
            f(2, 4, a.clone()),
            f(4, 6, b.clone()),
        ]);
        assert_eq!(got, vec![f(0, 4, a), f(4, 6, b)]);
    }

    #[test]
    fn identity_never_merges() {
        // Two anchors over the same range are two distinct things (rule 3).
        let a = MarkKind::Anchor { id: "c1".into() };
        let b = MarkKind::Anchor { id: "c2".into() };
        let got = normalize_marks(vec![f(3, 3, a.clone()), f(3, 3, b.clone())]);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&f(3, 3, a)));
        assert!(got.contains(&f(3, 3, b)));
    }

    #[test]
    fn zero_width_formatting_dropped_zero_width_anchor_kept() {
        let got = normalize_marks(vec![
            f(2, 2, MarkKind::Strong),
            f(2, 2, MarkKind::Anchor { id: "x".into() }),
        ]);
        assert_eq!(got, vec![f(2, 2, MarkKind::Anchor { id: "x".into() })]);
    }

    #[test]
    fn empty_is_valid() {
        assert_eq!(RichText::empty().validate(), Ok(()));
    }

    #[test]
    fn validate_catches_slot_mismatch() {
        let mut rt = RichText::empty();
        rt.text = "\u{FFFC}".into();
        rt.lines = vec![Line {
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }];
        assert_eq!(
            rt.validate(),
            Err(Invariant::IslandSlotMismatch {
                slots: 1,
                islands: 0
            })
        );
    }

    #[test]
    fn validate_catches_line_count() {
        let mut rt = RichText::empty();
        rt.text = "a\nb".into(); // 2 segments, but 1 line
        assert_eq!(
            rt.validate(),
            Err(Invariant::LineCountMismatch {
                lines: 1,
                segments: 2
            })
        );
    }

    #[test]
    fn normalize_is_idempotent() {
        let mut rt = RichText::empty();
        rt.text = "hello world".into();
        rt.marks = vec![
            f(6, 11, MarkKind::Strong),
            f(0, 5, MarkKind::Strong),
            f(0, 5, MarkKind::Emph),
        ];
        rt.normalize();
        let once = rt.marks.clone();
        rt.normalize();
        assert_eq!(rt.marks, once);
        assert_eq!(rt.validate(), Ok(()));
    }
}
