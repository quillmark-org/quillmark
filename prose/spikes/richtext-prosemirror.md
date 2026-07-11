# Spike: ProseMirror on RichText, no markdown intermediary

A best-effort spike to exercise the experimental `richtext` field type and the
WASM `LiveSession` navigation surface from a real editor, and to collect
feedback. It wires a **ProseMirror editor directly onto the `RichText` corpus**
(no markdown transport) with **bidirectional editor⇄preview navigation**, over
the airmark-quiver `usaf_memo` quill migrated to `richtext`.

## Update — all five filed issues addressed (PR #879)

The friction below drove five issues, all since fixed on `integration/richtext`
and integrated back into the spike:

| Finding (§) | Issue | Fix |
| --- | --- | --- |
| §1 no public delta/position map | #876 | `applyFieldDelta` / `mapFieldPos` / `revision` now on the public `LiveSession` |
| §2 no body-corpus mutator | #874 | `Document.setBody(corpus)` |
| §3 richtext flips Typst `str`→`content` | #873 | `plaintext(field)` plate helper + migration-hazard docs |
| §4 inline richtext → parbreak | #872 | `richtext(inline)` lowers to inline content |
| §6 stale wasm TS types | #875 | `type: … | richtext` (not `markdown`); `RichTextLine` gains `rule` |

The spike now consumes the new surface: `session.ts` sets the body with
`setBody` (no DTO round-trip) and forwards `applyFieldDelta`/`mapFieldPos`/
`revision`; `spike-richtext.test.js` exercises `setBody` (#874) and an
incremental `$body` delta with forward caret mapping (#876); the `usaf_memo`
render is now warning-free (#872). The findings below are kept as the original
report; the table above records their resolution.

## What was built (and where)

- **quillmark** — WASM built from this branch (`build-wasm.sh --ci`, v0.92.2-dev).
  `crates/bindings/wasm/spike-richtext.test.js` is the end-to-end proof: a
  corpus-native document (body + richtext fields as corpus objects, no markdown)
  loads via `Document.fromJson`, renders, and drives
  `regions`/`fieldAt`/`positionAt`/`locate`/`apply`. **5/5 pass** against the
  in-repo `usaf_memo` fixture.
- **airmark-quiver** — trimmed to `usaf_memo`; prose fields migrated to
  `richtext`; verified rendering (references show italic titles from richtext
  emphasis).
- **web-app** — `/spike/richtext` route: ProseMirror ⇄ corpus codec
  (`src/lib/spike/corpus.ts`), PM⇄USV caret mapping (`positions.ts`), a
  `LiveSession` wrapper (`session.ts`). **12/12** headless round-trip tests pass.

## What worked well

- **The corpus JSON is a clean editor target.** `crates/richtext/src/serial.rs`
  is precisely specified and byte-deterministic; mirroring it in TypeScript and
  writing a bidirectional ProseMirror codec (paragraphs, headings, lists, quotes,
  code, rules, inline marks, hard breaks) was mechanical. The golden-bytes test
  and the "line count == segment count" invariant caught mistakes early.
- **Seed-commits-corpus makes documents corpus-native for free.** A seeded
  `usaf_memo` document already carries the body and every richtext field as
  corpus objects in the storage DTO (`payload.items[].value`, `main.body`) — an
  editor edits those objects in place and never touches markdown.
- **`LiveSession` navigation is exactly as documented and genuinely bidirectional.**
  For the body (`$body`): `regions()` returns a span-bearing region;
  `positionAt(page, x, y)` returned an exact USV offset (`{field:'$body',pos:17}`
  at a clicked point); `locate('$body', span.start)` returned a caret rect that
  `fieldAt` resolved back to `$body`. `apply` reported `{pageCount, dirtyPages}`
  correctly on an edit. This is the strongest part of the surface.
- **richtext coercion accepts an editor corpus object directly** (revalidate +
  re-canonicalize), so the editor never has to emit markdown for a field.
- **Field-level region tracking via the plate binding works** — binding
  `signature-field(..., field: "signature_block")` surfaces a routable region
  for a field that has no body span.

## Friction, gaps, and bugs

Ordered by how much they shaped the spike.

### 1. The public runtime has no per-field delta / position map (the big one)

`LiveSession` (public `runtime.js`/`.d.ts`) exposes only whole-document
`apply(doc)`. The FFI carries `applyFieldDelta`, `mapFieldPos`, and `revision`
(CodeMirror `ChangeSet` semantics), but they are stripped from the canonical
surface (#850). Consequences for an editor:

- Every keystroke re-materializes the **entire** document (`Document.fromJson`
  of the whole DTO) and recompiles. The per-field text-splice path — the thing
  designed for exactly this loop — is unreachable.
- There is **no forward position mapping**, so caret sync across an edit has no
  correct primitive. The spike had to approximate PM⇄USV with `doc.textBetween`
  + code-point counting (`positions.ts`), which drifts around unusual structure.

**Ask:** surface `applyFieldDelta` / `mapFieldPos` / `revision` through
`runtime.js`. This is the single highest-value change for editor integrators.

### 2. No `setBody(corpus)` mutator

`replaceBody(body)` takes a **markdown string** only. `setField` rejects `$body`
(name must match `[A-Za-z_]…`, so the `$` is invalid). The only corpus-native
way to set the body is to hand-edit the storage DTO and round-trip through
`Document.fromJson`. A corpus-native editor for the body therefore cannot use
the ergonomic mutator path at all.

**Ask:** a `setBody(corpus)` (and/or let `setField("$body", corpus)` through).

### 3. Migrating a field to `richtext` silently changes its Typst type `str → content`

This gated the airmark migration field-by-field. `richtext` values reach the
plate as Typst **content**, so any template that does string operations on the
field breaks at *render* time, not load time:

- `memo_for` → `create-auto-grid` → `assert(type(item) == str)` → **hard failure**
  (`All items in content array must be strings`). `memo_for` had to stay
  `array<string>`.
- `ensure-string(scalar)` does `str(value)` → errors on content
  (`letterhead_title`, `letterhead_seal_subtitle`, `memo_from`).
- `.trim()` / `.starts-with()` on `classification` / `dissemination` / `cui_*`.

There is no schema signal or compile-time check that a `richtext` field is only
consumed in content position. Migration is a manual audit of the Typst package.

**Ask:** document this hazard prominently in the `richtext` migration guide;
consider a first-class "richtext → plaintext" projection for template authors
(pdfform already lowers richtext to plaintext for form fields — the same
projection would help Typst templates that need a string).

### 4. Inline richtext lowering emits `parbreak` warnings

Migrating the inline-richtext **array** fields (`signature_block`, `cc`,
`distribution`, `attachments`, `letterhead_caption`) renders correctly but emits
`parbreak may not occur inside of a paragraph and was ignored` — 4 per render,
from the **generated helper** (`lib.typ`), not the quill package. A single-`Para`
inline field lowers to block/paragraph content; when the package nests it in a
`par()` (hanging-indent signature lines), the paragraph break is illegal.
The 3-field fixture (`subject`/`tag_line`/`references`) has zero such warnings.

**Ask:** inline richtext (`inline: true`) should lower to **inline** content
(no paragraph/parbreak), so it composes cleanly wherever a template puts it.

### 5. Consumer type drift on a minor wasm bump

Linking this build (0.92.2-dev) over the web-app's pinned `0.92.1` broke
`svelte-check` in existing app code via TS-surface changes:

- `RenderSession` export **removed** (renamed to `LiveSession`).
- `Card.body` is now `RichText | string` (was `string`) — every consumer that
  treated the body as a string now type-errors.
- `Severity` dropped `'note'`.

None are in the spike code; all are pre-existing app code meeting the newer
build. **Ask:** call these out in a migration note, or keep a deprecated
`RenderSession` alias for a version.

### 6. Smaller TS-surface inaccuracies

- The core build's custom-section TS types `QuillFieldSchema.type` as
  `… | datetime | markdown` — stale; the Rust source of truth is `richtext`.
- The hand-written `RichTextLine` TS union (engine.rs) omits `rule`, which the
  Rust serializer emits — a consumer typing against it rejects a valid line.

### 7. Nav precision is body-shaped

Fine caret nav (`positionAt`/`locate` with a corpus span) is available for
content fields — `$body`, `richtext[]` elements, card content fields. A
richtext **scalar** referenced as `data.subject` surfaces as a scalar reference
site with **no span**, so per-field caret sync degrades to field-level (region)
nav for those. Expected per `PREVIEW.md`, but it means "click into the subject
text at this character" isn't available the way it is for the body.

### 8. Build / cold-start ergonomics (minor)

- `build-wasm.sh` requires an exact `wasm-bindgen-cli` match with `Cargo.lock`
  (0.2.118); the environment shipped 0.2.122. The up-front check is clear and
  correct, just a pinning cost.
- The `--ci` (unoptimized) typst backend is ~92 MB; the first `engine.open` in
  Node cold-starts in ~40 s (had to raise the test timeout and warm up in
  `beforeAll`). Fine for local dev; relevant to CI test budgets.

## Net

The `richtext` corpus and the `LiveSession` read-side navigation are ready for an
editor — the codec and the bidirectional nav both worked on the first serious
attempt. The gap between "spike" and "product" is almost entirely the **write
side**: expose the per-field delta + position-map path (#1), add a body-corpus
mutator (#2), make inline lowering inline (#4), and give template authors a
sanctioned richtext→string projection so migration stops being a package audit
(#3).
