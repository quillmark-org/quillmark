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

/// Position mapping failed because `base_revision` precedes the oldest entry
/// still retained in the ring buffer.
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
}

impl ChangeLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            revision: 0,
            entries: VecDeque::new(),
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
    /// Returns [`StaleRevision`] when `base_revision` is strictly before the
    /// oldest retained entry — the consumer must re-read at
    /// [`Self::revision`].
    pub fn map_pos(
        &self,
        field: &str,
        base_revision: u64,
        pos: usize,
        assoc: Assoc,
    ) -> Result<usize, StaleRevision> {
        if base_revision > self.revision {
            return Ok(pos);
        }
        if let Some(oldest) = self.oldest_retained() {
            // Revision 0 is the pre-edit baseline (no log entry); only evicted
            // *recorded* revisions are stale.
            if base_revision > 0 && base_revision < oldest {
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

    #[test]
    fn map_pos_stale_when_base_before_ring() {
        let mut log = ChangeLog::new(2);
        log.record("f", diff("a", "b")); // rev 1
        log.record("f", diff("b", "c")); // rev 2
        log.record("f", diff("c", "d")); // rev 3, evicts rev 1
        let err = log.map_pos("f", 1, 0, Assoc::Before).unwrap_err();
        assert_eq!(
            err,
            StaleRevision {
                base_revision: 1,
                oldest_retained: 2,
            }
        );
    }

    #[test]
    fn map_pos_chains_insertions_like_delta() {
        let mut log = ChangeLog::with_default_capacity();
        let d = diff("abcdef", "abcXYdef");
        log.record("f", d);
        assert_eq!(log.map_pos("f", 0, 3, Assoc::After).unwrap(), 5);
        assert_eq!(log.map_pos("f", 0, 3, Assoc::Before).unwrap(), 3);
    }
}
