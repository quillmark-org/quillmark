//! Bounded per-field change log with monotonic revision and composed
//! [`Delta::map_pos`](crate::delta::Delta::map_pos).
//!
//! Phase 3 PR-C/D: the live session records text splices, mark ops, and line ops
//! per schema field. Consumers map a position captured at revision *N* forward
//! through edits *N+1…*; entries older than the ring buffer are dropped —
//! callers that fall behind re-read at [`ChangeLog::revision`].

use std::collections::VecDeque;

use crate::delta::{Assoc, Delta};
use crate::ops::{LineOp, MarkOp};

/// Default ring capacity — enough for a typing burst without unbounded growth.
pub const DEFAULT_CAPACITY: usize = 256;

/// One committed field edit: text splice plus optional mark/line op streams.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    pub revision: u64,
    pub path: String,
    pub text_delta: Delta,
    pub mark_ops: Vec<MarkOp>,
    pub line_ops: Vec<LineOp>,
}

/// Position mapping failed because `base_revision` cannot be folded forward
/// against the retained log — either it precedes the oldest entry still
/// retained in the ring buffer, or it is in the future (greater than the
/// log's current revision). `oldest_retained` carries the ring's oldest
/// retained revision in the stale case; in the future-revision case there is
/// no meaningful "oldest retained" to report, so it carries the log's current
/// revision instead — the value the caller should re-read at either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleRevision {
    pub base_revision: u64,
    pub oldest_retained: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChangeLog {
    capacity: usize,
    revision: u64,
    entries: VecDeque<FieldChange>,
    /// Invalidation floor: a `map_pos` against any `base_revision` strictly
    /// below this fails closed. Raised to the post-bump revision by
    /// [`invalidate`](Self::invalidate); `0` before any whole-document rewrite
    /// bypasses the delta protocol.
    invalidated_below: u64,
}

impl ChangeLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            revision: 0,
            entries: VecDeque::new(),
            invalidated_below: 0,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Current revision — the revision of the last committed edit, or `0` before
    /// any edit.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Oldest revision still in the ring, or `None` when empty.
    pub fn oldest_retained(&self) -> Option<u64> {
        self.entries.front().map(|e| e.revision)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append a text-only field edit, bump revision, evict when over capacity.
    pub fn record(&mut self, path: impl Into<String>, text_delta: Delta) -> u64 {
        self.record_change(path, text_delta, [], [])
    }

    /// Append a full field edit bundle. Returns the new revision.
    pub fn record_change(
        &mut self,
        path: impl Into<String>,
        text_delta: Delta,
        mark_ops: impl Into<Vec<MarkOp>>,
        line_ops: impl Into<Vec<LineOp>>,
    ) -> u64 {
        self.revision = self.revision.saturating_add(1);
        let entry = FieldChange {
            revision: self.revision,
            path: path.into(),
            text_delta,
            mark_ops: mark_ops.into(),
            line_ops: line_ops.into(),
        };
        self.entries.push_back(entry);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        self.revision
    }

    /// Map a USV position in `field` forward from `base_revision` through
    /// subsequent text deltas for that field. Identity when `base_revision` is
    /// current or when no matching entries exist.
    ///
    /// Returns [`StaleRevision`] when `base_revision` cannot be folded forward
    /// against the retained log: either it is in the future (greater than
    /// [`Self::revision`] — a caller passing a revision it never observed), or
    /// it is stale (an entry the fold needs, `base_revision + 1`, has already
    /// been evicted). The consumer must re-read at [`Self::revision`].
    pub fn map_pos(
        &self,
        field: &str,
        base_revision: u64,
        pos: usize,
        assoc: Assoc,
    ) -> Result<usize, StaleRevision> {
        if base_revision > self.revision {
            return Err(StaleRevision {
                base_revision,
                oldest_retained: self.revision,
            });
        }
        // A whole-document rewrite raised the invalidation floor: every base
        // below it captured text this log's deltas no longer describe, so
        // folding them would silently lie. The floor is the oldest base still
        // mappable — a read taken at it composes identity forward through any
        // deltas recorded since.
        if base_revision < self.invalidated_below {
            return Err(StaleRevision {
                base_revision,
                oldest_retained: self.invalidated_below,
            });
        }
        if let Some(oldest) = self.oldest_retained() {
            // The fold below needs every entry with revision > base_revision,
            // i.e. starting at `base_revision + 1`. That entry is retained iff
            // `base_revision + 1 >= oldest`; anything older has already been
            // evicted and folding would silently skip it, corrupting the
            // result. Revision 0 is the pre-edit baseline (no log entry) but
            // is not exempt: once entry 1 is evicted, base 0 is stale too.
            if base_revision + 1 < oldest {
                return Err(StaleRevision {
                    base_revision,
                    oldest_retained: oldest,
                });
            }
        }
        let mapped = self
            .entries
            .iter()
            .filter(|e| e.revision > base_revision && e.path == field)
            .fold(pos, |p, e| e.text_delta.map_pos(p, assoc));
        Ok(mapped)
    }

    /// Entries with `revision > after`, in commit order.
    pub fn entries_after(&self, after: u64) -> impl Iterator<Item = &FieldChange> {
        self.entries.iter().filter(move |e| e.revision > after)
    }

    /// Invalidate the log for a whole-document rewrite that bypasses the
    /// delta protocol (`LiveSession::apply`): bump the revision, drop every
    /// entry, and raise the invalidation floor to the new revision. A later
    /// `map_pos` against any base below the floor fails closed with
    /// [`StaleRevision`] instead of silently composing through per-field
    /// deltas that no longer describe the current text; a base at or above the
    /// floor still maps, folding forward through any deltas recorded after the
    /// rewrite. Returns the new revision.
    pub fn invalidate(&mut self) -> u64 {
        self.revision = self.revision.saturating_add(1);
        self.entries.clear();
        self.invalidated_below = self.revision;
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta::diff;

    #[test]
    fn revision_starts_at_zero_and_increments() {
        let mut log = ChangeLog::with_default_capacity();
        assert_eq!(log.revision(), 0);
        let r1 = log.record("subject", diff("a", "b"));
        assert_eq!(r1, 1);
        let r2 = log.record("tag_line", diff("x", "y"));
        assert_eq!(r2, 2);
        assert_eq!(log.revision(), 2);
    }

    #[test]
    fn ring_evicts_oldest() {
        let mut log = ChangeLog::new(2);
        log.record("a", diff("", "1"));
        log.record("b", diff("", "2"));
        assert_eq!(log.oldest_retained(), Some(1));
        log.record("c", diff("", "3"));
        assert_eq!(log.len(), 2);
        assert_eq!(log.oldest_retained(), Some(2));
        assert!(log.entries_after(0).any(|e| e.path == "b"));
        assert!(!log.entries_after(0).any(|e| e.path == "a"));
    }

    #[test]
    fn map_pos_composes_same_field() {
        let mut log = ChangeLog::with_default_capacity();
        // "hello" -> insert "!" at 5 -> "hello!" -> replace "hello" prefix...
        // Use two explicit diffs on one field.
        let d1 = diff("abc", "abXc"); // insert X at 2
        let d2 = diff("abXc", "abXYc"); // insert Y at 3
        log.record("body", d1);
        log.record("body", d2);
        // Position 2 (before X) stays 2 through first insert, then 2 through second
        assert_eq!(log.map_pos("body", 0, 2, Assoc::Before).unwrap(), 2);
        // Position 3 (at end after first edit: after X) maps through second insert
        assert_eq!(log.map_pos("body", 1, 3, Assoc::After).unwrap(), 4);
    }

    #[test]
    fn map_pos_ignores_other_fields() {
        let mut log = ChangeLog::with_default_capacity();
        log.record("body", diff("xxx", "yyy"));
        assert_eq!(log.map_pos("subject", 0, 1, Assoc::Before).unwrap(), 1);
    }

    #[test]
    fn map_pos_identity_at_current_revision() {
        let mut log = ChangeLog::with_default_capacity();
        log.record("f", diff("abc", "abd"));
        assert_eq!(
            log.map_pos("f", log.revision(), 1, Assoc::Before).unwrap(),
            1
        );
    }

    /// The off-by-one boundary case pinned above from the other side: base ==
    /// `oldest - 1` still maps correctly after eviction, because the fold
    /// only ever needs entries with revision > base_revision — it never reads
    /// the evicted entry itself.
    #[test]
    fn map_pos_succeeds_at_oldest_minus_one() {
        let mut log = ChangeLog::new(2);
        log.record("f", diff("hello", "help")); // rev 1: Retain(3), Delete(2), Insert("p")
        log.record("f", diff("help", "helpful")); // rev 2: Retain(4), Insert("ful")
        log.record("f", diff("helpful", "helpfully")); // rev 3: Retain(7), Insert("ly"), evicts rev 1
        assert_eq!(log.oldest_retained(), Some(2));

        // Position 3 in "help" (the state as of rev 1, right before the
        // trailing "p") maps through rev 2 and rev 3 without needing rev 1,
        // which is already evicted.
        assert_eq!(log.map_pos("f", 1, 3, Assoc::Before).unwrap(), 3);
    }

    /// The exact bug from GH-845: a position captured at the pre-edit
    /// baseline (revision 0, before any edit) must go stale once the entry
    /// it needs (rev 1) is evicted — folding only the retained suffix (rev 2,
    /// rev 3) would otherwise silently return a wrong position instead of
    /// erroring.
    ///
    /// Ground truth, tracing the full (unevicted) history: position 4 in
    /// "hello" (the 'o') sits inside the `Delete(2)` of rev 1
    /// (`diff("hello", "help")` = `Retain(3), Delete(2), Insert("p")`), which
    /// collapses it to 3; rev 2 (`Retain(4), Insert("ful")`) and rev 3
    /// (`Retain(7), Insert("ly")`) both retain position 3 unchanged (it falls
    /// inside their leading `Retain`). So the correct forward mapping is 3.
    /// A fold that skips the evicted rev 1 and starts from the raw base
    /// position 4 instead would run rev 2's `Insert` at position 4 (the
    /// insertion point, `Assoc::Before` stays put) giving 4, then rev 3's
    /// leading `Retain(7)` passes 4 through unchanged — a wrong answer (4
    /// instead of 3) returned with no indication anything was skipped. The
    /// fix must refuse to compute either number and return `StaleRevision`.
    #[test]
    fn map_pos_base_zero_stale_after_prefix_eviction() {
        let mut log = ChangeLog::new(2);
        log.record("f", diff("hello", "help")); // rev 1, evicted below
        log.record("f", diff("help", "helpful")); // rev 2
        log.record("f", diff("helpful", "helpfully")); // rev 3, evicts rev 1
        assert_eq!(log.oldest_retained(), Some(2));

        let err = log.map_pos("f", 0, 4, Assoc::Before).unwrap_err();
        assert_eq!(
            err,
            StaleRevision {
                base_revision: 0,
                oldest_retained: 2,
            }
        );
    }

    #[test]
    fn map_pos_errors_on_future_revision() {
        let mut log = ChangeLog::with_default_capacity();
        log.record("f", diff("a", "b")); // rev 1
        assert_eq!(log.revision(), 1);

        // base_revision 5 was never observed — no entry, no ring position —
        // and must error rather than silently pass `pos` through unchanged.
        let err = log.map_pos("f", 5, 0, Assoc::Before).unwrap_err();
        assert_eq!(
            err,
            StaleRevision {
                base_revision: 5,
                oldest_retained: 1,
            }
        );
    }

    #[test]
    fn invalidate_fails_closed_for_pre_apply_bases() {
        let mut log = ChangeLog::with_default_capacity();
        log.record("f", diff("a", "b")); // rev 1
        log.record("f", diff("b", "c")); // rev 2
        let r = log.invalidate();
        assert_eq!(r, 3);
        assert_eq!(log.revision(), 3);
        // Any base captured before the invalidation is now stale...
        let err = log.map_pos("f", 2, 0, Assoc::Before).unwrap_err();
        assert_eq!(
            err,
            StaleRevision {
                base_revision: 2,
                oldest_retained: 3,
            }
        );
        // ...but a read taken exactly at the new revision still resolves.
        assert_eq!(log.map_pos("f", 3, 5, Assoc::Before).unwrap(), 5);
    }

    /// Deltas recorded *after* an invalidation fold forward normally from the
    /// floor revision, while every pre-invalidation base stays stale: the
    /// floor partitions mappable from unmappable bases, and records past it
    /// re-arm the delta protocol.
    #[test]
    fn map_pos_after_invalidate_maps_from_floor() {
        let mut log = ChangeLog::with_default_capacity();
        log.record("f", diff("a", "b")); // rev 1
        log.record("f", diff("b", "c")); // rev 2
        assert_eq!(log.invalidate(), 3); // floor := 3
        log.record("f", diff("abcdef", "abcXYdef")); // rev 4, base == floor

        // A read at the floor (rev 3) folds through the rev-4 insert…
        assert_eq!(log.map_pos("f", 3, 3, Assoc::After).unwrap(), 5);
        // …and a read at rev 4 is current, so identity.
        assert_eq!(log.map_pos("f", 4, 3, Assoc::Before).unwrap(), 3);
        // But every pre-invalidation base is still stale against the floor,
        // even though its needed entry was never in the ring.
        assert_eq!(
            log.map_pos("f", 2, 0, Assoc::Before).unwrap_err(),
            StaleRevision {
                base_revision: 2,
                oldest_retained: 3,
            }
        );
    }

}
