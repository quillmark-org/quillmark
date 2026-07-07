# Phase 3 — edit surface

The engine already stores, renders, and navigates `RichText`; this phase wires
**editing**: per-field text splices, mark and line op channels, monotonic
revision with a bounded change log, and a form-editor binding on the phase-1
mark freeze. The stale-text writer (`delta::diff_import`) stays the path for
LLM/MCP whole-document markdown; the live form emits deltas directly.

Gated on [phase 2](phase-2.md) (landed through PR-G) and the [phase-1
freeze](phase-1.md). Spike-A (phase-0 residual gate) closed in PR-A before PR-B
freezes transport shape.

**Status: open.** PR-A (Spike-A editor binding) and PR-H (form POC) **reported**
on branch `spike/richtext-phase-3` (pushed to origin); runnable probes live
there, not on `integration/richtext`. PR-B (Myers/LCS diff) is next for the
engine path.

## Spike branch

`spike/richtext-phase-3` — throwaway PR-A + PR-H harness in
`crates/richtext-spikes/` (workspace member, outside `default-members`), plus
the experimental `LiveSession` navigation surface the form POC exercises.
Checkout that branch to run probes; this integration branch carries findings
only.

```bash
cd crates/richtext-spikes/editor-pm && npm install && npm test   # Spike-A
cd crates/richtext-spikes/form-poc && npm install && npm run dev  # PR-H manual
```

## PR map

1. **PR-A — Spike-A: ProseMirror binding probe.** Throwaway harness in
   `crates/richtext-spikes/editor-pm/`. Corpus JSON ↔ ProseMirror doc for
   formatting marks; exercises edge-expand, adjacent-merge-at-insertion, and
   cross-kind overlap. Pass criterion: every scenario round-trips to the same
   normalized corpus marks the Rust serializer commits to — disagreements stay
   in editor config, not in the model. Findings in § Spike-A.
2. **PR-B — Myers/LCS diff.** Replace the phase-1 prefix/suffix trim in
   `delta::diff` with a minimal edit script (Myers or `similar`). Disjoint
   edits no longer collapse anchors sitting between them; stale-text rebase
   keeps the move detector, gains tighter deltas for the change log.
3. **PR-C — revision + bounded change log.** Monotonic `revision: u64` on the
   live session; ring buffer of per-field entries `{ revision, path, text_delta,
   mark_ops?, line_ops? }`. `map_pos` composes across the log (CodeMirror
   `ChangeDesc` / ProseMirror mapping semantics — already implemented on a
   single [`Delta`](../../crates/richtext/src/delta.rs)). Stale positions map
   forward instead of silently reading current compile output.
4. **PR-D — mark and line op channels.** Text splices stay attribute-free
   ([`delta`](../../crates/richtext/src/delta.rs)); mark edits (`MarkOp`) and
   line/block edits (`LineOp` — split, join, `kind`, `containers`) are
   separate op streams that rebase through text deltas via `map_pos`.
   `RichText::apply_text_delta`, `apply_mark_ops`, `apply_line_ops`; normalize
   after every apply.
5. **PR-E — fallible document mutators.** Typed richtext field writers on
   [`Card`](../../crates/core/src/document/edit.rs): set corpus, apply delta
   bundle, import-with-error (retire silent `replace_body` degradation).
   Document-level batch apply with invariant enforcement unchanged.
6. **PR-F — preview wire + revision stamp.** Append optional `revision` to
   [`RenderedRegion`](../../crates/core/src/region.rs) and tag read responses
   (`regions`, `fieldAt`, `positionAt`, `locate`) — the additive-optional
   discipline phase 2 froze for this. Session applies field deltas against a
   base revision; mismatch surfaces as a diagnostic, not a silent stale read.
7. **PR-G — WASM session API.** `LiveSession::applyFieldDelta` (or equivalent
   batch) in [`bindings/wasm`](../../crates/bindings/wasm/); vitest coverage.
   Whole-document `apply(doc)` remains for the markdown/LLM path; form fields
   use the delta path.
8. **PR-H — form-editor POC.** First end-to-end consumer:
   `crates/richtext-spikes/form-poc/` — Vite dev app with ProseMirror inline
   fields, `LiveSession.apply`, and canvas preview on `usaf_memo`. Reuses the
   PR-A bridge; whole-document apply until PR-G lands. Findings in § PR-H.

## Design spine

### One edit language, two writers

Both writers target the same per-field op bundle; only the encoder differs.

| Writer | Encoder | When |
|--------|---------|------|
| Form editor | native delta + mark/line ops (PR-G/H) | keystrokes, toolbar |
| Stale text (MCP, saved `.qmd`) | cold import + `diff_import` | whole-document markdown replace |

The engine never sees markdown on the hot path for stored corpus fields. String
fields in markdown documents still re-import at `compile_data` (phase-2 risk
register) until authored structurally.

### Text splices, not attributed ops

[`Delta`](../../crates/richtext/src/delta.rs) is `retain` / `insert` /
`delete` over USV — CodeMirror `ChangeSet` semantics, **not** Quill-Delta.
Formatting does not ride as op attributes; overlapping same-kind marks and two
identity anchors over one range are impossible in an attribute map and are
first-class in the corpus (Peritext-style free overlap + identity handles).
Mark and line edits are PR-D's separate channels.

### Revision earns its keep here

Phase 2 deliberately omitted revision: `apply` is transactional, the consumer
single-owner and serial, regions key on `(field, corpus range)` alone (see
[phase-2.md](phase-2.md) § Emit, regions, navigation). Phase 3 adds revision
because a form cursor, a highlight box, or an anchor position from revision *N*
must map forward through edits at *N+1…* — `delta::map_pos`, not merely
recompile-and-hope. The change log is **bounded** (ring buffer); consumers that
fall behind re-read at the current revision.

### Editor binding contract (Spike-A)

The phase-1 freeze stores **ranges**, not editor policies. Edge-expand at
insertion, adjacent-merge of same-kind marks, and cross-kind overlap freedom
are editor-owned; `RichText::normalize` is the single canonicalizer on export
from any editor. Spike-A binds ProseMirror (rich mark ranges, documented
boundary semantics) and confirms no scenario requires a serialization or
normalization rule change. CodeMirror remains the reference for **text**
delta/`map_pos` semantics; ProseMirror's `StepMap` aligns and the bridge
translates.

## Spike-A (PR-A)

**Probe:** `crates/richtext-spikes/editor-pm/` — vitest, headless
`prosemirror-model` + custom schema for the frozen formatting mark set.

**Scenarios (minimum matrix):**

| Scenario | Editor behavior under test | Pass |
|----------|---------------------------|------|
| Edge-expand | Type at trailing strong edge | Exported corpus mark includes typed chars; re-load is stable |
| Adjacent-merge | Extend strong by one char at boundary | Corpus unions to one range (normalize) |
| Cross-kind overlap | `strong` + `emph` on same span | Both marks stored; order canonical |
| Boundary split | Delete char at mark edge | Trimmed mark; no `\n` edge |
| Inline field | Single-para, no containers | Matches `richtext(inline)` shape |
| Astral USV | Emoji inside emph range | One USV per astral; mark range stable |

**Outcome: no model change.** ProseMirror's edge-expand at insertion,
adjacent-merge when extending a mark by typing, and cross-kind overlap all
export to corpus mark ranges that match the phase-1 normalizer — disagreements
stay in PM's stored-mark / inclusive config, not in serialization. The JS
`normalizeFormattingMarks` mirrors the Rust rules; export is the single
canonicalizer. Astral USV counting passes; full UTF-16 ↔ PM position mapping
for live editing remains a PR-H production concern (risk register §1).

**Bridge pattern (reused in PR-H):** `corpusToDoc` / `docToCorpusMarks` over a
single-paragraph schema; `usvToPmPos` maps USV offsets to PM doc positions.
Headless `EditorSession` drives scenarios without a DOM.

## PR-H — form POC findings

**Probe:** `crates/richtext-spikes/form-poc/` — Vite app on `usaf_memo` 0.2.0.

**What it exercises:**

- `subject` and `tag_line` as `richtext(inline)` ProseMirror fields (toolbar:
  strong / emph / underline); `$body` stays a markdown textarea on the
  whole-document path.
- Debounced `LiveSession.apply(doc)` (~280 ms) after any field edit — whole-doc
  recompile, not per-field deltas (PR-G not started).
- Canvas preview via `session.paint()`; region highlight overlay from
  `session.regions()`.
- Bidirectional cross-navigation: form focus ↔ highlight ↔ canvas click via
  `fieldAt()` / `locate()` / `positionAt()`.
- Playwright e2e: boot + canvas paint, live apply per field, toolbar toggle,
  form ↔ canvas focus routing.

**Insights:**

1. **Whole-doc apply is viable for a manual POC** — inline richtext fields
   round-trip through `doc.setField(corpus)` + `session.apply(doc)` with live
   preview and region geometry intact. Confirms the phase-2 session surface is
   sufficient for a first authoring UI before PR-G field deltas land.
2. **PR-A bridge shares cleanly** — `form-poc` imports `editor-pm` source via a
   Vite alias; `vite.config.js` pins ProseMirror packages to one
   `prosemirror-model` instance so schema and view agree.
3. **`markdownInlineToEditor` is spike-only bootstrap** — parses `**` / `*`
   so pre-coercion markdown-shaped seeds (e.g. `**Richtext** form POC`) load
   into ProseMirror before compile-time coercion; not a production codec.
4. **`usaf_memo` `subject` → `richtext(inline)`** on the spike branch exercises
   two inline fields against one quill; integration fixture unchanged until
   PR-H promotes.
5. **Experimental WASM navigation surface holds** — `regions()`, `pageSize()`,
   `fieldAt()`, `positionAt()`, `locate()` on `LiveSession` are enough for
   overlay highlight + canvas hit-test without revision stamps (PR-F).
6. **Inline-first is the right POC cut** — multi-line `$body` and block edits
   need `LineOp` (PR-D) before a block-capable editor; block canvas stays out
   of scope.

## Sequencing invariant

- Spike-A reports before PR-B freezes change-log entry shape (text delta only
  until PR-D, but the log slot must accommodate mark/line ops). **Closed.**
- PR-B (minimal diff) before PR-C (log stores deltas worth replaying).
- PR-D (mark/line ops) before PR-E (mutators emit them).
- PR-F (revision stamp) before PR-G (WASM exposes revision-aware apply).
- PR-A's bridge pattern is reused in PR-H; no second editor serialization.

Phase 2 outputs (`RenderedRegion`, canonical bytes, segment maps) extend; none
are discarded.

## Risk register

1. **UTF-16 / USV boundary in the editor bridge.** JS editors count UTF-16;
   the corpus counts USV. The bridge must use the same conversions as
   [`usv`](../../crates/richtext/src/usv.rs) (wasm re-exports or shared tests).
   Spike-A covers astral chars in headless tests; live `EditorView` caret
   placement across surrogate pairs is unexercised until production wiring.
2. **Line/block edits vs inline-first POC.** PR-H targets `richtext(inline)`;
   multi-line fields need `LineOp` split/join (PR-D) before a block-capable
   editor ships. Block canvas remains out of scope.
3. **Coexistence with whole-doc `apply`.** Markdown/MCP path stays
   `diff_import` at document granularity; session must apply stale-text and
   field-delta edits in a defined order (revision monotonicity across both).

## Related

- #831 (this rework), #829 (regions — phase 2)
- [INDEX.md](INDEX.md), [phase-0.md](phase-0.md), [phase-1.md](phase-1.md),
  [phase-2.md](phase-2.md)
- Branch `spike/richtext-phase-3` (origin) — runnable PR-A + PR-H probes
- `prose/canon/PREVIEW.md`, `DOCUMENT_STORAGE.md`, `SCHEMAS.md`
- `crates/richtext/src/delta.rs`, `crates/core/src/document/edit.rs`,
  `crates/core/src/region.rs`
