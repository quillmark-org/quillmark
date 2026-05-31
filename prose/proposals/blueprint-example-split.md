# Blueprint / Example — Two Named Reference Documents

> **Motivation**: `blueprint_filled(FillBehavior)` exposes an internal fill
> strategy as a public enum, overloading what a "blueprint" *is*. Split the
> public surface into two intent-named reference documents — `blueprint`
> (canonical `<must-fill>`) and `example` (illustrative) — demote the fill
> strategy to an internal detail, and retire the pure-zero blueprint mode as
> a dead branch.

## TL;DR

A blueprint is **always** the canonical `<must-fill>` authoring document —
no modes. Add a second named output, `example`, the illustrative
consolidation of the schema: each field renders its `example:`, else its
`default:`, else the zero value. Demote `FillBehavior` from a public
parameter to an internal `FillSource` strategy. The pure-zero mode (today's
`TypeEmpty` blueprint) is removed: its only consumer — the quiver authoring
contract — instead zero-filled-renders an empty document (see
[zero-filled-render.md](zero-filled-render.md)).

Pre-1.0; not yet implemented. When built, graduates into
[BLUEPRINT.md](../canon/BLUEPRINT.md).

## Background

The current public surface (in `crates/core/src/quill/`) is `blueprint()`
plus `blueprint_filled(FillBehavior)` with three variants:

| `FillBehavior` | Fills Must Fill cells with | Consumer |
|---|---|---|
| `Strict` | `<must-fill>` sentinel (identical to `blueprint()`) | authoring surface |
| `Preview` | the field's `example:`, else the zero value | CLI `render` with no input file |
| `TypeEmpty` | zero value everywhere (`""`, `0`, `false`, `[]`, `{}`, first enum) | quiver authoring-contract test |

Two problems:

- **"Blueprint" is overloaded.** A consumer must understand three fill
  strategies to know what a blueprint even is. The canonical authoring
  surface and a populated sample are not the same artifact, yet both are
  "a blueprint."
- **It leaks an internal concern as a public flavor.** `TypeEmpty` is the
  render *floor* (the type-minimal valid input), not a document anyone
  authors from. Exposing it as a blueprint variant conflates "document I
  fill in" with "minimal render input."

## Two named reference documents

| Output | Intent | Fill source | Sentinels? |
|---|---|---|---|
| `blueprint` | *"give me the form to fill"* | `default`, else `<must-fill>` | yes |
| `example` | *"show me a filled-out one"* | `example` › `default` › zero | no |

- **Each artifact orders its value sources for its own purpose** — there is
  no single cross-output "default always wins" rule. The blueprint shows an
  Endorsed field's `default:` (and a `<must-fill>` sentinel otherwise); the
  `example` document prioritizes the illustrative `example:`, falling back to
  `default:`, then the zero value.
- `example` is example-*first* but not fully populated: a field with neither
  an `example:` nor a `default:` renders at its zero value. So it is
  "examples where defined, defaults next, blank otherwise" — the most
  illustrative document available, not a guaranteed-complete one. State this
  in the contract so no one expects every field filled.
- **The endorsed-field divergence is intentional.** A field carrying *both*
  a `default:` and an `example:` shows its example in the `example` document
  but its default on the render path — the example optimizes for
  *illustration*, render optimizes for *fidelity* (and explicitly never
  consults `example:`, see [zero-filled-render.md](zero-filled-render.md)).
  The two artifacts serve different masters; this is by design, not an
  exception bolted onto a shared rule. (In practice the divergence only fires
  when a field has both, which is uncommon — examples earn their keep mostly
  on Must Fill fields that have no default.)
- **Naming.** The output is named `example`. It is the document-level
  counterpart of the field-level `example:` property — literally the
  consolidation of those field examples (and defaults). The shared word is a
  faithful part-whole correspondence, not a namespace collision: a Rust
  method / CLI subcommand and a schema key never clash as symbols. The one
  mild surprise — `example` is example-*first*, not example-*only* — is
  neutralized by canonizing the definition: *"a quill example is the
  illustrative consolidation: each field's `example:`, else its `default:`,
  else blank."*

This is the same axis canonized at the field level — placeholder-to-fill
(`<must-fill>` / no `default`) vs. illustrative value (`example:`) — lifted
to the document level.

## Internal unification — one emitter, a `FillSource` strategy

The emitter already walks the schema and pulls a per-field value from a
source. Demote `FillBehavior` to an **internal** `FillSource` with two
variants, each composing the shared zero-value leaf with its own precedence:

- `Sentinel` → `blueprint`: `default`, else `<must-fill>`.
- `Example` → `example`: `example` › `default` › zero.

The per-field **zero value** (`must_fill_value` / `first_enum`, factored into
a `QuillValue` producer) is a shared leaf utility — called by the `Example`
fallback here *and* by zero-filled render. The two proposals share one
zero-value producer; this is the "unified internally" goal. Precedence lives
*above* the leaf, composed per artifact, so the `example` document's
example-first order is a local choice that never touches the producer or the
render path.

Two variants for two artifacts — no configurable precedence policy. If a
third artifact ever appears, add a third variant then; generalizing now would
invent meaningless combinations (a "blueprint with examples" nobody wants).

## The dead branch: pure-zero blueprint

`blueprint_filled(TypeEmpty)` emits a document with every Must Fill field at
its zero value. Its only caller is the quiver authoring-contract test
(`every_quill_in_quiver_renders`). Once zero-filled render exists, this mode
has no reason to live:

- The contract — *"the plate renders type-minimal valid input"* — is more
  directly expressed as **zero-filled render of an empty document** for each
  quill. No blueprint string is generated or re-parsed.
- The canonical blueprint can't serve as that fixture anyway: its
  `<must-fill>` sentinels are **malformed** under zero-filled render (they
  error), so "render the blueprint" was never the literal test.

So the blueprint emitter keeps exactly two fill sources — `Sentinel` and
`Example`. The pure-`Zero` *mode* leaves blueprint logic entirely; the zero
*value* survives as the example fallback and the render floor.

`Preview` likewise stops being a blueprint mode: "render with no input"
becomes "render the `example` document" (or a zero-filled render of an empty
doc, when blank is wanted).

## Bindings surface

Replace the Rust `blueprint_filled(behavior)` escape hatch with `blueprint()`
+ `example()`. Wasm/Python/CLI already expose only `blueprint`; add the
`example` accessor. CLI `render` with no input renders the `example`
document.

## Scope (this sprint)

This proposal ships the public split (`blueprint()` + `example()`) and the
internal `FillSource` demotion. The render-side companion
([zero-filled-render.md](zero-filled-render.md)) ships zero-filled render and
the malformed-sentinel error; its **warnings surface** and **standalone
completeness query** are deferred to a follow-up sprint and are not part of
this work.

## Rejected / open

- **Keep `FillBehavior` public** — rejected. It is an internal strategy; the
  public surface should name *intents* (blueprint vs. example), not
  strategies.
- **Generalize `FillSource` into a configurable precedence policy** —
  rejected as overengineering. Two artifacts, two variants.
- **(resolved) The second output's name** — decided: `example`. The
  apparent collision with the `example:` field property is a faithful
  part-whole correspondence, not a symbol clash, and is closed by canonizing
  the definition above.

## Graduation

Fold into [BLUEPRINT.md](../canon/BLUEPRINT.md): a "Two reference documents"
section replaces "Filled blueprints" / the `FillBehavior` table, states the
per-artifact precedence (blueprint: `default`-else-sentinel; example:
`example` › `default` › zero), defines the quill example as the illustrative
consolidation, and cross-links the zero-value producer to the
zero-filled-render section. Delete this proposal.
