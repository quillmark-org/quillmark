//! The per-field edit surface: a [`Delta`] of text splices over the USV corpus,
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
//! identity anchors over one range, the exact algebra the corpus model keeps
//! (Peritext free overlap + identity handles). Editing marks and line/block
//! attributes are their own op channels (phase 3), not attributes on this delta.
//! The positional channel stays isomorphic to a text CRDT's op stream (the
//! phase-4 collab target).
//!
//! Phase 1 delivers the diff + rebase + a **move detector**; the live delta
//! transport (revision, bounded change log) is phase 3. Position mapping follows
//! CodeMirror's `ChangeDesc.mapPos` / ProseMirror mapping semantics.
//!
//! ## The move weak spot (documented limit)
//!
//! A paragraph reorder is delete-here + insert-there to any char differ, so a
//! naive rebase collapses an anchor in the moved text to the deletion point. The
//! detector re-homes an anchor onto a **single, verbatim block move** by locating
//! the moved text in the new corpus. Text both *moved and rewritten* in one round
//! (the match is lost) drops the anchor — the accepted residual, stated not
//! hidden. Tightening verbatim → fuzzy (longest-common-substring) is a hardening
//! follow-up.

use crate::model::{Mark, MarkKind, RichText};

/// A per-field edit against a base corpus. Ops apply left-to-right, consuming
/// base positions; `Retain`/`Delete` advance the base cursor, `Insert` adds new
/// text. USV throughout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    pub ops: Vec<Op>,
}

/// One delta operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Keep `n` chars of the base unchanged.
    Retain(usize),
    /// Insert this text at the cursor.
    Insert(String),
    /// Drop `n` chars of the base.
    Delete(usize),
}

/// Which side of a same-position insertion a mapped point lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// Stay before inserted text.
    Before,
    /// Move after inserted text.
    After,
}

impl Delta {
    /// Apply to `base`, producing the new text. Ignores over-long
    /// `Retain`/`Delete` gracefully (clamps), so a mismatched delta cannot
    /// panic.
    pub fn apply(&self, base: &str) -> String {
        let chars: Vec<char> = base.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    let end = (i + n).min(chars.len());
                    out.extend(&chars[i..end]);
                    i = end;
                }
                Op::Delete(n) => {
                    i = (i + n).min(chars.len());
                }
                Op::Insert(s) => out.push_str(s),
            }
        }
        out.extend(&chars[i.min(chars.len())..]);
        out
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

/// Char-level diff via common-prefix / common-suffix trim: the change is one
/// `Delete` of the differing base middle and one `Insert` of the new middle.
///
/// Coarse by design for phase 1. A consequence: two disjoint edits collapse the
/// whole span between them into one delete+insert, so an anchor sitting between
/// them collapses and must relocate via the move detector. Anchor *survival*
/// still holds (relocation recovers moved text; unrelated edits keep the
/// surrounding retain), but the returned `Delta` is not a minimal edit script.
/// Phase 3 (the real edit surface + change log) replaces this with a Myers/LCS
/// diff; phase-1 anchor rebase does not need one.
pub fn diff(base: &str, new: &str) -> Delta {
    let a: Vec<char> = base.chars().collect();
    let b: Vec<char> = new.chars().collect();

    let mut p = 0usize;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0usize;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }

    let mut ops = Vec::new();
    if p > 0 {
        ops.push(Op::Retain(p));
    }
    let del = a.len() - p - s;
    if del > 0 {
        ops.push(Op::Delete(del));
    }
    let ins: String = b[p..b.len() - s].iter().collect();
    if !ins.is_empty() {
        ops.push(Op::Insert(ins));
    }
    if s > 0 {
        ops.push(Op::Retain(s));
    }
    Delta { ops }
}

/// The stale-text writer path: cold-parse `new_markdown`, char-diff it against
/// `base`, and carry `base`'s identity marks (anchors) forward, rebased through
/// the diff (re-homing verbatim block moves). The returned corpus is `new_rt`
/// (structure/marks/islands from the fresh import) plus the surviving anchors.
///
/// Returns the new corpus and the [`Delta`] used (the change log entry a phase-3
/// revision would record).
pub fn diff_import(
    base: &RichText,
    new_markdown: &str,
) -> Result<(RichText, Delta), crate::import::ImportError> {
    let mut new_rt = crate::import::from_markdown(new_markdown)?;
    let delta = diff(&base.text, &new_rt.text);

    let base_chars: Vec<char> = base.text.chars().collect();
    let new_chars: Vec<char> = new_rt.text.chars().collect();
    let inserted = delta.inserted_spans();
    for m in &base.marks {
        // Only identity marks live in the corpus but not in markdown; formatting
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
    // inside inserted text (not merely somewhere in the surviving corpus).
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
/// surviving text. Overlap (not full containment) is required because a coarse
/// diff can split a moved block across an inserted region and the retained
/// suffix; demanding containment would miss real moves, while demanding overlap
/// still rejects an unrelated occurrence sitting entirely in retained text.
/// Enforces [`MIN_MOVE`].
fn find_in_spans(hay: &[char], needle: &[char], spans: &[(usize, usize)]) -> Option<usize> {
    if needle.len() < MIN_MOVE || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| {
        &hay[i..i + needle.len()] == needle
            && spans
                .iter()
                .any(|&(s, e)| i < e && i + needle.len() > s)
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
    fn anchor_survives_edit_elsewhere() {
        // Anchor on "target" (chars 7..13); edit happens before it.
        let mut base = from_markdown("hello, target word").unwrap();
        base.marks.push(Mark {
            start: 7,
            end: 13,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        base.normalize();
        let (new_rt, _) = diff_import(&base, "why hello, target word").unwrap();
        let anchor = new_rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor preserved");
        assert_eq!(
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)].to_string(),
            "target"
        );
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
            new_rt.text[byte(&new_rt.text, anchor.start)..byte(&new_rt.text, anchor.end)].to_string(),
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

    fn byte(s: &str, char_idx: usize) -> usize {
        crate::usv::char_to_byte(s, char_idx)
    }
}
