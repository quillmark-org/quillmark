# crates/richtext

## Needs judgment

### ops.rs:352 — line removal is quadratic under large deletions

`sync_lines_for_delta` calls `lines.remove(line_idx + 1)` once per deleted
`\n` — O(n) each, quadratic for a select-all-delete on a large body. A
single-pass cursor rewrite is possible but the interleaving of retain/insert
with the template-clone rule has edge cases on malformed corpora; semantics
need pinning by tests before restructuring.

### ops.rs:221 — `apply_field_change` normalizes three times (behavior-load-bearing)

`apply_text_delta`, `apply_line_ops`, and `apply_mark_ops` each end with
`self.normalize()`, so a committed edit bundle pays 3× (mark sort + full-text
char collection + island props rebuild). The obvious fix — non-normalizing
inner steps and one `normalize()` at the bundle end — is **not**
behavior-preserving, so it is deferred until the semantics are pinned:

- `normalize` unions adjacent same-kind marks (`[0..3]`+`[3..5]`→`[0..5]`), and
  `apply_mark_ops`'s `Remove` matches by `ranges_overlap` (half-open). With the
  intermediate normalize, `Remove{3..5}` overlaps the whole union and clears
  `[0..3]` too; without it, only `[3..5]` is removed. The normalize between text
  and mark ops changes which marks a subsequent `Remove` matches.
- `split_line`/`join_line` insert/delete `\n` in the text **without** remapping
  marks, and `normalize`'s formatting-edge trim is `\n`-position-sensitive, so
  normalizing on the pre-line-op text vs the post-line-op text can trim
  different edges.

Fix: give `apply_field_change` a real op-level model (remap marks across line
ops; define `Remove`-vs-union order) and pin it with tests, then collapse to one
terminal normalize.

### ops.rs:111, delta.rs:99 — short-delta leniency lives at the wrong altitude

`extend_to_base` pads abbreviated deltas inside `apply_text_delta` to
accommodate abbreviated producers, leaving `Delta` with three application
semantics: `apply` clamps, `try_apply` is strict, `apply_text_delta`
pads-then-checks — while `map_pos` separately implements implicit trailing
retain. A consumer that computes a delta and later replays it via strict
`try_apply` (an editor bridge, sync) gets `BaseLengthMismatch` for an
abbreviated edit that `apply_text_delta` accepted. Fix: normalize abbreviated
deltas at the boundary where they enter, or make implicit trailing retain the
documented contract of `try_apply` itself. (The WASM `applyFieldDelta` producer
this originally accommodated was removed in #886; the padding remains for the
corpus writers.)
