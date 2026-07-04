//! Cold-parse + corpus diff → delta, and position rebasing across it (Spike A).
//! This is the stale-text writer's mechanism: an MCP `update_document` or a
//! saved `.qmd` arrives as a full new markdown document; we cold-parse it to a
//! corpus, char-diff against the base corpus, and rebase every mark/anchor
//! through the resulting delta — no preservation contract on the LLM.
//!
//! It also exhibits the documented weak spot: a **paragraph reorder** is
//! delete+insert to a naive differ, so an anchor inside the moved text rebases
//! to the deletion's collapse point (detached) unless a move detector re-homes
//! it. Both paths are here so the finding can measure the difference.

use crate::model::CharRange;

/// One delta op in char (USV) units. `Delete` carries its text so a move
/// detector can match it against a reinsertion. Quill-Delta semantics:
/// `retain`/`insert`/`delete` against a base revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Retain(usize),
    Insert(String),
    Delete(String),
}

/// Bias for mapping a boundary that sits exactly at an edit: `Before` sticks to
/// the left (a mark's end), `After` to the right (a mark's start).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bias {
    Before,
    After,
}

/// Char-level diff `base → target` via LCS. Deterministic; O(n·m) — fine for
/// the spike's inputs, and the shape of a real Myers/`similar` diff.
pub fn diff_chars(base: &str, target: &str) -> Vec<Op> {
    let a: Vec<char> = base.chars().collect();
    let b: Vec<char> = target.chars().collect();
    let n = a.len();
    let m = b.len();

    // LCS length table.
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    // Backtrack into coalesced ops.
    let mut ops: Vec<Op> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    let push = |ops: &mut Vec<Op>, op: Op| match (ops.last_mut(), &op) {
        (Some(Op::Retain(x)), Op::Retain(y)) => *x += y,
        (Some(Op::Insert(s)), Op::Insert(t)) => s.push_str(t),
        (Some(Op::Delete(s)), Op::Delete(t)) => s.push_str(t),
        _ => ops.push(op),
    };
    while i < n && j < m {
        if a[i] == b[j] {
            push(&mut ops, Op::Retain(1));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            push(&mut ops, Op::Delete(a[i].to_string()));
            i += 1;
        } else {
            push(&mut ops, Op::Insert(b[j].to_string()));
            j += 1;
        }
    }
    while i < n {
        push(&mut ops, Op::Delete(a[i].to_string()));
        i += 1;
    }
    while j < m {
        push(&mut ops, Op::Insert(b[j].to_string()));
        j += 1;
    }
    ops
}

/// Map a base char position through the delta (CodeMirror `ChangeDesc.mapPos`).
/// Returns `(new_pos, inside_deletion)`; `inside_deletion` is true when the
/// position fell strictly inside deleted text (it collapses to the deletion
/// point). `bias` breaks the tie at an exact edit boundary.
pub fn map_pos(ops: &[Op], pos: usize, bias: Bias) -> (usize, bool) {
    let mut old = 0usize;
    let mut new = 0usize;
    for op in ops {
        match op {
            Op::Retain(n) => {
                if pos <= old + n {
                    return (new + (pos - old), false);
                }
                old += n;
                new += n;
            }
            Op::Insert(s) => {
                let len = s.chars().count();
                // An insertion at exactly `pos`: `After` bias steps over it.
                if pos == old && bias == Bias::Before {
                    return (new, false);
                }
                new += len;
            }
            Op::Delete(s) => {
                let len = s.chars().count();
                if pos < old + len {
                    // Inside (or at the right edge of) the deletion.
                    if pos <= old {
                        return (new, false); // at the left edge — not deleted
                    }
                    return (new, true); // strictly inside → collapse, flagged
                }
                old += len;
            }
        }
    }
    (new, false)
}

/// Rebase a mark's range through the delta with no move awareness. A formatting
/// mark whose whole span was deleted collapses to empty (a drop); a surviving
/// span keeps whatever text remained. Returns `None` when the mark collapses to
/// empty *and* it is not an identity mark (caller decides via `is_anchor`).
pub fn rebase_range(ops: &[Op], range: CharRange) -> (CharRange, RebaseFate) {
    let (s, s_del) = map_pos(ops, range.start, Bias::After);
    let (e, e_del) = map_pos(ops, range.end, Bias::Before);
    let new = CharRange::new(s.min(e), s.max(e));
    let fate = if range.is_empty() {
        // Anchor: it survives as a point wherever it maps; `detached` if the
        // char it was pinned to was deleted.
        if s_del {
            RebaseFate::AnchorDetached
        } else {
            RebaseFate::Kept
        }
    } else if new.is_empty() {
        RebaseFate::Dropped
    } else if s_del || e_del {
        RebaseFate::Shrunk
    } else {
        RebaseFate::Kept
    };
    (new, fate)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebaseFate {
    /// Mapped cleanly, no touched text.
    Kept,
    /// Part of the span was deleted; the mark survives over what remained.
    Shrunk,
    /// The whole span was deleted; a formatting mark is gone.
    Dropped,
    /// A zero-width anchor whose pinned char was deleted — survives as a point
    /// but no longer attached to its original text (the reorder weak spot).
    AnchorDetached,
}

/// Minimum moved-block length (chars) worth re-homing. Below this a "move" is
/// noise (a shared word), not a reorder.
const MIN_MOVE: usize = 4;

/// Move detection: find the longest run of text that the delta both deletes
/// (from the base) and inserts (into the target), and report it as a
/// `(base_start, target_start, len)` move. Robust to how the char-diff splits a
/// reorder — a moved paragraph's leading/trailing `\n` may attach to either
/// side, so exact whole-segment matching is too brittle; longest-common-
/// substring over the deleted vs inserted text finds the moved block itself.
/// Limit: one move, and the block must survive as a contiguous run in both the
/// deleted and inserted text (true for a single reorder; a move that is also
/// edited mid-block degrades to a shorter match or none — stated in the
/// finding).
pub fn detect_move(ops: &[Op]) -> Option<Move> {
    // Concatenate deleted text (with each char's base position) and inserted
    // text (with each char's target position).
    let mut old = 0usize;
    let mut new = 0usize;
    let mut del: Vec<char> = Vec::new();
    let mut del_pos: Vec<usize> = Vec::new();
    let mut ins: Vec<char> = Vec::new();
    let mut ins_pos: Vec<usize> = Vec::new();
    for op in ops {
        match op {
            Op::Retain(n) => {
                old += n;
                new += n;
            }
            Op::Delete(s) => {
                for c in s.chars() {
                    del.push(c);
                    del_pos.push(old);
                    old += 1;
                }
            }
            Op::Insert(s) => {
                for c in s.chars() {
                    ins.push(c);
                    ins_pos.push(new);
                    new += 1;
                }
            }
        }
    }
    if del.is_empty() || ins.is_empty() {
        return None;
    }

    // Longest common substring (DP over the two char vectors).
    let (n, m) = (del.len(), ins.len());
    let mut dp = vec![0usize; m + 1];
    let mut best_len = 0usize;
    let mut best_di = 0usize; // end index (exclusive) in `del`
    let mut best_ii = 0usize; // end index (exclusive) in `ins`
    for i in 1..=n {
        let mut prev = 0usize;
        for j in 1..=m {
            let tmp = dp[j];
            if del[i - 1] == ins[j - 1] {
                dp[j] = prev + 1;
                if dp[j] > best_len {
                    best_len = dp[j];
                    best_di = i;
                    best_ii = j;
                }
            } else {
                dp[j] = 0;
            }
            prev = tmp;
        }
    }

    if best_len < MIN_MOVE {
        return None;
    }
    let base_start = del_pos[best_di - best_len];
    let target_start = ins_pos[best_ii - best_len];
    Some(Move {
        base_start,
        target_start,
        len: best_len,
    })
}

/// A detected block move in char units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub base_start: usize,
    pub target_start: usize,
    pub len: usize,
}

/// Move-aware anchor rebase: if `pos` falls inside a detected move, re-home it
/// into the moved block at the target; otherwise fall back to [`map_pos`].
pub fn rebase_anchor_move_aware(ops: &[Op], pos: usize) -> usize {
    if let Some(mv) = detect_move(ops) {
        if pos >= mv.base_start && pos < mv.base_start + mv.len {
            return mv.target_start + (pos - mv.base_start);
        }
    }
    map_pos(ops, pos, Bias::After).0
}
