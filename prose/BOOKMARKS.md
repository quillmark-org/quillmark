# Bookmarks

Notes on simplifications and refactors deliberately deferred. Each entry
describes a known cleanup opportunity that isn't worth a separate
proposal yet — a placeholder so the insight isn't lost between releases.

When an entry is acted on, delete it (or promote it to a proper
proposal in `proposals/`). When an entry has stayed here for a year
without action and nobody can argue for it, delete it too.

---

## Plate JSON construction lives in one open-coded place

**Where:** `Document::to_plate_json` in
`crates/core/src/document/mod.rs:194-234`.

**What:** The plate-JSON wire shape (the `$quill` / `$body` / `$cards`
+ flat user fields object) is hand-built with `serde_json::Map::insert`
calls. After the 0.83.0 cleanup it's the **only** place in
`quillmark-core` that produces this shape — the V0_82_0 storage DTO is
unrelated, and the legacy synthetic `QUILL`/`CARD` discriminator
fields are gone.

**Why it might want unification:** the function is essentially a
`From<&Document> for serde_json::Value` impl in disguise. Promoting it
to a `From` impl (or a small dedicated type with a `Serialize` impl)
would:

- Make the contract type-level rather than positional.
- Open the door to a typed `PlateJson` struct that backends could
  consume directly without re-deriving the shape from `serde_json::Value`
  lookups (cf. the Typst helper's `data.at("$cards")` /
  `card.at("$kind")` accesses).
- Let us share emit helpers between Document → plate-JSON and Card →
  card-JSON without duplicating the field-loop.

**Why we punted:** the shape is small (four `$` keys + a flat user
spread) and is consumed by backend templates as opaque JSON anyway —
turning it into a typed struct only pays off if more than one backend
ends up needing typed access. Defer until the second canvas-capable
backend lands, or until the `__meta__` shim in the Typst helper
package is generalised.

**Notes for the next implementer:** keep the `$`-key ordering
(`$quill`, `$body`, `$cards`) deterministic — `to_plate_json`
relies on `serde_json/preserve_order` and consumers may have started
content-hashing the result. Touch `crates/backends/typst/src/lib.rs`
(`transform_markdown_fields`) at the same time so the schema
walker uses the same shape.

---

## Typed container empty default loses inline shape documentation

**Where:** `write_typed_table_field` / `write_typed_object_field` in
`crates/core/src/quill/blueprint.rs`.

**What:** When a typed container declares `default: []` (or `default: {}`)
the blueprint now renders the value inline — `refs: []  # array<object>;
skip-ok` — and emits no row/property shape. Inner shape is only surfaced
when the container is Must Fill (no `default:` at all), via the synthetic
row / per-property recursion path.

**Why we accepted the loss:** prior to this, an empty container default
asymmetrically forced a synthetic row with leaf sentinels — the field's
own cell signal said "Must Fill" even though `default: []` is a fully
shippable value per `prose/canon/SCHEMAS.md`. The asymmetry leaked into
the rendering code (an override parameter on `inline_annotation`) and
disagreed with the canonical "type-empty defaults are Endorsed" rule.
Removing the asymmetry was a one-rule simplification — uniform cascade:
`default.is_some()` → Endorsed → render the default; `default` absent →
Must Fill → render the shape with sentinels.

**What's left for a future pass:** schema authors who want both "shippable
empty" *and* inline shape documentation now have no single knob. Options
when this comes up:

- Add a leading `# rows shaped like: {…}` comment for typed containers
  with an empty default, derived from `properties:`. Cheap to emit, no
  semantic conflict (it's a comment).
- Promote `example:` rendering for typed containers so an `example:
  [{…}]` under an empty default actually shows a shape sketch — today
  the example collapses to a single-line `# e.g. …` regardless.
- Add an explicit `ui.show_shape: true` flag on the field.

Defer until a real authoring case asks for it. The canon update lives
in `prose/canon/BLUEPRINT.md` "Typed tables" / "Typed dictionaries"
which already point readers here.
