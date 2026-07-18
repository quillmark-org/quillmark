//! Mark and line op channels — structural edits separate from text splices.
//!
//! [`MarkOp`] and [`LineOp`] apply after [`Content::apply_text_delta`] in one
//! bundle. Mark ranges are in **final-text coordinates**: mark ops run after
//! line ops and validate against the post-line-op length, so a producer
//! computes them in the only frame it can — the text as it stands once the
//! delta and line ops have landed. Line split/join splice a `\n` in `text` and
//! rebase marks through that one-char change with
//! [`Delta::map_pos`](crate::delta::Delta::map_pos), the same mapping the
//! text-delta channel uses, so a mark's coordinates track the splice rather
//! than drifting.

use crate::delta::{Assoc, Delta, Op};
use crate::model::{Container, Island, Line, LineKind, Mark, MarkKind, Content, Usv, ISLAND_SLOT};
use crate::normalize::is_bidi_char;
use crate::usv::char_to_byte;
use std::borrow::Cow;

/// A mark edit in final-text coordinates (post-delta, post-line-op).
#[derive(Debug, Clone, PartialEq)]
pub enum MarkOp {
    /// Add a mark over `[start, end)`.
    Add {
        start: Usv,
        end: Usv,
        kind: MarkKind,
    },
    /// Un-format `kind` over `[start, end)`: subtract the range from each
    /// overlapping same-kind *formatting* mark, keeping the non-overlapping
    /// fragments (a mid-run removal punches a hole; `normalize` drops any
    /// zero-width fragment an edge-aligned removal leaves). Non-formatting
    /// (identity/unknown) handles can't be range-fragmented, so an overlapping
    /// one is dropped whole — anchors normally go through [`MarkOp::RemoveAnchor`].
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
    /// Set (or clear) a line's `continues` flag — whether it continues the
    /// previous line's block across a within-block hard break (a markdown hard
    /// break, a code fence's interior line) rather than starting a new block.
    /// The op-grained twin of the value that `install` already round-trips:
    /// `split`/`join`/text-delta `\n` insertion all mint `continues: false`
    /// lines, so without this a hard break or a new code-fence interior line is
    /// unreachable op-wise and falls back to a whole-`install` (losing that
    /// edit's identity anchors). Setting `continues: true` on line 0 is
    /// [`ApplyError::FirstLineContinues`] (nothing precedes it to continue).
    SetContinues { line: usize, continues: bool },
}

// ── Change-bundle wire (mark / line op ⇄ JSON) ──────────────────────────────
//
// [`Delta`] serializes through serde derive; [`MarkOp`] and [`LineOp`] carry
// [`MarkKind`] / [`LineKind`] / [`Container`], whose canonical JSON is the
// hand-written `serial` encoding (the `{type, …}` / `{kind, …}` discriminants a
// `ContentMark` / `ContentLine` already uses). These converters reuse that
// exact vocabulary so the `applyChange` bundle speaks the same shapes the
// content read surface does, rather than a second serde-derived dialect. The
// language bindings call them to lower a JS/Python bundle to core ops.

use crate::serial::{
    container_from_value, container_to_value, line_kind_from_value, line_kind_to_value,
    mark_from_value, mark_to_value, ParseError,
};
use serde_json::{Map, Value};

/// Encode a [`MarkOp`] to its wire object. `Add`/`Remove` carry the mark
/// vocabulary (`{op, start, end, type, …}`); `RemoveAnchor` is `{op, id}`.
pub fn mark_op_to_value(op: &MarkOp) -> Value {
    let mut m = Map::new();
    match op {
        MarkOp::Add { start, end, kind } => {
            m.insert("op".into(), "add".into());
            merge_mark(&mut m, *start, *end, kind);
        }
        MarkOp::Remove { start, end, kind } => {
            m.insert("op".into(), "remove".into());
            merge_mark(&mut m, *start, *end, kind);
        }
        MarkOp::RemoveAnchor { id } => {
            m.insert("op".into(), "removeAnchor".into());
            m.insert("id".into(), Value::String(id.clone()));
        }
    }
    Value::Object(m)
}

/// Merge a mark's `{start, end, type, …}` fields into an op object, reusing the
/// canonical `serial` mark encoding.
fn merge_mark(m: &mut Map<String, Value>, start: Usv, end: Usv, kind: &MarkKind) {
    let mark = Mark {
        start,
        end,
        kind: kind.clone(),
    };
    if let Value::Object(fields) = mark_to_value(&mark) {
        m.extend(fields);
    }
}

/// Decode a [`MarkOp`] from its wire object. Dispatches on `op`; `add`/`remove`
/// read the mark vocabulary through [`mark_from_value`].
pub fn mark_op_from_value(v: &Value) -> Result<MarkOp, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("mark op"))?;
    match o.get("op").and_then(Value::as_str) {
        Some("add") => {
            let mark = mark_from_value(v)?;
            Ok(MarkOp::Add {
                start: mark.start,
                end: mark.end,
                kind: mark.kind,
            })
        }
        Some("remove") => {
            let mark = mark_from_value(v)?;
            Ok(MarkOp::Remove {
                start: mark.start,
                end: mark.end,
                kind: mark.kind,
            })
        }
        Some("removeAnchor") => Ok(MarkOp::RemoveAnchor {
            id: o
                .get("id")
                .and_then(Value::as_str)
                .ok_or(ParseError::Shape("removeAnchor id"))?
                .to_string(),
        }),
        _ => Err(ParseError::Shape("mark op kind")),
    }
}

/// Encode a [`LineOp`] to its wire object. `SetKind` flattens the line-kind
/// discriminant (`kind`/`level`/`lang`) alongside `op`/`line`.
pub fn line_op_to_value(op: &LineOp) -> Value {
    let mut m = Map::new();
    match op {
        LineOp::Split { at } => {
            m.insert("op".into(), "split".into());
            m.insert("at".into(), Value::from(*at));
        }
        LineOp::Join { line } => {
            m.insert("op".into(), "join".into());
            m.insert("line".into(), Value::from(*line));
        }
        LineOp::SetKind { line, kind } => {
            m.insert("op".into(), "setKind".into());
            m.insert("line".into(), Value::from(*line));
            if let Value::Object(fields) = line_kind_to_value(kind) {
                m.extend(fields);
            }
        }
        LineOp::SetContainers { line, containers } => {
            m.insert("op".into(), "setContainers".into());
            m.insert("line".into(), Value::from(*line));
            m.insert(
                "containers".into(),
                Value::Array(containers.iter().map(container_to_value).collect()),
            );
        }
        LineOp::SetContinues { line, continues } => {
            m.insert("op".into(), "setContinues".into());
            m.insert("line".into(), Value::from(*line));
            m.insert("continues".into(), Value::Bool(*continues));
        }
    }
    Value::Object(m)
}

/// Decode a [`LineOp`] from its wire object. Dispatches on `op`.
pub fn line_op_from_value(v: &Value) -> Result<LineOp, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("line op"))?;
    let line = || {
        o.get("line")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .ok_or(ParseError::Shape("line op line"))
    };
    match o.get("op").and_then(Value::as_str) {
        Some("split") => Ok(LineOp::Split {
            at: o
                .get("at")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .ok_or(ParseError::Shape("split at"))?,
        }),
        Some("join") => Ok(LineOp::Join { line: line()? }),
        Some("setKind") => Ok(LineOp::SetKind {
            line: line()?,
            kind: line_kind_from_value(v)?,
        }),
        Some("setContainers") => Ok(LineOp::SetContainers {
            line: line()?,
            containers: o
                .get("containers")
                .and_then(Value::as_array)
                .ok_or(ParseError::Shape("setContainers containers"))?
                .iter()
                .map(container_from_value)
                .collect::<Result<_, _>>()?,
        }),
        Some("setContinues") => Ok(LineOp::SetContinues {
            line: line()?,
            continues: o
                .get("continues")
                .and_then(Value::as_bool)
                .ok_or(ParseError::Shape("setContinues continues"))?,
        }),
        _ => Err(ParseError::Shape("line op kind")),
    }
}

/// Lower a committed change **bundle** object (`{delta?, lineOps?, markOps?}`) to
/// core ops — the whole-bundle reader the `applyChange` verb needs, so each
/// binding lowers a JS/Python bundle in one call instead of re-deriving the
/// delta/op extraction. A missing `delta` is the identity (no text change); a
/// missing/`null` op array is empty. Both camelCase (`lineOps`) and snake_case
/// (`line_ops`) keys are accepted, so the one reader serves the wasm (camelCase)
/// and Python (either) surfaces. The error is a message string the binding wraps
/// in its own error type.
#[allow(clippy::type_complexity)]
pub fn change_bundle_from_value(
    v: &Value,
) -> Result<(Delta, Vec<LineOp>, Vec<MarkOp>), String> {
    let obj = v
        .as_object()
        .ok_or("bundle must be an object { delta?, lineOps?, markOps? }")?;
    let get = |snake: &str, camel: &str| obj.get(snake).or_else(|| obj.get(camel));
    let delta = match get("delta", "delta") {
        Some(Value::Null) | None => Delta { ops: Vec::new() },
        Some(d) => serde_json::from_value(d.clone()).map_err(|e| format!("invalid delta: {e}"))?,
    };
    let line_ops = op_array(get("line_ops", "lineOps"), line_op_from_value, "lineOps")?;
    let mark_ops = op_array(get("mark_ops", "markOps"), mark_op_from_value, "markOps")?;
    Ok((delta, line_ops, mark_ops))
}

/// Lower an optional JSON array of op objects through `convert` (missing/`null`
/// → empty), naming `what` in any shape-error message. The list twin shared by
/// [`change_bundle_from_value`]'s line- and mark-op channels.
fn op_array<T>(
    value: Option<&Value>,
    convert: impl Fn(&Value) -> Result<T, ParseError>,
    what: &str,
) -> Result<Vec<T>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let arr = value
        .as_array()
        .ok_or_else(|| format!("{what} must be an array"))?;
    arr.iter()
        .map(|v| convert(v).map_err(|e| format!("invalid {what}: {e}")))
        .collect()
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
    /// A [`LineOp::SetContinues`] tried to set `continues: true` on line 0, which
    /// has nothing before it to continue — the apply-time twin of the
    /// [`Invariant::FirstLineContinues`](crate::model::Invariant::FirstLineContinues)
    /// validation error, refused here because `normalize` does not repair it.
    FirstLineContinues,
    /// The text delta's expected base length disagreed with the content —
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

impl Content {
    /// Splice `text` via `delta`, rebase marks, sync `lines` to `\n` changes,
    /// cascade island removal for any deleted slot, then normalize.
    ///
    /// Islands stay in lockstep with their [`ISLAND_SLOT`] chars: a delta that
    /// *deletes* a slot drops the corresponding [`Island`] (the content goes
    /// away with its slot); a delta that *inserts* a raw slot is rejected
    /// ([`ApplyError::IslandSlotInInsert`]) — islands are created through their
    /// own channel, never a text splice, so a slot arriving here would orphan.
    ///
    /// Inserted text is sanitized first: `\r` and Unicode bidi controls — the
    /// chars [`Content::validate`] forbids — are stripped, mirroring the
    /// normalization `import` applies at the string boundary. The text-delta
    /// channel is the *other* way text enters the content, so without this an
    /// insert of `\r` or a bidi control returned `Ok` while leaving a content
    /// that fails `validate()` (see issue #899).
    pub fn apply_text_delta(&mut self, delta: &Delta) -> Result<(), ApplyError> {
        self.apply_text_delta_inner(delta)?;
        self.normalize();
        Ok(())
    }

    /// [`apply_text_delta`](Self::apply_text_delta) without the terminal
    /// normalize — the stage [`apply_field_change`](Self::apply_field_change)
    /// runs so a committed bundle canonicalizes once at the end, not after each
    /// op.
    fn apply_text_delta_inner(&mut self, delta: &Delta) -> Result<(), ApplyError> {
        // Reject before mutating: a raw slot in an insert would create a slot
        // with no backing island. Checked up front so the content is untouched
        // on this error.
        for op in &delta.ops {
            if let Op::Insert(s) = op {
                if s.contains(ISLAND_SLOT) {
                    return Err(ApplyError::IslandSlotInInsert);
                }
            }
        }

        // Strip the chars `validate()` forbids (`\r`, bidi controls) from every
        // insert before they reach the content. Stripping — not rejecting —
        // mirrors `import`: these are content to normalize away, unlike a raw
        // slot, which has no backing island and must be refused. Sanitizing the
        // whole delta up front keeps `try_apply` / `map_pos` / line+island sync
        // in agreement on one cleaned op stream; a clean delta (every keystroke)
        // is borrowed through untouched, so the hot path skips the clone.
        let sanitized = sanitize_inserts(delta);
        let delta = sanitized.as_ref();

        let old_chars: Vec<char> = self.text.chars().collect();
        let old_lines = self.lines.clone();
        // A splice may name only the region it changes: `try_apply` retains the
        // untouched remainder implicitly, so a bare prepend applies against the
        // whole content. An over-long delta (consuming more base than exists)
        // still fails the base-length check.
        let new_text = delta
            .try_apply(&self.text)
            .map_err(|e| ApplyError::DeltaBaseMismatch {
                expected: e.expected,
                actual: e.actual,
            })?;

        self.rebase_marks(delta);
        let new_len = new_text.chars().count();
        self.marks.retain(|m| {
            m.start <= m.end
                && m.end <= new_len
                && (m.start < m.end || !m.kind.is_formatting())
        });

        self.text = new_text;
        self.lines = sync_lines_for_delta(&old_chars, old_lines, delta);
        let old_islands = std::mem::take(&mut self.islands);
        self.islands = sync_islands_for_delta(&old_chars, old_islands, delta);
        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        Ok(())
    }

    /// Rebase every mark's range through `delta`'s
    /// [`map_pos`](crate::delta::Delta::map_pos): a range mark's start biases
    /// `After` and its end `Before` (an insertion at either edge grows text
    /// *outside* the span), a point (zero-width) mark biases `Before`. The one
    /// mapping the text-delta channel and line split/join both rebase marks by.
    fn rebase_marks(&mut self, delta: &Delta) {
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
    }

    /// Apply mark ops in final-text coordinates, then normalize.
    pub fn apply_mark_ops(&mut self, ops: &[MarkOp]) -> Result<(), ApplyError> {
        self.apply_mark_ops_inner(ops)?;
        self.normalize();
        Ok(())
    }

    /// [`apply_mark_ops`](Self::apply_mark_ops) without the terminal normalize —
    /// the bundle's final stage, canonicalized once by
    /// [`apply_field_change`](Self::apply_field_change).
    fn apply_mark_ops_inner(&mut self, ops: &[MarkOp]) -> Result<(), ApplyError> {
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
                    let mut next = Vec::with_capacity(self.marks.len());
                    for m in self.marks.drain(..) {
                        // Untouched: a different kind, or no overlap with the
                        // removed range.
                        if m.kind != *kind || !ranges_overlap(m.start, m.end, *start, *end) {
                            next.push(m);
                            continue;
                        }
                        // Identity/unknown handles have no range algebra to
                        // subtract — drop the overlapping one whole.
                        if !kind.is_formatting() {
                            continue;
                        }
                        // Formatting: subtract [start, end), re-emitting the
                        // surviving fragments. An edge-aligned removal yields a
                        // zero-width fragment here; `normalize` drops it.
                        if m.start < *start {
                            next.push(Mark {
                                start: m.start,
                                end: *start,
                                kind: m.kind.clone(),
                            });
                        }
                        if *end < m.end {
                            next.push(Mark {
                                start: *end,
                                end: m.end,
                                kind: m.kind.clone(),
                            });
                        }
                    }
                    self.marks = next;
                }
                MarkOp::RemoveAnchor { id } => {
                    self.marks
                        .retain(|m| !matches!(&m.kind, MarkKind::Anchor { id: aid } if aid == id));
                }
            }
        }
        Ok(())
    }

    /// Apply line ops — split/join splice `\n`; set ops touch metadata only.
    pub fn apply_line_ops(&mut self, ops: &[LineOp]) -> Result<(), ApplyError> {
        self.apply_line_ops_inner(ops)?;
        self.normalize();
        Ok(())
    }

    /// [`apply_line_ops`](Self::apply_line_ops) without the terminal normalize —
    /// a bundle stage canonicalized once by
    /// [`apply_field_change`](Self::apply_field_change).
    fn apply_line_ops_inner(&mut self, ops: &[LineOp]) -> Result<(), ApplyError> {
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
                LineOp::SetContinues { line, continues } => {
                    // Line 0 has nothing before it to continue: setting the flag
                    // there would forge the `FirstLineContinues` invariant that
                    // `normalize` does not repair. Reject before the write so the
                    // content stays valid (`apply_field_change` stages line ops on
                    // a scratch copy, so this leaves `self` untouched).
                    if *line == 0 && *continues {
                        return Err(ApplyError::FirstLineContinues);
                    }
                    let l = self.line_mut(*line)?;
                    l.continues = *continues;
                }
            }
        }
        Ok(())
    }

    /// One committed field edit bundle: text delta, then line ops, then marks,
    /// canonicalized by a single terminal [`normalize`](Self::normalize).
    ///
    /// All-or-nothing: on any op's error `self` is left exactly as it was, so a
    /// caller need not snapshot-and-restore around a failed bundle. A bundle
    /// carrying line or mark ops has several fallible stages that would
    /// otherwise partially commit, so it is staged on a scratch copy and
    /// swapped in only once every stage succeeds. The pure-text-delta path (the
    /// per-keystroke hot path) skips the clone: `apply_text_delta` validates the
    /// delta before mutating, so it is already atomic on the errors a caller can
    /// provoke.
    ///
    /// The stages run on their non-normalizing inner forms and `normalize` runs
    /// once at the end. One terminal normalize suffices because split/join
    /// rebase marks through their `\n` splice
    /// ([`map_pos`](crate::delta::Delta::map_pos) semantics): the
    /// formatting-edge `\n`-trim then commutes with the line ops (trim-per-stage
    /// and trim-once converge), and `MarkOp::Remove` is coverage-set
    /// subtraction, which commutes with `normalize`'s same-kind union
    /// (`(A ∪ B) \ R = (A\R) ∪ (B\R)`). One canonicalization point, one pass.
    pub fn apply_field_change(
        &mut self,
        text_delta: &Delta,
        line_ops: &[LineOp],
        mark_ops: &[MarkOp],
    ) -> Result<(), ApplyError> {
        if line_ops.is_empty() && mark_ops.is_empty() {
            return self.apply_text_delta(text_delta);
        }
        let mut scratch = self.clone();
        scratch.apply_text_delta_inner(text_delta)?;
        scratch.apply_line_ops_inner(line_ops)?;
        scratch.apply_mark_ops_inner(mark_ops)?;
        scratch.normalize();
        *self = scratch;
        Ok(())
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

        // Rebase marks through the one-char `\n` insertion — the same map_pos
        // rule the text-delta channel uses, so a split does not drift a mark's
        // coordinates (a mark spanning `at` grows by the inserted char; the
        // terminal normalize trims any `\n` edge it lands on).
        self.rebase_marks(&Delta {
            ops: vec![Op::Retain(at), Op::Insert("\n".to_string())],
        });

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

        // Rebase marks through the one-char `\n` deletion, as the text-delta
        // channel would: a mark spanning the boundary shrinks by one; one that
        // covered only the `\n` collapses to zero-width and the terminal
        // normalize drops it.
        self.rebase_marks(&Delta {
            ops: vec![Op::Retain(nl), Op::Delete(1)],
        });

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

fn ranges_overlap(a0: Usv, a1: Usv, b0: Usv, b1: Usv) -> bool {
    a0 < b1 && b0 < a1
}

/// A char the content text may not carry (`validate()` rejects it): a bare `\r`
/// or a Unicode bidi formatting control. `\n` is a real line boundary and a raw
/// [`ISLAND_SLOT`] is refused separately, so neither belongs here.
fn insert_forbidden(c: char) -> bool {
    c == '\r' || is_bidi_char(c)
}

/// Drop [`insert_forbidden`] chars from every `Op::Insert`, returning the delta
/// borrowed untouched when no insert carries one (the common keystroke). Mirrors
/// the forbidden-char stripping `import` applies (`push_text`, `strip_bidi_
/// formatting`); a raw `\r`/bidi arriving through the text-delta channel would
/// otherwise persist a content that fails `validate()`.
fn sanitize_inserts(delta: &Delta) -> Cow<'_, Delta> {
    let needs_cleaning = delta
        .ops
        .iter()
        .any(|op| matches!(op, Op::Insert(s) if s.chars().any(insert_forbidden)));
    if !needs_cleaning {
        return Cow::Borrowed(delta);
    }
    let ops = delta
        .ops
        .iter()
        .map(|op| match op {
            Op::Insert(s) => Op::Insert(s.chars().filter(|c| !insert_forbidden(*c)).collect()),
            other => other.clone(),
        })
        .collect();
    Cow::Owned(Delta { ops })
}

/// Walk `delta` over `old_chars` and mirror `\n` insert/delete in `lines`,
/// building the result in one forward pass — O(old_chars walked + inserts),
/// no per-`\n` mid-`Vec` `remove`/`insert`.
///
/// The cursor sits *in* a line, `cur`; downstream of it is always the untouched
/// original suffix (`rest`), because a split lands its clone right at the cursor
/// and a delete drops the next original. So the three `\n` events reduce to:
/// a retained `\n` finalizes `cur` and pulls the next original into it; a
/// deleted `\n` drops the next original (merging it in), when one exists; an
/// inserted `\n` finalizes `cur` and makes a clone (its `continues` cleared) the
/// new `cur`. `cur == None` is the past-the-end state on a malformed corpus
/// (more `\n` than lines), where a split clones a default line.
fn sync_lines_for_delta(old_chars: &[char], old_lines: Vec<Line>, delta: &Delta) -> Vec<Line> {
    let cap = old_lines.len();
    let mut rest = old_lines.into_iter();
    let mut out: Vec<Line> = Vec::with_capacity(cap);
    let mut cur: Option<Line> = rest.next();
    let mut old = 0usize;

    for op in &delta.ops {
        match op {
            Op::Retain(n) => {
                for _ in 0..*n {
                    if old >= old_chars.len() {
                        break;
                    }
                    if old_chars[old] == '\n' {
                        out.extend(cur.take());
                        cur = rest.next();
                    }
                    old += 1;
                }
            }
            Op::Delete(n) => {
                for _ in 0..*n {
                    if old >= old_chars.len() {
                        break;
                    }
                    // A deleted '\n' merges the next original into `cur` — drop
                    // it. With no next original there is nothing to drop.
                    if old_chars[old] == '\n' {
                        rest.next();
                    }
                    old += 1;
                }
            }
            Op::Insert(s) => {
                for c in s.chars() {
                    if c == '\n' {
                        let mut new_line = match cur.take() {
                            Some(line) => {
                                let clone = line.clone();
                                out.push(line);
                                clone
                            }
                            None => default_para_line(),
                        };
                        new_line.continues = false;
                        cur = Some(new_line);
                    }
                }
            }
        }
    }

    out.extend(cur);
    out.extend(rest);
    out
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
    fn mark_op_wire_round_trips_each_variant() {
        let ops = vec![
            MarkOp::Add {
                start: 0,
                end: 3,
                kind: MarkKind::Strong,
            },
            MarkOp::Add {
                start: 1,
                end: 2,
                kind: MarkKind::Link {
                    url: "https://x".into(),
                },
            },
            MarkOp::Remove {
                start: 4,
                end: 6,
                kind: MarkKind::Anchor { id: "c1".into() },
            },
            MarkOp::RemoveAnchor { id: "c2".into() },
        ];
        for op in ops {
            let v = mark_op_to_value(&op);
            assert_eq!(mark_op_from_value(&v).unwrap(), op, "round-trip: {v}");
        }
    }

    #[test]
    fn line_op_wire_round_trips_each_variant() {
        let ops = vec![
            LineOp::Split { at: 5 },
            LineOp::Join { line: 1 },
            LineOp::SetKind {
                line: 0,
                kind: LineKind::Heading { level: 2 },
            },
            LineOp::SetContainers {
                line: 2,
                containers: vec![Container::Quote],
            },
            LineOp::SetContinues {
                line: 1,
                continues: true,
            },
            LineOp::SetContinues {
                line: 3,
                continues: false,
            },
        ];
        for op in ops {
            let v = line_op_to_value(&op);
            assert_eq!(line_op_from_value(&v).unwrap(), op, "round-trip: {v}");
        }
    }

    #[test]
    fn delta_serde_shape() {
        let d = Delta {
            ops: vec![Op::Retain(2), Op::Insert("hi".into()), Op::Delete(1)],
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"ops": [{"retain": 2}, {"insert": "hi"}, {"delete": 1}]})
        );
        assert_eq!(serde_json::from_value::<Delta>(v).unwrap(), d);
    }

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
    fn apply_text_delta_pads_short_prepend() {
        // A bare prepend names only its inserted text (no trailing retain); it
        // still splices against the whole content rather than failing the base
        // check (regression for the per-field delta path).
        let mut rt = from_markdown("hello").unwrap();
        rt.apply_text_delta(&Delta {
            ops: vec![Op::Insert("NEW ".into())],
        })
        .unwrap();
        assert_eq!(rt.text, "NEW hello");
    }

    #[test]
    fn apply_text_delta_rejects_over_long_delta() {
        // Consuming more base than exists is a wrong-revision delta, not an
        // abbreviated one — it still fails closed.
        let mut rt = from_markdown("hi").unwrap();
        assert!(matches!(
            rt.apply_text_delta(&Delta {
                ops: vec![Op::Retain(99)],
            }),
            Err(ApplyError::DeltaBaseMismatch { .. })
        ));
        assert_eq!(rt.text, "hi");
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
    fn apply_mark_ops_remove_punches_hole() {
        // Un-formatting the middle of a run leaves the two non-overlapping
        // fragments, not an empty mark set (issue #901). Strong[0,6) over
        // "abcdef", Remove[2,4) -> Strong[0,2) + Strong[4,6).
        let mut rt = from_markdown("abcdef").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 6,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 2,
            end: 4,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        assert_eq!(strong, vec![(0, 2), (4, 6)]);
    }

    #[test]
    fn apply_mark_ops_remove_at_edge_leaves_no_zero_width() {
        // A removal flush against the mark's start yields a zero-width left
        // fragment [0,0); normalize drops it, leaving only the right fragment.
        let mut rt = from_markdown("abcdef").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 6,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 0,
            end: 2,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        assert_eq!(strong, vec![(2, 6)]);
    }

    #[test]
    fn apply_mark_ops_remove_covering_range_drops_mark() {
        // A removal that fully covers the mark leaves nothing (both fragments
        // zero-width or inverted) — the whole-drop case still holds.
        let mut rt = from_markdown("abcdef").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 2,
            end: 4,
            kind: MarkKind::Emph,
        }])
        .unwrap();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 0,
            end: 6,
            kind: MarkKind::Emph,
        }])
        .unwrap();
        assert!(!rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Emph)));
    }

    #[test]
    fn apply_mark_ops_remove_non_formatting_drops_whole() {
        // Identity/unknown handles can't be range-fragmented: an overlapping
        // one is dropped whole, never split into fragments.
        let mut rt = from_markdown("abcdef").unwrap();
        rt.marks.push(Mark {
            start: 0,
            end: 6,
            kind: MarkKind::Unknown {
                tag: "x".into(),
                attrs: serde_json::json!({}),
            },
        });
        rt.normalize();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 2,
            end: 4,
            kind: MarkKind::Unknown {
                tag: "x".into(),
                attrs: serde_json::json!({}),
            },
        }])
        .unwrap();
        assert!(!rt
            .marks
            .iter()
            .any(|m| matches!(m.kind, MarkKind::Unknown { .. })));
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

    #[test]
    fn line_op_set_continues_sets_and_clears() {
        // Two paragraph lines (delta-split → both `continues: false`, i.e. two
        // blocks). `setContinues` on line 1 turns the boundary into a within-block
        // hard break, and export then emits one block, not two paragraphs.
        let mut rt = from_markdown("one two").unwrap();
        rt.apply_text_delta(&diff("one two", "one\ntwo")).unwrap();
        assert!(!rt.lines[1].continues, "delta-split newline is a new block");

        rt.apply_line_ops(&[LineOp::SetContinues {
            line: 1,
            continues: true,
        }])
        .unwrap();
        assert!(rt.lines[1].continues);
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(
            crate::export::to_markdown(&rt).matches("\n\n").count(),
            0,
            "a within-block hard break is not a paragraph boundary"
        );

        // Clearing restores the block boundary.
        rt.apply_line_ops(&[LineOp::SetContinues {
            line: 1,
            continues: false,
        }])
        .unwrap();
        assert!(!rt.lines[1].continues);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn line_op_set_continues_rejects_first_line() {
        let mut rt = from_markdown("one two").unwrap();
        rt.apply_text_delta(&diff("one two", "one\ntwo")).unwrap();
        let before = rt.clone();
        // `continues: true` on line 0 forges `FirstLineContinues`; refused, and
        // the content is left untouched.
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetContinues {
                line: 0,
                continues: true,
            }]),
            Err(ApplyError::FirstLineContinues)
        );
        assert_eq!(rt, before, "rejected op leaves the content untouched");
        // Clearing line 0 (already `false`) is a no-op, not an error.
        rt.apply_line_ops(&[LineOp::SetContinues {
            line: 0,
            continues: false,
        }])
        .unwrap();
        assert_eq!(rt.validate(), Ok(()));
    }

    fn island(id: &str) -> Island {
        Island {
            id: id.into(),
            island_type: "image".into(),
            props: serde_json::json!({}),
            loss: crate::model::Loss::Lossless,
        }
    }

    /// A single-line content `a￼b` (one inline island slot, one backing island).
    fn corpus_with_island() -> Content {
        let mut rt = Content::empty();
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
        let mut rt = Content::empty();
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
        // Content untouched on the rejected insert (checked before any mutation).
        assert_eq!(rt.text, "ab");
        assert!(rt.islands.is_empty());
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_carriage_return_is_stripped() {
        // A `\r` in an insert is dropped, not persisted — the content stays
        // valid instead of the op returning Ok over a `CarriageReturn`
        // violation (issue #899). `\r\n` still yields the line-boundary `\n`.
        let mut rt = from_markdown("ab").unwrap();
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Insert("\r".into()), Op::Retain(1)],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, "ab");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_bidi_control_is_stripped() {
        // A bidi override (U+202E) in an insert is dropped — the content stays
        // valid and import's Trojan-source defense is not bypassed (issue #899).
        let mut rt = from_markdown("ab").unwrap();
        let d = Delta {
            ops: vec![
                Op::Retain(1),
                Op::Insert("\u{202E}".into()),
                Op::Retain(1),
            ],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, "ab");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_crlf_keeps_the_newline_and_splits() {
        // Stripping only the `\r` of a `\r\n` leaves a real line boundary: the
        // insert still splits the line, and slot/line sync stays intact.
        let mut rt = from_markdown("ab").unwrap();
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Insert("\r\n".into()), Op::Retain(1)],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_of_clean_text_is_not_reallocated() {
        // The hot path: a delta whose inserts carry no forbidden char borrows
        // through `sanitize_inserts` unchanged.
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Insert("clean\n".into()), Op::Retain(1)],
        };
        assert!(matches!(sanitize_inserts(&d), Cow::Borrowed(_)));
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

    #[test]
    fn apply_field_change_is_all_or_nothing() {
        // A bundle whose text delta and first mark op succeed but whose second
        // mark op is out of range must leave the content exactly as it was — the
        // successful earlier stages do not partially commit.
        let mut rt = from_markdown("abc").unwrap();
        let before = rt.clone();
        let d = diff("abc", "abXc");
        let err = rt.apply_field_change(
            &d,
            &[],
            &[
                MarkOp::Add {
                    start: 0,
                    end: 2,
                    kind: MarkKind::Strong,
                },
                MarkOp::Add {
                    start: 99,
                    end: 100,
                    kind: MarkKind::Emph,
                },
            ],
        );
        assert!(matches!(err, Err(ApplyError::MarkOutOfRange { .. })));
        assert_eq!(rt, before, "failed bundle must not mutate the content");
    }

    // ── sync_lines_for_delta characterization (issue #926 finding 2) ─────────
    //
    // Pin the observable behavior of the line-sync walk — retain/insert/delete
    // interleavings, the split template-clone rule, and the malformed-corpus
    // guards — against a silent change to its internals.

    /// A `Heading{level}` line, its level a visible tag so a test can trace
    /// which original line landed where; `continues` distinguishes a clone.
    fn tag_line(level: u8, continues: bool) -> Line {
        Line {
            kind: LineKind::Heading { level },
            containers: Vec::new(),
            continues,
        }
    }

    /// `(tag, continues)` per line — `Heading{level}` reads its level, `Para` is
    /// tag 0 (the default line), any other kind is 255.
    fn tags(lines: &[Line]) -> Vec<(u8, bool)> {
        lines
            .iter()
            .map(|l| match l.kind {
                LineKind::Heading { level } => (level, l.continues),
                LineKind::Para => (0, l.continues),
                _ => (255, l.continues),
            })
            .collect()
    }

    #[test]
    fn sync_lines_retain_only_is_identity() {
        let old_chars: Vec<char> = "a\nb\nc".chars().collect();
        let lines = vec![tag_line(1, false), tag_line(2, false), tag_line(3, false)];
        let d = Delta {
            ops: vec![Op::Retain(5)],
        };
        assert_eq!(sync_lines_for_delta(&old_chars, lines.clone(), &d), lines);
    }

    #[test]
    fn sync_lines_insert_newline_clones_split_line_and_clears_continues() {
        // Split line 1 ("bc") mid-line: the first half stays the original line
        // (keeps kind, containers, and its `continues: true`); the second half
        // is a clone of it with `continues` forced false.
        let old_chars: Vec<char> = "a\nbc".chars().collect();
        let l1 = Line {
            kind: LineKind::Heading { level: 5 },
            containers: vec![Container::Quote],
            continues: true,
        };
        let lines = vec![tag_line(1, false), l1.clone()];
        // Retain(3)[a\nb] moves to line 1; Insert("\n") splits it; Retain(1)[c].
        let d = Delta {
            ops: vec![Op::Retain(3), Op::Insert("\n".into()), Op::Retain(1)],
        };
        let out = sync_lines_for_delta(&old_chars, lines, &d);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1], l1, "first half is the untouched original line");
        assert_eq!(out[2].kind, LineKind::Heading { level: 5 });
        assert_eq!(out[2].containers, vec![Container::Quote]);
        assert!(!out[2].continues, "the split clone starts a new block");
    }

    #[test]
    fn sync_lines_delete_newline_drops_following_line() {
        // Delete the first '\n' of "a\nb\nc": lines 0 and 1 merge, dropping line
        // 1; the current line (0) and line 2 survive.
        let old_chars: Vec<char> = "a\nb\nc".chars().collect();
        let lines = vec![tag_line(1, false), tag_line(2, false), tag_line(3, false)];
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Delete(1), Op::Retain(3)],
        };
        let out = sync_lines_for_delta(&old_chars, lines, &d);
        assert_eq!(tags(&out), vec![(1, false), (3, false)]);
    }

    #[test]
    fn sync_lines_delete_trailing_newline_without_following_line_is_guarded() {
        // Malformed corpus: text "a\n" is two segments but `lines` has one
        // entry. Deleting the '\n' when `line_idx + 1` is out of bounds removes
        // nothing (the guard), leaving the single line intact.
        let old_chars: Vec<char> = "a\n".chars().collect();
        let lines = vec![tag_line(1, false)];
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Delete(1)],
        };
        let out = sync_lines_for_delta(&old_chars, lines, &d);
        assert_eq!(tags(&out), vec![(1, false)]);
    }

    #[test]
    fn sync_lines_stops_at_end_of_old_chars() {
        // A retain running past the end of old_chars stops at the end rather
        // than indexing out of bounds (the `old >= old_chars.len()` guard).
        let old_chars: Vec<char> = "a\nb".chars().collect();
        let lines = vec![tag_line(1, false), tag_line(2, false)];
        let d = Delta {
            ops: vec![Op::Retain(99)],
        };
        assert_eq!(sync_lines_for_delta(&old_chars, lines.clone(), &d), lines);
    }

    #[test]
    fn sync_lines_insert_two_newlines_adds_two_clones() {
        // Inserting "\n\n" mid-line adds two lines, each carrying the split
        // line's kind and containers with `continues: false`.
        let old_chars: Vec<char> = "abc".chars().collect();
        let src = Line {
            kind: LineKind::Heading { level: 7 },
            containers: vec![Container::Quote],
            continues: false,
        };
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Insert("\n\n".into()), Op::Retain(2)],
        };
        let out = sync_lines_for_delta(&old_chars, vec![src], &d);
        assert_eq!(out.len(), 3);
        for l in &out {
            assert_eq!(l.kind, LineKind::Heading { level: 7 });
            assert_eq!(l.containers, vec![Container::Quote]);
            assert!(!l.continues);
        }
    }

    // ── line-op mark remap + terminal-normalize collapse (issue #926 finding 3) ──

    #[test]
    fn split_line_rebases_mark_across_the_split_point() {
        // A strong mark spanning the split point grows by the inserted `\n`
        // rather than staying at its old coordinates. "abcd", strong[1..3)
        // ("bc"); split at 2 → "ab\ncd"; the mark must still cover "b"+"c",
        // i.e. [1..4) over "ab\ncd".
        let mut rt = from_markdown("abcd").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 1,
            end: 3,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        rt.apply_line_ops(&[LineOp::Split { at: 2 }]).unwrap();
        assert_eq!(rt.text, "ab\ncd");
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        // [1..4) spans "b\nc"; normalize keeps the interior `\n` (a mark may
        // legitimately span lines), trimming only leading/trailing boundaries.
        assert_eq!(strong, vec![(1, 4)]);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn join_line_rebases_marks_to_final_text_coordinates() {
        // The issue's concrete drift case: "ab\ncd", strong[2..4) (over "\nc").
        // Joining line 0 removes the `\n`; the remap + terminal normalize must
        // land strong on "c" — coordinate [2..3) over "abcd" — not on "d"
        // (the un-remapped-mark bug) nor on "cd".
        let mut rt = from_markdown("ab").unwrap();
        rt.apply_text_delta(&diff("ab", "ab\ncd")).unwrap();
        rt.marks.push(Mark {
            start: 2,
            end: 4,
            kind: MarkKind::Strong,
        });
        rt.normalize();
        // Post-normalize the `\n` edge trims to [3..4) ("c"); either way the
        // join must converge to strong on "c".
        rt.apply_line_ops(&[LineOp::Join { line: 0 }]).unwrap();
        assert_eq!(rt.text, "abcd");
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        assert_eq!(strong, vec![(2, 3)], "strong lands on 'c', not 'd' or 'cd'");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn field_change_terminal_normalize_matches_per_stage_normalize() {
        // The collapse proof obligation: a bundle applied through
        // `apply_field_change` (one terminal normalize) must equal applying the
        // same stages each with its own normalize (the public wrappers). The
        // remap through split/join is what makes the two converge.
        let start = from_markdown("hello world").unwrap();
        let text_delta = diff("hello world", "hello brave world");
        let line_ops = vec![LineOp::Split { at: 5 }]; // after "hello"
        let mark_ops = vec![MarkOp::Add {
            start: 0,
            end: 5,
            kind: MarkKind::Strong,
        }];

        let mut bundled = start.clone();
        bundled
            .apply_field_change(&text_delta, &line_ops, &mark_ops)
            .unwrap();

        let mut staged = start;
        staged.apply_text_delta(&text_delta).unwrap();
        staged.apply_line_ops(&line_ops).unwrap();
        staged.apply_mark_ops(&mark_ops).unwrap();

        assert_eq!(bundled, staged, "terminal normalize diverged from per-stage");
        assert_eq!(bundled.validate(), Ok(()));
    }

    #[test]
    fn sync_lines_select_all_delete_collapses_to_first_line() {
        // The motivating case (issue #926 finding 2): deleting a whole
        // multi-line body drops every line but the first (each deleted '\n'
        // merges the next line away).
        let text: String = (0..50).map(|i| format!("line{i}\n")).collect();
        let old_chars: Vec<char> = text.chars().collect();
        let lines: Vec<Line> = (0..=50).map(|i| tag_line((i % 200) as u8, false)).collect();
        assert_eq!(lines.len(), old_chars.iter().filter(|&&c| c == '\n').count() + 1);
        let d = Delta {
            ops: vec![Op::Delete(old_chars.len())],
        };
        let out = sync_lines_for_delta(&old_chars, lines, &d);
        assert_eq!(tags(&out), vec![(0, false)], "only the first line survives");
    }

    #[test]
    fn sync_lines_insert_newline_past_end_appends_default() {
        // Malformed corpus: after a retain walks past the sole line (line_idx ==
        // lines.len()), an inserted '\n' has no line to clone and appends a
        // default Para.
        let old_chars: Vec<char> = "a\n".chars().collect();
        let lines = vec![tag_line(1, false)];
        // Retain(2)[a\n] moves line_idx to 1 (== lines.len()); Insert("\n").
        let d = Delta {
            ops: vec![Op::Retain(2), Op::Insert("\n".into())],
        };
        let out = sync_lines_for_delta(&old_chars, lines, &d);
        assert_eq!(out.len(), 2);
        assert_eq!(tags(&out)[0], (1, false));
        assert_eq!(out[1].kind, LineKind::Para);
        assert!(out[1].containers.is_empty());
        assert!(!out[1].continues);
    }
}
