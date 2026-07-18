//! The per-field edit surface: a [`Delta`] of text splices over the USV content,
//! plus the **stale-text writer** path — cold-parse a full new markdown document,
//! char-diff it against the base, and rebase the base's identity marks
//! (anchors/comments) through the diff so annotations survive an LLM
//! full-document rewrite with no preservation contract on the LLM.
//!
//! ## Text splices, not attributed ops
//!
//! [`Delta`] is `retain` / `insert` / `delete` over the character sequence —
//! CodeMirror `ChangeSet` / OT text semantics, **not** Quill-Delta. It carries
//! no formatting attributes: marks and islands are separate `(range, kind)` data
//! that *rebase through* a delta ([`Delta::map_pos`]), they do not ride it as op
//! attributes. This is deliberate — an attribute map is a per-character property
//! map and cannot represent overlapping same-kind marks or two distinct
//! identity anchors over one range, the exact algebra the content model keeps
//! (Peritext free overlap + identity handles). Editing marks and line/block
//! attributes are their own op channels, not attributes on this delta. The
//! positional channel stays isomorphic to a text CRDT's op stream — the shape
//! real-time collaborative editing would need.
//!
//! [`diff`] computes a Myers/LCS minimal edit script and pairs it with a
//! **move detector** that re-homes an anchor across a verbatim block move.
//! Position mapping ([`Delta::map_pos`]) follows CodeMirror's
//! `ChangeDesc.mapPos` / ProseMirror mapping semantics. Anchoring a captured
//! position across edits is the editor's job (its own transaction mapping); the
//! content carries no session-side change log.
//!
//! ## The move weak spot (documented limit)
//!
//! A paragraph reorder is delete-here + insert-there to any char differ, so a
//! naive rebase collapses an anchor in the moved text to the deletion point. The
//! detector re-homes an anchor onto a **single, verbatim block move** by locating
//! the moved text in the new content. Text both *moved and rewritten* in one round
//! (the match is lost) drops the anchor — the accepted residual, stated not
//! hidden. Tightening verbatim → fuzzy (longest-common-substring) is a hardening
//! follow-up.

use crate::model::{Mark, MarkKind, Content};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

/// A per-field edit against a base content. Ops apply left-to-right, consuming
/// base positions; `Retain`/`Delete` advance the base cursor, `Insert` adds new
/// text. USV throughout.
///
/// Serializes as `{ "ops": [ {"retain": n} | {"insert": s} | {"delete": n} ] }`
/// — plain, structured-clone-able data an editor bridge stores in a change
/// record and maps its own positions through ([`map_pos`](Self::map_pos)). The
/// serde shape is the wire the `rebase` codec and `applyChange` bundle carry
/// across the language bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub ops: Vec<Op>,
}

/// One delta operation. Serializes externally-tagged with a lowercase key
/// (`{"retain": 5}`, `{"insert": "x"}`, `{"delete": 2}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Keep `n` chars of the base unchanged.
    Retain(usize),
    /// Insert this text at the cursor.
    Insert(String),
    /// Drop `n` chars of the base.
    Delete(usize),
}

/// Which side of a same-position insertion a mapped point lands on. Serializes
/// as `"before"` / `"after"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Assoc {
    /// Stay before inserted text.
    Before,
    /// Move after inserted text.
    After,
}

/// A delta's expected base length disagreed with the text it was applied to —
/// the delta was built against a different revision of the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseLengthMismatch {
    pub expected: usize,
    pub actual: usize,
}

impl Delta {
    /// Chars of base the `Retain`/`Delete` ops together consume — the base
    /// length this delta was built against.
    pub fn expected_base_len(&self) -> usize {
        self.ops
            .iter()
            .map(|op| match op {
                Op::Retain(n) | Op::Delete(n) => *n,
                Op::Insert(_) => 0,
            })
            .sum()
    }

    /// Apply to `base`, producing the new text. Base beyond what the ops
    /// consume is retained implicitly — a *short* delta names only the region
    /// it changes (a bare prepend, an edit near the start) and the untouched
    /// remainder carries through. **Panics** if the ops consume *more* base than
    /// exists (`expected_base_len() > base.chars().count()`): a delta built
    /// against a longer revision. This is the trusted-provenance path; clamping
    /// an over-long delta silently is corruption, so where the base's provenance
    /// isn't already trusted use [`Self::try_apply`], which returns the mismatch
    /// as an error instead.
    pub fn apply(&self, base: &str) -> String {
        let chars: Vec<char> = base.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        for op in &self.ops {
            match op {
                // Over-long Retain/Delete index past `chars` and panic here —
                // the intended failure on a wrong-revision base.
                Op::Retain(n) => {
                    out.extend(&chars[i..i + n]);
                    i += n;
                }
                Op::Delete(n) => i += n,
                Op::Insert(s) => out.push_str(s),
            }
        }
        out.extend(&chars[i..]);
        out
    }

    /// [`Self::apply`], but returns [`BaseLengthMismatch`] instead of panicking
    /// when the ops consume *more* base than `base` has — a delta built against
    /// a longer revision. Implicit trailing retain is the contract: a *short*
    /// delta (ops consuming less than `base`) is accepted and the untouched
    /// remainder is retained, matching [`map_pos`](Self::map_pos)'s implicit
    /// trailing retain so a producer that names only the changed region need not
    /// pad a bare trailing [`Op::Retain`].
    ///
    /// Cost of the leniency: a short delta carries no full-base-length check, so
    /// one replayed against a wrong but *longer* base applies silently instead
    /// of failing. An abbreviated delta forfeits that tripwire by construction;
    /// over-consumption still fails.
    pub fn try_apply(&self, base: &str) -> Result<String, BaseLengthMismatch> {
        let expected = self.expected_base_len();
        let actual = base.chars().count();
        if expected > actual {
            return Err(BaseLengthMismatch { expected, actual });
        }
        Ok(self.apply(base))
    }

    /// Map a base char position to its new position. `assoc` decides the side of
    /// a same-position insertion (`After` moves past it).
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        let mut old = 0usize;
        let mut new = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    // Strictly inside the retain resolves here; the right
                    // boundary (pos == old + n) falls through, so a following
                    // Insert can apply its `assoc`.
                    if pos < old + n {
                        return new + (pos - old);
                    }
                    old += n;
                    new += n;
                }
                Op::Delete(n) => {
                    if pos < old + n {
                        // Inside (or at the start of) the deletion — collapse to
                        // the deletion point.
                        return new;
                    }
                    old += n;
                }
                Op::Insert(s) => {
                    let len = s.chars().count();
                    if pos == old {
                        match assoc {
                            Assoc::Before => return new,
                            Assoc::After => new += len, // fall through past insert
                        }
                    } else {
                        new += len;
                    }
                }
            }
        }
        new + pos.saturating_sub(old)
    }

    /// Whether base position `pos` sits strictly inside a deleted span. The
    /// deletion's left edge (`pos == old`) survives — a point anchor there stays
    /// put — so only `old < pos < old + n` counts as deleted.
    fn is_deleted(&self, pos: usize) -> bool {
        let mut old = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => old += n,
                Op::Delete(n) => {
                    if pos > old && pos < old + n {
                        return true;
                    }
                    old += n;
                }
                Op::Insert(_) => {}
            }
        }
        false
    }

    /// New-text char ranges covered by `Insert` ops — the only regions an anchor
    /// may be re-homed into (moved text must have been *inserted*, not merely
    /// present in surviving text elsewhere).
    fn inserted_spans(&self) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut new = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => new += n,
                Op::Insert(s) => {
                    let len = s.chars().count();
                    if len > 0 {
                        spans.push((new, new + len));
                    }
                    new += len;
                }
                Op::Delete(_) => {}
            }
        }
        spans
    }
}

/// A relocation match shorter than this many chars is too weak to trust — the
/// verbatim-move detector's length floor (mirrors the spike's `MIN_MOVE`).
const MIN_MOVE: usize = 4;

/// Above this many USV chars, the single-line path skips `similar`'s
/// char-level Myers diff and falls back to [`coarse_replace`] (issue #849).
/// `TextDiff::from_chars` is O(N·D) with no deadline; on two long, unrelated
/// single-line strings (no newlines to fall back to line granularity — the
/// realistic shape of an LLM full-document rewrite) D grows with N, so cost
/// is effectively quadratic. Two unrelated 30,000-char lines measured 86s in
/// a debug build. This threshold sits comfortably below that (6x headroom)
/// while still covering a real single-paragraph field, which plausibly runs
/// to a few thousand chars. A fixed cutoff was chosen over
/// `TextDiffConfig::timeout` — nothing in this crate uses `TextDiffConfig`
/// today, and a char budget is deterministic (no wall-clock flakiness in
/// CI, no partial-diff result to reason about).
const CHAR_DIFF_LIMIT: usize = 5_000;

/// Char-level Myers/LCS diff over USV: a minimal `Retain` / `Delete` / `Insert`
/// script. Disjoint edits stay separate ops rather than collapsing the span
/// between them into one delete+insert, so anchors sitting in unchanged middle
/// text survive rebase without relying on the move detector.
///
/// Single-line text diffs at char granularity; multi-line text diffs at line
/// granularity so a paragraph reorder surfaces as whole-line insert spans the
/// move detector can match (char Myers fragments reordered blocks). Above
/// `CHAR_DIFF_LIMIT` chars, the single-line path skips Myers entirely and
/// uses `coarse_replace` instead.
pub fn diff(base: &str, new: &str) -> Delta {
    let multiline = base.contains('\n') || new.contains('\n');
    if !multiline
        && (base.chars().count() > CHAR_DIFF_LIMIT || new.chars().count() > CHAR_DIFF_LIMIT)
    {
        return coarse_replace(base, new);
    }
    let text_diff = if multiline {
        TextDiff::from_lines(base, new)
    } else {
        TextDiff::from_chars(base, new)
    };
    let mut ops = Vec::new();
    for change in text_diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => push_retain(&mut ops, change.value().chars().count()),
            ChangeTag::Delete => push_delete(&mut ops, change.value().chars().count()),
            ChangeTag::Insert => push_insert(&mut ops, change.value()),
        }
    }
    Delta { ops }
}

/// Linear-time fallback for [`diff`] above [`CHAR_DIFF_LIMIT`]: trims the
/// longest common prefix and suffix (plain char comparison, no Myers) and
/// replaces only the middle. Not a minimal edit script, but still useful for
/// anchor rebasing — an anchor sitting in the untouched prefix or suffix maps
/// through a real `Retain` exactly as it would from a full diff; only an
/// anchor inside the replaced middle depends on the move detector.
fn coarse_replace(base: &str, new: &str) -> Delta {
    let base_chars: Vec<char> = base.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let max_common = base_chars.len().min(new_chars.len());

    let mut prefix = 0;
    while prefix < max_common && base_chars[prefix] == new_chars[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < max_common - prefix
        && base_chars[base_chars.len() - 1 - suffix] == new_chars[new_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut ops = Vec::new();
    push_retain(&mut ops, prefix);
    push_delete(&mut ops, base_chars.len() - prefix - suffix);
    let inserted: String = new_chars[prefix..new_chars.len() - suffix].iter().collect();
    push_insert(&mut ops, &inserted);
    push_retain(&mut ops, suffix);
    Delta { ops }
}

fn push_retain(ops: &mut Vec<Op>, n: usize) {
    if n == 0 {
        return;
    }
    if let Some(Op::Retain(last)) = ops.last_mut() {
        *last += n;
    } else {
        ops.push(Op::Retain(n));
    }
}

fn push_delete(ops: &mut Vec<Op>, n: usize) {
    if n == 0 {
        return;
    }
    if let Some(Op::Delete(last)) = ops.last_mut() {
        *last += n;
    } else {
        ops.push(Op::Delete(n));
    }
}

fn push_insert(ops: &mut Vec<Op>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Op::Insert(last)) = ops.last_mut() {
        last.push_str(s);
    } else {
        ops.push(Op::Insert(s.to_owned()));
    }
}

/// The stale-text writer path: cold-parse `new_markdown`, char-diff it against
/// `base`, and carry `base`'s identity marks (anchors) forward, rebased through
/// the diff (re-homing verbatim block moves). The returned content is `new_rt`
/// (structure/marks/islands from the fresh import) plus the surviving anchors.
///
/// Returns the new content and the [`Delta`] used — the text change an editor
/// bridge can map its own positions through.
pub fn diff_import(
    base: &Content,
    new_markdown: &str,
) -> Result<(Content, Delta), crate::import::ImportError> {
    let mut new_rt = crate::import::from_markdown(new_markdown)?;
    let delta = diff(&base.text, &new_rt.text);

    let base_chars: Vec<char> = base.text.chars().collect();
    let new_chars: Vec<char> = new_rt.text.chars().collect();
    let inserted = delta.inserted_spans();
    for m in &base.marks {
        // Only identity marks live in the content but not in markdown; formatting
        // marks are re-derived by the fresh import, so we do not carry them.
        let MarkKind::Anchor { .. } = &m.kind else {
            continue;
        };
        if let Some((ns, ne)) = rebase_anchor(&delta, &base_chars, &new_chars, &inserted, m) {
            new_rt.marks.push(Mark {
                start: ns,
                end: ne,
                kind: m.kind.clone(),
            });
        }
        // else: detached — the accepted residual drop.
    }
    new_rt.normalize();
    Ok((new_rt, delta))
}

/// Rebase one anchor through the delta. Returns its new range, or `None` if it
/// detaches (its text was deleted and no verbatim move re-homes it).
fn rebase_anchor(
    delta: &Delta,
    base_chars: &[char],
    new_chars: &[char],
    inserted: &[(usize, usize)],
    m: &Mark,
) -> Option<(usize, usize)> {
    if m.start == m.end {
        // Zero-width point anchor.
        if !delta.is_deleted(m.start) {
            let p = delta.map_pos(m.start, Assoc::Before);
            return Some((p, p));
        }
        // Its surrounding text was deleted — relocate only if that text was
        // re-inserted verbatim elsewhere (a move).
        return relocate_point(base_chars, new_chars, inserted, m.start);
    }

    let ns = delta.map_pos(m.start, Assoc::After);
    let ne = delta.map_pos(m.end, Assoc::Before);
    if ns < ne {
        return Some((ns, ne)); // survived a surrounding edit
    }
    // Collapsed — try a verbatim block move: the annotated span must reappear
    // inside inserted text (not merely somewhere in the surviving content).
    relocate_span(base_chars, new_chars, inserted, m.start, m.end)
}

/// Find the annotated span `base[start..end]` inside an inserted region of the
/// new text. Requires a length floor and containment in inserted text, so an
/// unrelated surviving occurrence of the same words cannot capture the anchor.
fn relocate_span(
    base_chars: &[char],
    new_chars: &[char],
    inserted: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    if end > base_chars.len() {
        return None;
    }
    let needle = &base_chars[start..end];
    find_in_spans(new_chars, needle, inserted).map(|pos| (pos, pos + needle.len()))
}

/// Relocate a point anchor by its left context (text immediately before it),
/// but only if that context reappears inside inserted text — the same
/// move-only, length-floored discipline as [`relocate_span`].
fn relocate_point(
    base_chars: &[char],
    new_chars: &[char],
    inserted: &[(usize, usize)],
    pos: usize,
) -> Option<(usize, usize)> {
    const K: usize = 24;
    let l0 = pos.saturating_sub(K);
    let left = &base_chars[l0..pos];
    if let Some(p) = find_in_spans(new_chars, left, inserted) {
        return Some((p + left.len(), p + left.len()));
    }
    let r1 = (pos + K).min(base_chars.len());
    let right = &base_chars[pos..r1];
    if let Some(p) = find_in_spans(new_chars, right, inserted) {
        return Some((p, p));
    }
    None
}

/// First index where `needle` occurs in `hay` while *overlapping* an inserted
/// span — i.e. the match touches text the rewrite actually inserted, not purely
/// surviving text. Overlap (not full containment) is required because a diff
/// can split a moved block across an inserted region and the retained
/// suffix; demanding containment would miss real moves, while demanding overlap
/// still rejects an unrelated occurrence sitting entirely in retained text.
/// Enforces [`MIN_MOVE`]. O(hay × needle) naive scan — fine at memo/document
/// scale; a large-document target would want a substring-search algorithm
/// (e.g. KMP) here.
fn find_in_spans(hay: &[char], needle: &[char], spans: &[(usize, usize)]) -> Option<usize> {
    if needle.len() < MIN_MOVE || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| {
        &hay[i..i + needle.len()] == needle
            && spans.iter().any(|&(s, e)| i < e && i + needle.len() > s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::from_markdown;
    use crate::model::MarkKind;

    #[test]
    fn diff_apply_round_trips() {
        let d = diff("the quick brown fox", "the slow brown fox");
        assert_eq!(d.apply("the quick brown fox"), "the slow brown fox");
    }

    #[test]
    fn map_pos_insertion() {
        // Insert "XY" at position 3 of "abcdef".
        let d = diff("abcdef", "abcXYdef");
        assert_eq!(d.apply("abcdef"), "abcXYdef");
        // A point before the insert is unmoved; after it shifts by 2.
        assert_eq!(d.map_pos(2, Assoc::After), 2);
        assert_eq!(d.map_pos(4, Assoc::Before), 6);
    }

    #[test]
    fn try_apply_accepts_short_delta_with_implicit_trailing_retain() {
        // A bare prepend consumes no base; the untouched remainder is retained
        // implicitly rather than tripping the base-length check.
        let short = Delta {
            ops: vec![Op::Insert("NEW ".into())],
        };
        assert_eq!(short.expected_base_len(), 0);
        assert_eq!(short.try_apply("hello").unwrap(), "NEW hello");

        // An edit near the start, naming only its region, applies against the
        // whole base — same result whether or not a trailing retain is written.
        let partial = Delta {
            ops: vec![Op::Retain(1), Op::Insert("X".into())],
        };
        assert_eq!(partial.try_apply("hello").unwrap(), "hXello");
    }

    #[test]
    fn try_apply_rejects_over_long_delta() {
        // Consuming more base than exists is a wrong-revision delta, not an
        // abbreviated one — it errors, it does not clamp.
        let over = Delta {
            ops: vec![Op::Retain(9)],
        };
        assert_eq!(
            over.try_apply("hello"),
            Err(BaseLengthMismatch {
                expected: 9,
                actual: 5,
            })
        );

        // Over-consumption via Delete fails the same way.
        let over_del = Delta {
            ops: vec![Op::Delete(9)],
        };
        assert!(over_del.try_apply("hello").is_err());
    }

    #[test]
    #[should_panic]
    fn apply_panics_on_over_long_delta() {
        // The trusted-provenance path panics rather than clamping an over-long
        // delta to silent garbage.
        let over = Delta {
            ops: vec![Op::Retain(9)],
        };
        let _ = over.apply("hello");
    }

    #[test]
    fn anchor_rehomed_on_block_move() {
        // Two paragraphs; anchor on the first; the rewrite swaps their order.
        let mut base = from_markdown("first para here\n\nsecond para here").unwrap();
        // "first para here" is chars 0..15
        base.marks.push(Mark {
            start: 0,
            end: 15,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "second para here\n\nfirst para here").unwrap();
        let anchor = new_rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor re-homed onto moved block");
        assert_eq!(
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)]
                .to_string(),
            "first para here"
        );
    }

    #[test]
    fn anchor_dropped_when_text_deleted() {
        let mut base = from_markdown("keep this and drop that").unwrap();
        // Anchor on "drop that" (14..23).
        base.marks.push(Mark {
            start: 14,
            end: 23,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "keep this").unwrap();
        assert!(
            !new_rt
                .marks
                .iter()
                .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")),
            "anchor on deleted text detaches (accepted residual)"
        );
    }

    #[test]
    fn anchor_not_rehomed_onto_unrelated_survivor() {
        // Regression (review finding 5): an anchor on deleted text must NOT
        // capture an unrelated *surviving* occurrence of the same words.
        let mut base = from_markdown("target one to drop\n\nkeep the target two").unwrap();
        base.marks.push(Mark {
            start: 0,
            end: 6, // "target" in the first (deleted) paragraph
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        // First paragraph deleted; the second (with its own "target") survives
        // as retained text — the anchor must drop, not jump to it.
        let (new_rt, _) = diff_import(&base, "keep the target two").unwrap();
        assert!(
            !new_rt
                .marks
                .iter()
                .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")),
            "anchor wrongly re-homed onto surviving unrelated text"
        );
    }

    #[test]
    fn map_pos_after_moves_past_boundary_insertion() {
        // Regression (finding 7): a point at a retain|insert boundary with
        // Assoc::After lands after the inserted text.
        let d = diff("abcdef", "abcXYdef");
        assert_eq!(d.map_pos(3, Assoc::After), 5);
        assert_eq!(d.map_pos(3, Assoc::Before), 3);
    }

    #[test]
    fn point_anchor_at_deletion_left_edge_survives() {
        // Regression (finding 13): the deletion's left edge is not "deleted".
        let d = diff("abcdef", "abef"); // delete "cd" (span [2,4))
        assert!(!d.is_deleted(2), "left edge of deletion survives");
        assert!(d.is_deleted(3), "interior of deletion is deleted");
    }

    #[test]
    fn disjoint_edits_are_separate_ops() {
        // Myers/LCS (char): prefix and suffix edits must not collapse the middle.
        let d = diff("aaaMIDDLEbbb", "AAAMIDDLEZZZ");
        assert_eq!(d.apply("aaaMIDDLEbbb"), "AAAMIDDLEZZZ");
        let retained: usize = d
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Retain(n) => Some(*n),
                _ => None,
            })
            .sum();
        assert!(
            retained >= 6,
            "unchanged middle span retained ({retained} USV): {ops:?}",
            ops = d.ops
        );
        assert!(
            !matches!(d.ops.as_slice(), [Op::Delete(_), Op::Insert(_)]),
            "coarse single replace: {ops:?}",
            ops = d.ops
        );
    }

    #[test]
    fn anchor_survives_between_disjoint_edits() {
        let mut base = from_markdown("aaaMIDDLEbbb").unwrap();
        base.marks.push(Mark {
            start: 3,
            end: 9,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "AAAMIDDLEZZZ").unwrap();
        let anchor = new_rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor between disjoint edits survives without move detector");
        assert_eq!(
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)]
                .to_string(),
            "MIDDLE"
        );
    }

    fn byte(s: &str, char_idx: usize) -> usize {
        crate::usv::char_to_byte(s, char_idx)
    }

    /// Deterministic filler with no long common substring between the two
    /// variants — worst case for a char-level Myers diff (issue #849).
    fn filler(n: usize, offset: u8) -> String {
        (0..n)
            .map(|i| char::from(b'a' + ((i as u8).wrapping_mul(7).wrapping_add(offset)) % 26))
            .collect()
    }

    #[test]
    fn large_single_line_diff_stays_fast() {
        // Two long, unrelated single-line strings — exactly the shape
        // `similar::TextDiff::from_chars` chokes on with no cutoff (issue
        // #849: 30,000 unrelated chars measured 86s in a debug build). Above
        // CHAR_DIFF_LIMIT, `diff` must skip Myers and stay far under budget
        // regardless of input size.
        let base = format!("PREFIX-{}-BASE-SUFFIX", filler(25_000, 0));
        let new = format!("PREFIX-{}-NEW-SUFFIX", filler(25_000, 13));

        let start = std::time::Instant::now();
        let d = diff(&base, &new);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "large single-line diff took {elapsed:?}, expected well under the 2s budget"
        );

        // Sensible, not just fast: still round-trips exactly.
        assert_eq!(d.apply(&base), new);
    }

    #[test]
    fn large_single_line_diff_retains_common_prefix_and_suffix() {
        // The coarse fallback must still be *usable* for anchor rebasing,
        // not merely fast: a shared prefix/suffix around a large rewritten
        // middle should come back as real Retain ops, not a single
        // whole-field Delete+Insert that would force every anchor through
        // the move detector.
        let base = format!("shared-prefix-{}-shared-suffix", filler(20_000, 0));
        let new = format!("shared-prefix-{}-shared-suffix", filler(20_000, 5));
        let d = diff(&base, &new);
        assert_eq!(d.apply(&base), new);

        let Some(Op::Retain(prefix_len)) = d.ops.first() else {
            panic!("expected a leading Retain for the shared prefix: {:?}", d.ops);
        };
        assert!(
            *prefix_len >= "shared-prefix-".len(),
            "shared prefix should be retained, got Retain({prefix_len})"
        );
        let Some(Op::Retain(suffix_len)) = d.ops.last() else {
            panic!("expected a trailing Retain for the shared suffix: {:?}", d.ops);
        };
        assert!(
            *suffix_len >= "shared-suffix".len(),
            "shared suffix should be retained, got Retain({suffix_len})"
        );
    }

    #[test]
    fn diff_import_large_single_line_rewrite_keeps_prefix_anchor() {
        // End-to-end through the real DoS-exposed path: `diff_import` is
        // what a full-document LLM rewrite hits. A large, single-line,
        // unrelated-middle rewrite must complete quickly *and* still rebase
        // an anchor sitting in unchanged (shared) text.
        let base_text = format!("hello target world-{}-end", filler(30_000, 0));
        let mut base = from_markdown(&base_text).unwrap();
        base.marks.push(Mark {
            start: 6,
            end: 12, // "target"
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();

        let new_markdown = format!("hello target world-{}-end", filler(30_000, 11));
        let start = std::time::Instant::now();
        let (new_rt, _delta) = diff_import(&base, &new_markdown).unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "diff_import took {elapsed:?}, expected well under the 2s budget"
        );

        let anchor = new_rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor in shared prefix survives the coarse fallback diff");
        assert_eq!(
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)]
                .to_string(),
            "target"
        );
    }
}
