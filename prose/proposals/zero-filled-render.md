# Zero-Filled Render & the Two Authoring Interfaces

> **Motivation**: a live-editing web app wants documents to preview and
> export while still in progress — without forcing authors to fill every
> unendorsed field or writing boilerplate to satisfy validation. The same
> leniency should serve LLM/MCP authoring, with one guard: a copied
> `<must-fill>` placeholder is an accident and must be rejected. This
> proposal adds a **zero-filled** *render* path and pins down how the two
> interfaces — UI form and MCP/LLM — share one document model and one
> validation behavior.

## TL;DR

Render fill and completeness verdict are **orthogonal**. The render path uses
**zero-filled render**: each absent field is filled with its type-empty
(zero) value, in the plate-JSON projection only, never in the persisted
document. A document that is merely *incomplete* (Must Fill fields absent)
renders fine; a *malformed* one — a value that won't coerce, or a surviving
`<must-fill>` sentinel — always errors. Strict completeness ("every Must Fill
present") is a separate concept, not a creation gate — and **in this sprint
it is not a shipped API**: the web app applies the same lenient render
everywhere, and the form's existing per-field state is the de-facto doneness
signal.

Pre-1.0; not yet implemented. When built, the conceptual model graduates
into [SCHEMAS.md](../canon/SCHEMAS.md) with a pointer from
[BLUEPRINT.md](../canon/BLUEPRINT.md).

## Scope (this sprint)

- **In:** zero-filled render (type-empty fill in the projection only) as the
  single, uniform render behavior for both preview and export; the
  malformed-sentinel and coercion-failure **hard errors**; the shared
  zero-value producer (with [blueprint-example-split.md](blueprint-example-split.md)).
- **Deferred:** a **warnings surface** for absent Must Fill fields (absence is
  zero-filled *silently* for now). A **standalone strict-completeness query
  API** — there is no finalize/publish gate in the web app, so nothing
  consumes it; the form's `FormFieldSource::Missing` already answers "is it
  done?" per field. Both remain in canon as concepts and future seams.

## Background

Today the render path (`compile_data` in `crates/quillmark/src/orchestration/`)
runs `coerce_and_validate`, which **hard-fails** on `must_fill_absent`: a
document that omits any field without a `default:` cannot render at all.
That is correct for a finished-document pipeline but wrong for a live form
editor — and, as it turns out, wrong for LLM drafting too — where the
natural state of a document is *in progress*.

Two facts already in the codebase make the fix small:

- The quill **authoring contract** guarantees every quill renders
  type-minimal valid input — today via the type-empty blueprint, and under
  this proposal expressed directly as a **zero-filled render of an empty
  document** (`every_quill_in_quiver_renders`). Type-empty input is the
  worst-case-but-renderable shape, so that test doubles as the proof that
  zero-filled render always compiles. (The companion
  [blueprint/example split](blueprint-example-split.md) retires the
  standalone type-empty *blueprint* mode in favor of this.)
- `apply_defaults` already builds a throwaway `final_doc` for the plate
  JSON and does not persist it. The render projection is already the right
  place to inject values that never touch storage.

## The orthogonality principle

Two questions that are easy to conflate, kept separate:

| Question | Mechanism | Fails when |
|---|---|---|
| *Show me something now* | **zero-filled render** | the document is **malformed** — a value that won't coerce, or a surviving `<must-fill>` sentinel. Never merely because it is incomplete. |
| *Is this document complete?* | **read the field provenance** (the form's `Document` / `Default` / `Missing` state) | any Must Fill field is **absent** |

A document can be renderable (zero-filled) and incomplete (a Must Fill field
is `Missing`) at the same time. "Always compiles" is a render guarantee;
"complete" is a separate verdict — surfaced this sprint by the form's
per-field state, not wired to render or to any creation gate. Naming the
render by its *fill* — not by a validation posture — keeps the two axes from
collapsing into one.

## Malformed vs. incomplete

Zero-filled render tolerates a document that is *incomplete* but rejects
one that is *malformed*. The line is about what the document **says**, not
how much of it is filled:

- **Incomplete** — a Must Fill field is **absent**. A coherent state: the
  author (human or LLM) simply did not provide the field. Zero-filled
  render fills it with the type-empty value; the omission is **not a render
  error**. (Surfacing the omission back to the author — a warning — is a
  deferred follow-up; today the form already shows it per-field via
  `FormFieldSource::Missing`.)
- **Malformed** — the document carries something that is **not a value**: a
  value that cannot coerce to its declared type, or a surviving
  `<must-fill>` **sentinel**. These always error, on every path.

The sentinel is the sharp case, and the reason this proposal treats LLM
input the same as form input. `<must-fill>` is the system's own
placeholder — stamped *"this is not a real value; replace it."* Leaving it
in the document is therefore provably an accident, almost always a verbatim
**transcription** of the blueprint that the author forgot to fill. Unlike
an absent field (a deliberate-looking omission), a surviving sentinel is
scaffolding leaked into content, and rendering it literally — `<must-fill>`
printed into a PDF — is never what anyone wants. So it earns a hard error
with a precise correction ("you left these fields as placeholders"), while
mere absence does not.

A human form never produces the sentinel: widgets emit values, and a user
who literally types `<must-fill>` yields the quoted, escaped string, not
the sentinel. So this rule fires **only on the LLM path** — exactly where
the transcription accident happens — without any interface-specific
special-casing.

**Strict completeness** — *every* Must Fill field present — remains a
well-defined concept, answered identically regardless of origin. It is a
**verdict** ("is this done?"), not a creation gate. This sprint reads it off
the form's per-field provenance rather than a dedicated API; wiring a
standalone query and any publish/submit/finalize gate is deferred (no such
gate exists in the web app today).

## Zero-filled render — type-empty fill in the projection only

A zero-filled render fills every absent field with its **type-empty (zero)**
value, in the plate-JSON projection that feeds the backend — and nowhere
else.

- The zero value is honestly blank for almost every type: `""` (string,
  markdown, **date**, **datetime** — the validator accepts the empty
  string for both), `0`, `false`, `[]`, `{}`.
- The lone seam is `enum`: there is no empty enum member, so the zero value
  is `first_enum` — a real, meaningful variant. Because the fill lives only
  in the ephemeral projection, this appears **only in preview pixels**: the
  persisted document keeps the enum absent, and a form reload shows the
  dropdown unselected. The "looks-chosen-but-wasn't" value never hardens
  into storage or form state.
- **Non-persist invariant.** The zero-fill must never be written back to
  the document. A type-empty value is *indistinguishable from
  authored-empty*; persisting it collapses "field absent (untouched)" and
  "field present and empty" into one and destroys `must_fill_absent`
  forever (it keys on absence) — and, critically, leaves a future quill
  schema migration unable to tell *author-never-touched* from
  *author-set-blank*, so it would migrate fakes as if they were intent. The
  fill is part of the render, never part of the document.

## The two interfaces

Both produce documents in one shared model and run the **same** render +
validation behavior (zero-fill absence; error on malformation). They differ
only in how doneness is surfaced to the author — and in the practical fact
that only the LLM path can emit a sentinel.

### UI form

- Uses zero-filled render for **both** preview and artifact export
  (PDF/SVG/PNG) — a blank form always produces a renderable result, no
  boilerplate.
- Emits **sparse** documents: an empty text box / unselected dropdown is
  *omitted* (treated as absent), so form-completeness and schema-presence
  coincide. The form's existing `FormFieldSource::Missing` / `Default`
  state is the human-facing doneness signal — and, this sprint, the doneness
  verdict itself.
- **Persists the sparse authored truth**, never the fill.
- No "is it done?" gate is wired to render or export — the web app renders
  **uniformly throughout**. A finalize gate can read the form provenance if
  and when one is needed.

### MCP / LLM (future consumer)

- `create_document(markdown)` would use the same zero-filled render. Absent
  Must Fill fields are zero-filled, not rejected — the LLM is **not** forced
  to fill every field, so it never has to invent data it does not have.
- The one accident guard, beyond type-validity: a surviving `<must-fill>`
  **sentinel errors**. `create_document` rejects it with diagnostics and
  the LLM retries (per the MCP workflow). This targets the common LLM
  failure — echoing the blueprint placeholder — precisely, without a blunt
  completeness gate.
- *(Deferred)* returning absent Must Fill fields as **warnings** in the
  response. Until the warnings surface ships, absence is zero-filled
  silently; the LLM still learns shape from the `example` document and hints.
- Contract for LLM authors: **fill each field or omit it — never leave
  `<must-fill>`.** The blueprint's sentinels mean "replace or delete," not
  content.
- Semantic quality is steered by the `example` document and the blueprint's
  `# e.g.` example hints (the `default`/`example` framing in
  [SCHEMAS.md](../canon/SCHEMAS.md)), not by validation.

This sprint lands the render behavior and the sentinel error on the shared
render path — independent of whether `create_document` itself ships now. The
MCP path is the future consumer that motivates the sentinel rule.

### Mixing the two

One document model, one render + validation behavior. A document's strict
completeness verdict is a uniform signal of "came from a finished process,"
independent of origin. No two-class document semantics.

## Rejected alternatives

- **Persist-time zero-fill** (the form populates type-empty into the
  *stored* document, so it is always complete-and-valid). Rejected: it
  makes `must_fill_absent` vacuous (every key always present), bakes the
  enum first-variant value into storage as a silent fake choice, blinds
  future schema migrations to author intent, and creates two-class document
  semantics in a mixed-author ecosystem — which this project *is*, by
  design, because blueprints exist precisely so LLMs author these documents
  too. Zero-fill must stay render-only.
- **Strict-gating the LLM on absence** (reject `create_document` when any
  Must Fill field is absent). Rejected in favor of erroring only on the
  malformed sentinel: absence is a coherent, zero-fillable state, and
  forcing the LLM to fill every field pushes it to invent data it does not
  have. The genuine accident worth rejecting is the placeholder
  transcription, which the sentinel rule targets exactly; completeness
  stays a verdict for any future finalize step.
- **Example-fallback render** (fill absent fields from `example:` instead
  of the type-empty zero value). Rejected for the render path: an example is
  realistic but *not the value most authors want* (the canonized framing),
  so it camouflages incompleteness and risks leaking placeholder/PII content
  through a complete-looking export. The zero value is honestly blank
  everywhere except enum. `example:` stays on the *generation* side — the
  `example` reference document (see
  [blueprint/example split](blueprint-example-split.md)), which deliberately
  prioritizes `example` › `default` › zero because **illustration** is its
  job — and does not follow onto the render path, whose job is **fidelity**.

## Implementation sketch

1. **Zero-value producer.** Factor the type-empty logic out of the
   blueprint string emitter (`must_fill_value` / `first_enum` in
   `crates/core/src/quill/`) into a per-field `QuillValue` producer shared
   by blueprint/example emission and the render path — one source of truth
   for "the zero value for this field." (Shared with the
   [blueprint/example split](blueprint-example-split.md), which uses the
   same producer for the `example` document's fallback.)
2. **One provenance-tagged field-resolution pass (the single render
   behavior).** On the render path (`crates/quillmark/src/orchestration/`),
   fold zero-fill into the existing default application: one pass resolving
   each field as **`authored` › `default` › zero** and **tagging which tier
   it came from**. The tags are exactly the form's trichotomy — `authored` =
   `Document`, `default` = `Default`, zero = `Missing` — so render fill and
   form provenance compute from one walk, not two. The resolved values go
   into the `to_plate_json` projection only (authored value wins, never
   persisted); the tier tags feed the form today and the deferred
   completeness/warnings surface later. `must_fill_absent` stops being a hard
   error (the zero tier handles it); `must_fill_sentinel` and coercion
   failures stay **hard errors**, centralized and always-on on the render
   path.
3. **(Deferred) Warnings + completeness query.** Surfacing absent Must Fill
   fields as render warnings, and exposing a standalone "every Must Fill
   present?" query for any future finalize/publish gate, are **out of scope
   this sprint** — the tier tags from step 2 already carry the information
   when a consumer appears.
4. **Surface.** Render returns the artifact on every binding (Rust / Wasm /
   Python / CLI); document that zero-filled output is preview/export, not a
   completeness assertion, and that a surviving `<must-fill>` is the one
   authoring accident render refuses to paper over.

## Graduation

Once implemented and tested, fold the conceptual model into
[SCHEMAS.md](../canon/SCHEMAS.md) as a "Zero-filled render" section
(render fill ⊥ completeness verdict; malformed vs. incomplete; the
provenance-tagged resolution; the two interfaces), add a one-line pointer
from [BLUEPRINT.md](../canon/BLUEPRINT.md)'s filled-blueprints section, and
delete this proposal. The deferred warnings surface and completeness-query
API graduate with the follow-up sprint that builds them.
