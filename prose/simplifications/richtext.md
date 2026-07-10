# crates/richtext

## Needs judgment

### ops.rs:352 — line removal is quadratic under large deletions

`sync_lines_for_delta` calls `lines.remove(line_idx + 1)` once per deleted
`\n` — O(n) each, quadratic for a select-all-delete on a large body. A
single-pass cursor rewrite is possible but the interleaving of retain/insert
with the template-clone rule has edge cases on malformed corpora; semantics
need pinning by tests before restructuring.

### ops.rs:330, ops.rs:391 — `sync_lines_for_delta` / `sync_islands_for_delta` are twins

The same Retain/Delete char-walk over `old_chars`, differing only in sentinel
(`\n` vs `ISLAND_SLOT`) and action. A fix to the walk must land twice. Fix: one
shared walk parameterized by sentinel handler — mechanical but restructures the
delta-apply path.

### ops.rs:221 — `apply_field_change` normalizes three times

`apply_text_delta`, `apply_line_ops`, and `apply_mark_ops` each end with
`self.normalize()`, so a committed edit bundle pays 3× (mark sort + full-text
char collection + island props rebuild). Fix: internal non-normalizing apply
steps with one `normalize()` at the bundle end. Needs care: the public
single-channel methods must keep normalizing.

### model.rs:333, serial.rs:388 — every `normalize()` rebuilds all table-cell JSON

`normalize_table_cell_marks` parses and rebuilds each cell (`parse_cell` +
`cell_to_value`) and `sorted_value`-clones every island's `props` tree even
when a pure text splice touched no island — O(total island props) per
keystroke on any corpus containing a table. Fix: skip the island pass unless
islands were touched (dirty flag on island-mutating paths), or verify-sorted
before rebuilding.

### model.rs:334, model.rs:448 — `island_type == "table"` string dispatch in three places

`normalize` and `validate` each string-match `"table"` to reach into props via
`serial`, and export's `emit_island` dispatches on the same tag independently.
Islands are an open set: a new mark-carrying island type requires coordinated
edits in three files, and missing one silently voids the canonical-bytes
guarantee for that type. Fix: one island-type dispatch table in `serial.rs`
exposing per-type normalize/validate/emit hooks.

### ops.rs:111, delta.rs:99 — short-delta leniency lives at the wrong altitude

`extend_to_base` pads abbreviated deltas inside `apply_text_delta` to
accommodate one producer (the WASM `applyFieldDelta` path), leaving `Delta`
with three application semantics: `apply` clamps, `try_apply` is strict,
`apply_text_delta` pads-then-checks — while `map_pos` separately implements
implicit trailing retain. The change log records the *unpadded* delta, so a
future consumer replaying entries via strict `try_apply` (undo, sync) gets
`BaseLengthMismatch` for edits that succeeded. Fix: normalize abbreviated
deltas at the boundary where they enter (the WASM delta deserializer), or make
implicit trailing retain the documented contract of `try_apply` itself.

### import.rs:512 — the `MarkdownFixer` erases the `<u>`/`Strong` distinction it owns

The fixer rewrites `<u>`/`</u>` into `Strong` events; both downstream
consumers then sniff raw source bytes to recover the distinction, and the two
peeks already disagree with the fixer's `is_u_open_tag` whitespace handling
(`< u >` classifies as Strong at the peek, Underline at the fixer). Fix: the
fixer emits the distinction explicitly (wrapper event or `MarkKind`), deleting
both peeks. The low-risk helper extraction above narrows the drift but not the
altitude.

### usv.rs:30 — `char_to_utf16` / `utf16_to_char` have no production consumers

Only the crate's own property tests use them; the module doc states the WASM
boundary passes raw USV and leaves UTF-16 to JS. Public surrogate-rounding
semantics to document and keep stable for a caller that does not exist. Fix:
delete until a UTF-16 consumer appears (`char_to_byte` stays). Public API of a
published crate.

### change_log.rs:77, change_log.rs:169 — unused `ChangeLog` surface

`len`, `is_empty`, `entries_after`, and the `CHANGE_LOG_DEFAULT_CAPACITY`
re-export (lib.rs:48) have no consumers outside the module's own tests;
session.rs uses only `record`/`record_change`/`revision`/`map_pos`/
`invalidate`. `entries_after` implies an incremental-reader protocol that is
not wired anywhere. Fix: drop or demote to `#[cfg(test)]` until the
delta-transport consumer lands. Published-crate API.

### serial.rs:455 — `content_key` is a second public name for `to_canonical_json`

A one-line alias whose only workspace use is one core dto test; both names are
frozen by the determinism contract. Fix: drop the alias and reference
`to_canonical_json` directly. Public API; core/dto.rs docs cite the name.

### serial.rs:115 — `to_canonical_value` builds the JSON tree twice

It constructs via `to_value()` then rebuilds the whole tree with
`sorted_value(...)`, and always clones + normalizes the corpus even when the
caller (live `Document` bodies, invariantly normalized) is already canonical.
Fix: emit the top-level keys and mark/line/island maps in sorted order
directly (props/attrs are already key-sorted by `normalize`), and/or a
no-clone path for known-normalized values. Interacts with the seam round-trips
(see `seams.md`).
