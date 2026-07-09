//! Mark and line op channels — structural edits separate from text splices.
//!
//! Phase 3 PR-D: [`MarkOp`] and [`LineOp`] apply after [`RichText::apply_text_delta`]
//! in one bundle; mark ranges are in post-delta coordinates. Line split/join
//! also splice `\n` in `text`; position mapping through the change log still
//! composes [`Delta::map_pos`](crate::delta::Delta::map_pos) on the text delta
//! channel — record `\n` edits there when mapping stale positions.

use crate::delta::{Assoc, Delta, Op};
use crate::model::{Container, Island, Line, LineKind, Mark, MarkKind, RichText, Usv, ISLAND_SLOT};
use crate::usv::char_to_byte;

/// A mark edit in post-text-delta coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkOp {
    /// Add a mark over `[start, end)`.
    Add {
        start: Usv,
        end: Usv,
        kind: MarkKind,
    },
    /// Drop formatting marks of `kind` overlapping `[start, end)`.
    Remove {
        start: Usv,
        end: Usv,
        kind: MarkKind,
    },
    /// Drop one identity anchor by id.
    RemoveAnchor { id: String },
}

/// A line/block edit. Split/join splice `\n` in `text`; set ops touch metadata
/// only.
#[derive(Debug, Clone, PartialEq)]
pub enum LineOp {
    /// Paragraph break at `at`: insert `\n` and split the line metadata.
    Split { at: Usv },
    /// Join line `line` with the next — remove the `\n` between them.
    Join { line: usize },
    /// Replace a line's block role.
    SetKind { line: usize, kind: LineKind },
    /// Replace a line's container path.
    SetContainers {
        line: usize,
        containers: Vec<Container>,
    },
}

/// Why an apply failed — range or line index out of bounds, or invariants
/// broken before normalization could repair them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    MarkOutOfRange {
        start: Usv,
        end: Usv,
        len: Usv,
    },
    LineOutOfRange {
        line: usize,
        lines: usize,
    },
    SplitPositionOutOfRange {
        at: Usv,
        len: Usv,
    },
    SplitAtNewline {
        at: Usv,
    },
    LineCountMismatch {
        lines: usize,
        segments: usize,
    },
    /// The text delta's expected base length disagreed with the corpus —
    /// it was built against a different revision.
    DeltaBaseMismatch {
        expected: usize,
        actual: usize,
    },
    /// An `Op::Insert` carried a raw [`ISLAND_SLOT`]. Islands are structurally
    /// uneditable through the text channel — a slot inserted here would have no
    /// backing [`Island`], an orphaned-slot invariant violation. Islands are
    /// created through their own channel, never a text splice.
    IslandSlotInInsert,
}

impl RichText {
    /// Splice `text` via `delta`, rebase marks, sync `lines` to `\n` changes,
    /// cascade island removal for any deleted slot, then normalize.
    ///
    /// Islands stay in lockstep with their [`ISLAND_SLOT`] chars: a delta that
    /// *deletes* a slot drops the corresponding [`Island`] (the content goes
    /// away with its slot); a delta that *inserts* a raw slot is rejected
    /// ([`ApplyError::IslandSlotInInsert`]) — islands are created through their
    /// own channel, never a text splice, so a slot arriving here would orphan.
    pub fn apply_text_delta(&mut self, delta: &Delta) -> Result<(), ApplyError> {
        // Reject before mutating: a raw slot in an insert would create a slot
        // with no backing island. Checked up front so the corpus is untouched
        // on this error.
        for op in &delta.ops {
            if let Op::Insert(s) = op {
                if s.contains(ISLAND_SLOT) {
                    return Err(ApplyError::IslandSlotInInsert);
                }
            }
        }

        let old_chars: Vec<char> = self.text.chars().collect();
        let old_lines = self.lines.clone();
        let new_text = delta
            .try_apply(&self.text)
            .map_err(|e| ApplyError::DeltaBaseMismatch {
                expected: e.expected,
                actual: e.actual,
            })?;

        for m in &mut self.marks {
            if m.start == m.end {
                let p = delta.map_pos(m.start, Assoc::Before);
                m.start = p;
                m.end = p;
            } else {
                m.start = delta.map_pos(m.start, Assoc::After);
                m.end = delta.map_pos(m.end, Assoc::Before);
            }
        }
        self.marks.retain(|m| {
            m.start <= m.end
                && m.end <= new_text.chars().count()
                && (m.start < m.end || !m.kind.is_formatting())
        });

        self.text = new_text;
        self.lines = sync_lines_for_delta(&old_chars, &old_lines, delta)?;
        let old_islands = std::mem::take(&mut self.islands);
        self.islands = sync_islands_for_delta(&old_chars, old_islands, delta);
        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        self.normalize();
        Ok(())
    }

    /// Apply mark ops in post-text-delta coordinates, then normalize.
    pub fn apply_mark_ops(&mut self, ops: &[MarkOp]) -> Result<(), ApplyError> {
        let len = self.len_usv();
        for op in ops {
            match op {
                MarkOp::Add { start, end, kind } => {
                    if *start > *end || *end > len {
                        return Err(ApplyError::MarkOutOfRange {
                            start: *start,
                            end: *end,
                            len,
                        });
                    }
                    if kind.is_formatting() && start == end {
                        return Err(ApplyError::MarkOutOfRange {
                            start: *start,
                            end: *end,
                            len,
                        });
                    }
                    self.marks.push(Mark {
                        start: *start,
                        end: *end,
                        kind: kind.clone(),
                    });
                }
                MarkOp::Remove { start, end, kind } => {
                    if *start > *end || *end > len {
                        return Err(ApplyError::MarkOutOfRange {
                            start: *start,
                            end: *end,
                            len,
                        });
                    }
                    self.marks.retain(|m| {
                        !same_mark_kind(&m.kind, kind)
                            || !ranges_overlap(m.start, m.end, *start, *end)
                    });
                }
                MarkOp::RemoveAnchor { id } => {
                    self.marks
                        .retain(|m| !matches!(&m.kind, MarkKind::Anchor { id: aid } if aid == id));
                }
            }
        }
        self.normalize();
        Ok(())
    }

    /// Apply line ops — split/join splice `\n`; set ops touch metadata only.
    pub fn apply_line_ops(&mut self, ops: &[LineOp]) -> Result<(), ApplyError> {
        for op in ops {
            match op {
                LineOp::Split { at } => self.split_line(*at)?,
                LineOp::Join { line } => self.join_line(*line)?,
                LineOp::SetKind { line, kind } => {
                    let line = self.line_mut(*line)?;
                    line.kind = kind.clone();
                }
                LineOp::SetContainers { line, containers } => {
                    let line = self.line_mut(*line)?;
                    line.containers = containers.clone();
                }
            }
        }
        self.normalize();
        Ok(())
    }

    /// One committed field edit bundle: text delta, then line ops, then marks.
    pub fn apply_field_change(
        &mut self,
        text_delta: &Delta,
        line_ops: &[LineOp],
        mark_ops: &[MarkOp],
    ) -> Result<(), ApplyError> {
        self.apply_text_delta(text_delta)?;
        self.apply_line_ops(line_ops)?;
        self.apply_mark_ops(mark_ops)
    }

    fn line_mut(&mut self, line: usize) -> Result<&mut Line, ApplyError> {
        let lines = self.lines.len();
        self.lines
            .get_mut(line)
            .ok_or(ApplyError::LineOutOfRange { line, lines })
    }

    fn split_line(&mut self, at: Usv) -> Result<(), ApplyError> {
        let char_indices: Vec<(usize, char)> = self.text.char_indices().collect();
        let len = char_indices.len();
        if at > len {
            return Err(ApplyError::SplitPositionOutOfRange { at, len });
        }
        if at > 0 && char_indices[at - 1].1 == '\n' {
            return Err(ApplyError::SplitAtNewline { at });
        }
        if at < len && char_indices[at].1 == '\n' {
            return Err(ApplyError::SplitAtNewline { at });
        }

        // `at`'s newline-adjacency neighbors, `at`'s byte offset, and the
        // newline count before `at` (== the post-insert line index, since the
        // insertion lands at index `at`, not before it) all come from this
        // one pass over `char_indices`, instead of four separate text scans.
        let byte = char_indices.get(at).map_or(self.text.len(), |&(b, _)| b);
        let line_idx = char_indices[..at].iter().filter(|&(_, c)| *c == '\n').count();
        self.text.insert(byte, '\n');

        let template = self
            .lines
            .get(line_idx)
            .cloned()
            .unwrap_or_else(default_para_line);
        let mut new_line = template;
        new_line.continues = false;
        self.lines.insert(line_idx + 1, new_line);

        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        Ok(())
    }

    fn join_line(&mut self, line: usize) -> Result<(), ApplyError> {
        if line + 1 >= self.lines.len() {
            return Err(ApplyError::LineOutOfRange {
                line,
                lines: self.lines.len(),
            });
        }
        let nl = newline_at_line_boundary(&self.text, line)?;
        let byte = char_to_byte(&self.text, nl);
        self.text.remove(byte);

        self.lines.remove(line + 1);

        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        Ok(())
    }
}

fn default_para_line() -> Line {
    Line {
        kind: LineKind::Para,
        containers: Vec::new(),
        continues: false,
    }
}

fn same_mark_kind(a: &MarkKind, b: &MarkKind) -> bool {
    match (a, b) {
        (MarkKind::Strong, MarkKind::Strong)
        | (MarkKind::Emph, MarkKind::Emph)
        | (MarkKind::Underline, MarkKind::Underline)
        | (MarkKind::Strike, MarkKind::Strike)
        | (MarkKind::Code, MarkKind::Code) => true,
        (MarkKind::Link { url: u1 }, MarkKind::Link { url: u2 }) => u1 == u2,
        (MarkKind::Anchor { id: i1 }, MarkKind::Anchor { id: i2 }) => i1 == i2,
        (MarkKind::Unknown { tag: t1, attrs: a1 }, MarkKind::Unknown { tag: t2, attrs: a2 }) => {
            t1 == t2 && a1 == a2
        }
        _ => false,
    }
}

fn ranges_overlap(a0: Usv, a1: Usv, b0: Usv, b1: Usv) -> bool {
    a0 < b1 && b0 < a1
}

/// Walk `delta` over `old_chars` and mirror `\n` insert/delete in `lines`.
fn sync_lines_for_delta(
    old_chars: &[char],
    old_lines: &[Line],
    delta: &Delta,
) -> Result<Vec<Line>, ApplyError> {
    let mut lines = old_lines.to_vec();
    let mut old = 0usize;
    let mut line_idx = 0usize;

    for op in &delta.ops {
        match op {
            Op::Retain(n) => {
                for _ in 0..*n {
                    if old >= old_chars.len() {
                        break;
                    }
                    if old_chars[old] == '\n' {
                        line_idx += 1;
                    }
                    old += 1;
                }
            }
            Op::Delete(n) => {
                for _ in 0..*n {
                    if old >= old_chars.len() {
                        break;
                    }
                    if old_chars[old] == '\n' && line_idx + 1 < lines.len() {
                        lines.remove(line_idx + 1);
                    }
                    old += 1;
                }
            }
            Op::Insert(s) => {
                for c in s.chars() {
                    if c == '\n' {
                        let template = lines
                            .get(line_idx)
                            .cloned()
                            .unwrap_or_else(default_para_line);
                        let mut new_line = template;
                        new_line.continues = false;
                        if line_idx < lines.len() {
                            lines.insert(line_idx + 1, new_line);
                        } else {
                            lines.push(new_line);
                        }
                        line_idx += 1;
                    }
                }
            }
        }
    }
    Ok(lines)
}

/// Walk `delta` over `old_chars` and drop any island whose [`ISLAND_SLOT`] char
/// was deleted (cascade removal — the island's content goes away with its slot).
/// Islands are stored in slot order, so the Nth slot backs the Nth island; a
/// deleted slot drops its island and the survivors renumber implicitly. Raw
/// slot *inserts* are rejected upstream, so an insert never mints a new slot.
fn sync_islands_for_delta(
    old_chars: &[char],
    old_islands: Vec<Island>,
    delta: &Delta,
) -> Vec<Island> {
    let mut keep = vec![true; old_islands.len()];
    let mut old = 0usize;
    let mut slot_idx = 0usize;

    for op in &delta.ops {
        match op {
            Op::Retain(n) => {
                for _ in 0..*n {
                    if old >= old_chars.len() {
                        break;
                    }
                    if old_chars[old] == ISLAND_SLOT {
                        slot_idx += 1;
                    }
                    old += 1;
                }
            }
            Op::Delete(n) => {
                for _ in 0..*n {
                    if old >= old_chars.len() {
                        break;
                    }
                    if old_chars[old] == ISLAND_SLOT {
                        if let Some(k) = keep.get_mut(slot_idx) {
                            *k = false;
                        }
                        slot_idx += 1;
                    }
                    old += 1;
                }
            }
            // Inserts add no slots (a raw ISLAND_SLOT insert is rejected before
            // this walk), so they never touch the island list.
            Op::Insert(_) => {}
        }
    }

    old_islands
        .into_iter()
        .zip(keep)
        .filter_map(|(island, keep)| keep.then_some(island))
        .collect()
}

fn newline_at_line_boundary(text: &str, line: usize) -> Result<Usv, ApplyError> {
    let mut current = 0usize;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            if current == line {
                return Ok(i);
            }
            current += 1;
        }
    }
    Err(ApplyError::LineOutOfRange {
        line,
        lines: text.chars().filter(|&c| c == '\n').count() + 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta::diff;
    use crate::import::from_markdown;

    #[test]
    fn apply_text_delta_rebases_marks() {
        let mut rt = from_markdown("hello").unwrap();
        rt.marks.push(Mark {
            start: 1,
            end: 4,
            kind: MarkKind::Strong,
        });
        rt.normalize();
        let d = diff("hello", "hXello");
        rt.apply_text_delta(&d).unwrap();
        let strong = rt
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::Strong))
            .unwrap();
        assert_eq!((strong.start, strong.end), (2, 5));
        assert_eq!(rt.text, "hXello");
    }

    #[test]
    fn apply_mark_ops_add_and_remove() {
        let mut rt = from_markdown("abcd").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 2,
            kind: MarkKind::Emph,
        }])
        .unwrap();
        assert!(rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Emph)));
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 0,
            end: 4,
            kind: MarkKind::Emph,
        }])
        .unwrap();
        assert!(!rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Emph)));
    }

    #[test]
    fn apply_text_delta_splits_lines_on_newline_insert() {
        let mut rt = from_markdown("one two").unwrap();
        let d = diff("one two", "one\ntwo");
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.segment_count(), 2);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn line_op_split_and_join() {
        let mut rt = from_markdown("onetwo").unwrap();
        rt.apply_line_ops(&[LineOp::Split { at: 3 }]).unwrap();
        assert_eq!(rt.text, "one\ntwo");
        assert_eq!(rt.lines.len(), 2);

        rt.apply_line_ops(&[LineOp::Join { line: 0 }]).unwrap();
        assert_eq!(rt.text, "onetwo");
        assert_eq!(rt.lines.len(), 1);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn line_op_set_kind() {
        let mut rt = from_markdown("title").unwrap();
        rt.apply_line_ops(&[LineOp::SetKind {
            line: 0,
            kind: LineKind::Heading { level: 2 },
        }])
        .unwrap();
        assert!(matches!(rt.lines[0].kind, LineKind::Heading { level: 2 }));
    }

    fn island(id: &str) -> Island {
        Island {
            id: id.into(),
            island_type: "image".into(),
            props: serde_json::json!({}),
            loss: crate::model::Loss::Lossless,
        }
    }

    /// A single-line corpus `a￼b` (one inline island slot, one backing island).
    fn corpus_with_island() -> RichText {
        let mut rt = RichText::empty();
        rt.text = format!("a{ISLAND_SLOT}b");
        rt.lines = vec![Line {
            kind: LineKind::Para,
            containers: vec![],
            continues: false,
        }];
        rt.islands = vec![island("i1")];
        assert_eq!(rt.validate(), Ok(()));
        rt
    }

    #[test]
    fn delete_slot_cascades_island_removal() {
        let mut rt = corpus_with_island();
        // Delete the slot char at index 1 (`a￼b` -> `ab`).
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Delete(1), Op::Retain(1)],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, "ab");
        assert!(rt.islands.is_empty(), "island cascaded away with its slot");
        // slot count now equals islands.len() — validate confirms the sync.
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn delete_one_of_two_slots_removes_the_matching_island() {
        let mut rt = RichText::empty();
        rt.text = format!("{ISLAND_SLOT}x{ISLAND_SLOT}");
        rt.lines = vec![Line {
            kind: LineKind::Para,
            containers: vec![],
            continues: false,
        }];
        rt.islands = vec![island("first"), island("second")];
        assert_eq!(rt.validate(), Ok(()));

        // Delete the FIRST slot (index 0): `￼x￼` -> `x￼`.
        let d = Delta {
            ops: vec![Op::Delete(1), Op::Retain(2)],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, format!("x{ISLAND_SLOT}"));
        // The surviving island is the second one — the cascade removed the
        // island whose slot was deleted, not merely the last entry.
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].id, "second");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_raw_slot_is_rejected() {
        let mut rt = from_markdown("ab").unwrap();
        // An Op::Insert carrying a raw U+FFFC would orphan a slot — reject it.
        let d = Delta {
            ops: vec![
                Op::Retain(1),
                Op::Insert(ISLAND_SLOT.to_string()),
                Op::Retain(1),
            ],
        };
        assert_eq!(rt.apply_text_delta(&d), Err(ApplyError::IslandSlotInInsert));
        // Corpus untouched on the rejected insert (checked before any mutation).
        assert_eq!(rt.text, "ab");
        assert!(rt.islands.is_empty());
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_slot_mixed_with_text_is_rejected() {
        // Even embedded in a larger insert, a raw slot is rejected wholesale.
        let mut rt = from_markdown("ab").unwrap();
        let d = Delta {
            ops: vec![Op::Retain(2), Op::Insert(format!("x{ISLAND_SLOT}y"))],
        };
        assert_eq!(rt.apply_text_delta(&d), Err(ApplyError::IslandSlotInInsert));
        assert_eq!(rt.text, "ab");
    }

    #[test]
    fn apply_field_change_bundle_order() {
        let mut rt = from_markdown("abc").unwrap();
        let d = diff("abc", "abXc");
        rt.apply_field_change(
            &d,
            &[],
            &[MarkOp::Add {
                start: 3,
                end: 4,
                kind: MarkKind::Strong,
            }],
        )
        .unwrap();
        let strong = rt
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::Strong))
            .unwrap();
        assert_eq!((strong.start, strong.end), (3, 4));
        assert_eq!(rt.text, "abXc");
    }
}
