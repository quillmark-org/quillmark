# Bookmarks

Notes on simplifications and refactors deliberately deferred. Each entry
describes a known cleanup opportunity that isn't worth a separate
proposal yet — a placeholder so the insight isn't lost between releases.

When an entry is acted on, delete it (or promote it to a proper
proposal in `proposals/`). When an entry has stayed here for a year
without action and nobody can argue for it, delete it too.

---

## Typed table empty default loses inline row-shape documentation

**Where:** `append_typed_table` in `crates/core/src/quill/blueprint.rs`.

**What:** When a typed table declares `default: []` the blueprint renders
the value inline — `refs: [] # array<object>` — and emits no row shape.
Inner row shape is only surfaced when the table is Unendorsed (no
`default:` at all), via the synthetic-row recursion path.

This used to apply to typed *dictionaries* too (`default: {}`), but that
half is resolved: `{}` now expands to the field's zero-filled shape, so an
empty endorsed object shows its keys. Arrays don't expand — a `default: []`
carries no element to expand from — so the typed-table case remains.

**Why we accepted the loss:** the alternative — forcing a synthetic row
under an empty default — asymmetrically stamps the field "Unendorsed" even
though `default: []` is a fully shippable value per
`prose/canon/SCHEMAS.md`. The uniform cascade (`default.is_some()` →
Endorsed → render the default; `default` absent → Unendorsed → render the
shape with markers) is the simpler rule, and the row shape that's lost is
recoverable through `example:`.

**What's left for a future pass:** schema authors who want both "shippable
empty" *and* inline row-shape documentation have no single knob. Options
when this comes up:

- Add a leading `# rows shaped like: {…}` comment for typed tables with an
  empty default, derived from the element `properties:`. Cheap to emit, no
  semantic conflict (it's a comment).
- Promote `example:` rendering so an `example: [{…}]` under an empty
  default shows a shape sketch — today it collapses to a one-line
  `# e.g. …` regardless.
- Add an explicit `ui.show_shape: true` flag on the field.

Defer until a real authoring case asks for it. The canon update lives in
`prose/canon/BLUEPRINT.md` "Typed tables", which points readers here.
