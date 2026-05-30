# Non-Strict Render & the Two Authoring Interfaces

> **Motivation**: a live-editing web app wants documents to preview and
> export while still in progress — without forcing authors to fill every
> unendorsed field or writing boilerplate to satisfy validation. At the
> same time, LLM/MCP authoring must stay held to a completeness bar. This
> proposal adds a non-strict *render* path and pins down how the two
> interfaces — UI form and MCP/LLM — share one document model without
> either one weakening the other.

## TL;DR

Render mode and validation verdict are **orthogonal**. Keep one validity
notion — strict validation answers *"is this document complete?"* — and
add a non-strict **render** path that type-empty-interpolates absent
fields into the plate-JSON projection only, never into the persisted
document. UI forms render non-strict and persist the sparse authored
truth; MCP/LLM `create_document` gates on strict. The same strict verdict
applies uniformly to documents of either origin.

Pre-1.0; not yet implemented. When built, the conceptual model graduates
into [SCHEMAS.md](../canon/SCHEMAS.md) with a pointer from
[BLUEPRINT.md](../canon/BLUEPRINT.md).

## Background

Today the render path (`compile_data` in `crates/quillmark/src/orchestration/`)
runs `coerce_and_validate`, which **hard-fails** on `must_fill_absent`: a
document that omits any field without a `default:` cannot render at all.
That is correct for a finished-document pipeline but wrong for a live form
editor, where the natural state of a document is *in progress*.

Two facts already in the codebase make the fix small:

- The quill **authoring contract** guarantees every quill renders its own
  type-empty blueprint (`blueprint_filled(FillBehavior::TypeEmpty)`,
  enforced by `every_quill_in_quiver_renders`). Type-empty input is the
  type-minimal valid document — the worst-case-but-renderable shape.
- `apply_defaults` already builds a throwaway `final_doc` for the plate
  JSON and does not persist it. The render projection is already the right
  place to inject values that never touch storage.

## The orthogonality principle

Two questions that are easy to conflate, kept separate:

| Question | Answered by | Always succeeds? |
|---|---|---|
| *Show me something now* | **render mode** (strict / non-strict) | non-strict: yes |
| *Is this document complete?* | **strict validation** | no — that's the point |

A document can be renderable (non-strict) and incomplete (fails strict) at
the same time. "Always compiles" is a render guarantee; "always valid" is
a validation verdict. They are not the same claim, and the design depends
on not fusing them.

## Strict validation — the single completeness verdict

Strict validation checks **structural completeness**, uniformly regardless
of who authored the document:

- every Must Fill field (no `default:`) is **present**;
- no `<must-fill>` sentinel survives;
- every value coerces to its declared type.

It does **not** check non-emptiness or semantics. A present `""` for a
Must Fill string passes strict. Semantic quality is steered by the
blueprint's `# e.g.` example hints (see the `default`/`example` framing in
[SCHEMAS.md](../canon/SCHEMAS.md)), not by the validator.

Consequence worth internalizing: a document in which **every** field is
present at its type-empty value (`""`, `0`, `false`, `[]`, `{}`,
first-enum) *passes strict* — present, non-sentinel, type-valid. A
**sparse** document (Must Fill fields absent) *fails strict*. The
difference between the two is exactly presence, which is what strict keys
on.

## Non-strict render — type-empty fill in the projection only

A non-strict render fills every absent field with its **type-empty**
value, in the plate-JSON projection that feeds the backend — and nowhere
else.

- Type-empty is honestly blank for almost every type: `""` (string,
  markdown, **date**, **datetime** — the validator accepts the empty
  string for both), `0`, `false`, `[]`, `{}`.
- The lone seam is `enum`: there is no empty enum member, so type-empty is
  `first_enum` — a real, meaningful variant. Because the fill lives only
  in the ephemeral projection, this appears **only in preview pixels**: the
  persisted document keeps the enum absent, and a form reload shows the
  dropdown unselected. The "looks-chosen-but-wasn't" value never hardens
  into storage or form state.
- **Non-persist invariant.** The type-empty fill must never be written
  back to the document. Type-empty is *indistinguishable from
  authored-empty*; persisting it collapses "field absent (untouched)" and
  "field present and empty" into one and destroys `must_fill_absent`
  forever (it keys on absence). The fill is part of the render, never part
  of the document.

## The two interfaces

Both produce documents in one shared model; they differ only in what they
gate on.

### UI form (non-strict)

- Renders non-strict for **both** preview and artifact export (PDF/SVG/PNG)
  — a blank form always produces a renderable result, no boilerplate.
- Emits **sparse** documents: an empty text box / unselected dropdown is
  *omitted* (treated as absent), so form-completeness and schema-presence
  coincide. The form's existing `FormFieldSource::Missing` / `Default`
  state remains the human-facing completeness signal — the analog of the
  `<must-fill>` sentinel for LLMs.
- **Persists the sparse authored truth**, never the fill.
- Strict validation is available as the *"is it done?"* gate, wired to the
  actions that need completeness (submit / publish), not to preview or
  draft export.

### MCP / LLM (strict)

- `create_document(markdown)` **gates on strict validation**. The LLM is
  handed the blueprint and must return a complete document: every Must
  Fill field present, no surviving `<must-fill>` sentinel.
- Strict enforces *structural* completeness; the blueprint's `# e.g.`
  hints carry the *semantic* guidance. "The LLM writes a semantically
  valid document" is guaranteed by **blueprint guidance + strict structural
  check together**, not by strict alone.

### Mixing the two

Both interfaces write into the same document model, so a document's strict
verdict is a uniform signal of "came from a finished process": LLM docs
pass, in-progress form docs fail, regardless of origin. No two-class
document semantics.

## Rejected alternatives

- **Option X — form populates type-empty at persist time** (document is
  always complete-and-valid). Rejected: it makes `must_fill_absent`
  vacuous (every key always present), bakes the enum first-variant value
  into storage as a silent fake choice, and creates two-class document
  semantics in a mixed-author ecosystem — which this project *is*, by
  design, because blueprints exist precisely so LLMs author these
  documents too.
- **Example-fallback in non-strict render** (fill absent fields from
  `example:` instead of type-empty). Rejected: an example is realistic but
  *not the value most authors want* (the canonized framing), so it
  camouflages incompleteness and risks leaking placeholder/PII content
  through a complete-looking export. Type-empty is honestly blank
  everywhere except enum. `example` keeps its existing home — blueprint
  `FillBehavior::Preview` for LLM/no-input *generation*, where a realistic
  shape genuinely helps — and does not follow onto the form render path.

## Implementation sketch

1. **Type-empty value producer.** Factor the type-empty logic out of the
   blueprint string emitter (`must_fill_value` / `first_enum` in
   `crates/core/src/quill/`) into a per-field `QuillValue` producer shared
   by blueprint emission and the render path — one source of truth for
   "the empty value for this field."
2. **Non-strict render mode.** On the render path
   (`crates/quillmark/src/orchestration/`), add a mode that, after
   coercion, interpolates type-empty `QuillValue`s for absent fields
   (mirroring `apply_defaults`, "authored value wins") and demotes
   `must_fill_absent` from a hard error to a warning. The fill goes into
   the `to_plate_json` projection only.
3. **Strict path unchanged.** `coerce_and_validate` keeps hard-failing on
   `must_fill_absent`; MCP `create_document` uses it as today.
4. **Surface.** Expose mode selection on the render bindings (Rust / Wasm /
   Python / CLI) and document that non-strict output is preview/export, not
   a completeness assertion.

## Graduation

Once implemented and tested, fold the conceptual model into
[SCHEMAS.md](../canon/SCHEMAS.md) as a "Strict vs. non-strict" section
(render mode ⊥ validation verdict; the two interfaces), add a one-line
pointer from [BLUEPRINT.md](../canon/BLUEPRINT.md)'s filled-blueprints
section, and delete this proposal.
