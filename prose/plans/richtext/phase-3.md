# Phase 3 — edit surface

The engine already stores, renders, and navigates `RichText`; this phase wires
**editing**: per-field text splices, mark and line op channels, monotonic
revision with a bounded change log, and a form-editor binding on the phase-1
mark freeze. The stale-text writer (`delta::diff_import`) stays the path for
LLM/MCP whole-document markdown; the live form emits deltas directly.

Gated on [phase 2](phase-2.md) (landed through PR-G) and the [phase-1
freeze](phase-1.md). Spike-A (phase-0 residual gate) closed in PR-A before PR-B
freezes transport shape.

**Status: open.** PR-A + PR-H **reported** on `spike/richtext-phase-3` (origin).
**PR-B–G landed** on `integration/richtext` (`0c108163a`…). **PR-H**
(form-editor POC promotion) is the remaining integration step.

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
2. **PR-B — Myers/LCS diff.** **Landed.** Replaced the phase-1 prefix/suffix
   trim in [`delta::diff`](../../crates/richtext/src/delta.rs) with a Myers/LCS
   minimal edit script via the `similar` crate. Single-line text diffs at char
   granularity; multi-line text diffs at line granularity so paragraph reorders
   surface as whole-line insert spans the move detector can match. Disjoint
   edits no longer collapse anchors sitting strictly between them; stale-text
   rebase keeps the move detector. Findings in § PR-B.
3. **PR-C — revision + bounded change log.** **Landed.**
   [`change_log::ChangeLog`](../../crates/richtext/src/change_log.rs) — monotonic
   `revision: u64`, ring buffer (default 256) of
   [`FieldChange`](../../crates/richtext/src/change_log.rs) entries,
   [`map_pos`](../../crates/richtext/src/change_log.rs) composes
   [`Delta::map_pos`](../../crates/richtext/src/delta.rs) per field.
   [`LiveSession`](../../crates/core/src/session.rs) owns the log;
   `record_field_delta` / `record_field_change`, `map_field_pos`. Findings in §
   PR-C.
4. **PR-D — mark and line op channels.** **Landed.**
   [`ops`](../../crates/richtext/src/ops.rs): `MarkOp`, `LineOp`,
   `RichText::apply_text_delta`, `apply_mark_ops`, `apply_line_ops`,
   `apply_field_change` (text → line → mark; normalize after each).
   `FieldChange` carries `mark_ops` + `line_ops`; change-log `map_pos` stays
   text-delta-only. Findings in § PR-D.
5. **PR-E — fallible document mutators.** **Landed.** Typed richtext body
   writers on [`Card`](../../crates/core/src/document/edit.rs):
   `set_body_corpus` (native corpus set), `apply_body_change` (text delta →
   line ops → mark ops bundle), `import_body_delta` (whole-document markdown
   replace via `diff_import`, returns the text delta for change-log
   recording), and a now-fallible `replace_body` — the silent degrade-to-empty
   retired for [`EditError::BodyImport`](../../crates/core/src/document/edit.rs)
   /`CorpusApply`. Existing field/kind invariant enforcement unchanged.
   Findings in § PR-E.
6. **PR-F — preview wire + revision stamp.** **Landed.** Optional `revision` on
   [`RenderedRegion`](../../crates/core/src/region.rs) and
   [`CorpusHit`](../../crates/core/src/region.rs), stamped by the
   [`LiveSession`](../../crates/core/src/session.rs) wrapper on `regions` /
   `locate` / `position_at` (backends emit `None`; `field_at` stays a bare,
   revision-invariant address). `ensure_base_revision` /
   `record_field_delta_at` / `record_field_change_at` guard a field delta
   against its base revision — a mismatch is a `session::revision_mismatch`
   diagnostic, not a silent stale write. Additive-optional throughout. Findings
   in § PR-F.
7. **PR-G — WASM session API.** **Landed.** `LiveSession.applyFieldDelta(doc,
   field, baseRevision, delta)` in [`bindings/wasm`](../../crates/bindings/wasm/)
   splices the main body corpus (`apply_body_change`), recompiles, and records
   the delta; a `revision` getter and `mapFieldPos(field, base, pos, assoc)`
   complete the surface, and `FieldRegion`/`CorpusHit` carry the optional
   `revision`. Whole-document `apply(doc)` remains the markdown/LLM path; form
   fields use the delta path. vitest coverage in `canvas.test.js`. Findings in
   § PR-G.
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
| Stale text (MCP, saved document) | cold import + `diff_import` | whole-document markdown replace |

The engine never sees markdown on the hot path for stored corpus fields. String
fields in markdown documents still re-import at `compile_data` (phase-2 risk
register) until authored structurally.

### Text splices, not attributed ops

[`Delta`](../../crates/richtext/src/delta.rs) is `retain` / `insert` /
`delete` over USV — CodeMirror `ChangeSet` text-splice semantics.
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

## PR-B — Myers/LCS diff findings

**Implementation:** `similar::TextDiff` — `from_chars` for inline text,
`from_lines` when either side contains `\n`. Ops coalesce adjacent
`Retain`/`Delete`/`Insert` into one [`Delta`](../../crates/richtext/src/delta.rs).

**Insights:**

1. **Char Myers alone fragments block moves** — a paragraph reorder char-diff
   into tiny insert shards (`"econd"`, `"fir"`, …) that fail the move detector's
   contiguous-needle + inserted-span overlap rule. Line-level Myers fixes this;
   char-level remains correct for `richtext(inline)` fields.
2. **Anchors strictly between disjoint edits survive via `map_pos`** — no move
   detector needed when the unchanged middle is a contiguous `Retain` run. Anchors
   touching a char-alignment boundary between an edit and unchanged text (e.g.
   shared prefix char) can expand with the edit; that is Myers alignment, not
   coarse-collapse.
3. **All phase-1 rebase tests pass** — including block-move re-home, unrelated-
   survivor rejection, and property-test anchor survival.

## PR-C — revision + change log findings

**Implementation:** [`change_log.rs`](../../crates/richtext/src/change_log.rs),
[`LiveSession`](../../crates/core/src/session.rs).

1. **Revision 0 is the pre-edit baseline** — not a log entry. `map_pos` returns
   `StaleRevision` only when `base_revision > 0` and precedes the oldest
   retained entry.
2. **`map_pos` composes text deltas per field** — other fields and mark/line ops
   in the same entry do not participate in position mapping.

## PR-D — mark/line op findings

**Implementation:** [`ops.rs`](../../crates/richtext/src/ops.rs); `FieldChange`
extended in [`change_log.rs`](../../crates/richtext/src/change_log.rs).

1. **Apply order in one bundle:** text delta → line ops → mark ops; normalize
   after each step.
2. **Text delta syncs `lines` to `\n` insert/delete** in the delta; `LineOp`
   split/join also splices `\n`. Change-log `map_pos` still composes text deltas
   only — record `\n` edits in `text_delta` when mapping stale positions through
   the log.
3. **Mark ranges in mark ops are post-text-delta coordinates** within the same
   revision.

## PR-E — document mutator findings

**Implementation:** [`edit.rs`](../../crates/core/src/document/edit.rs); one new
crate-internal `Card::body_mut`; `RichText` re-exported from
[`quillmark_core`](../../crates/core/src/lib.rs).

1. **Two writers, one corpus, three entry points.** The form-editor path is
   `apply_body_change` (a native delta + line/mark bundle the editor already
   computed); the stale-text path is `import_body_delta` (cold import +
   `diff_import`), with `replace_body` its delta-discarding wrapper.
   `set_body_corpus` is the raw native set for a corpus decoded elsewhere.
2. **`diff_import` now wired to `Document`.** `import_body_delta` is the seam:
   it rebases surviving identity anchors onto the fresh import (the old
   fresh-`import_body` `replace_body` dropped them) and returns the text delta,
   so the whole-document replace records into the change log exactly like a
   field edit. The session still owns the log — the mutator returns the delta;
   the caller records it (`record_field_delta` / `record_field_change`). Card
   holds no session reference.
3. **Silent degradation retired.** `replace_body` was infallible, degrading an
   over-nested body to the empty corpus; it now returns
   `EditError::BodyImport`. `apply_body_change` surfaces an out-of-bounds op as
   `EditError::CorpusApply` (wrapping `ApplyError`). Both new variants ride the
   existing `variant_name` → `[EditError::<Variant>]` binding contract, so
   wasm/python surface them without a mapper change; the wasm `replaceBody` /
   `updateCardBody` and python `replace_body` / `update_card_body` wrappers now
   propagate instead of swallowing.

## PR-F — preview wire + revision stamp findings

**Implementation:** [`region.rs`](../../crates/core/src/region.rs),
[`session.rs`](../../crates/core/src/session.rs).

1. **Stamp at the wrapper, not the backend.** `revision` is an
   additive-optional field on `RenderedRegion` and `CorpusHit`
   (`skip_serializing_if = None`, so a phase-2 wire with no `revision` key still
   reads back). Backends construct with `revision: None`; the
   [`LiveSession`](../../crates/core/src/session.rs) wrapper overwrites it with
   the current `revision()` on `regions` / `locate` / `position_at`. No backend
   learns about revisions, and the one-shot `RenderOptions::regions` sidecar
   stays `None` (no session).
2. **`field_at` is not stamped.** It returns a bare field address, which is
   revision-invariant — only a *position* drifts. Stamping it would have meant
   changing its return shape (a break), against the freeze; the fine-grained
   `position_at` carries the stamp instead.
3. **Base-revision guard, three surfaces.** `ensure_base_revision` is the raw
   check (`base == current`, else a `session::revision_mismatch` diagnostic);
   `record_field_delta_at` / `record_field_change_at` fold guard-then-record for
   callers where record *is* commit. A caller that interleaves a document
   mutation and a recompile (PR-G) uses `ensure_base_revision` up front and the
   plain `record_field_delta` last, so the log advances only on a fully
   committed edit.

## PR-G — WASM session API findings

**Implementation:** [`engine.rs`](../../crates/bindings/wasm/src/engine.rs),
[`types.rs`](../../crates/bindings/wasm/src/types.rs);
[`canvas.test.js`](../../crates/bindings/wasm/canvas.test.js).

1. **`applyFieldDelta` ties Document + session in one call.** It guards the base
   revision (untouched on mismatch), splices the main body via
   `apply_body_change` (text delta only), recompiles from the mutated document,
   and records the delta into the log **last** — so `revision` advances in
   lockstep with the live preview and a failed recompile leaves the log where it
   was. The `field` argument is `$body` this phase (the only structurally-stored
   corpus; scalar `richtext` fields still coerce from strings at `compile_data`);
   any other address throws, pointing the caller at `apply(doc)`.
2. **Delta wire shape.** `Delta = { ops: ({retain:n}|{insert:s}|{delete:n})[] }`
   — CodeMirror-flavoured, externally-tagged with `camelCase` keys, converted to
   `quillmark_core::Delta` at the boundary (`Op` newly re-exported from core). A
   pure-insert delta (`{ops:[{insert:"…"}]}`) needs no length knowledge — apply
   appends the untouched remainder — which the tests exploit.
3. **`revision` narrows u64 → u32 at the boundary** to stay a JS `number`
   (parity with `pageCount`), and `mapFieldPos(field, base, pos, "before"|"after")`
   exposes the forward map, throwing `session::stale_revision` past the ring.
   `FieldRegion.revision` / `CorpusHit.revision` are optional `u32`.
4. **Whole-doc `apply(doc)` stays revision-neutral.** It recompiles from a
   fully-formed document and records nothing (it holds no prior body to diff);
   the markdown/LLM path re-reads geometry rather than mapping positions
   forward. The stale-text writer that *wants* monotonic mapping records via
   `Document::import_body_delta` + an explicit record at the caller (per PR-E),
   not through `apply(doc)`.
5. **Canonical `runtime.js` wrapper unchanged.** It already forwards only a
   curated subset (no `positionAt` / `locate`), and its cross-wasm-memory
   `apply` bridge (`mod.Document.fromJson(doc.toJson())`) does not compose with
   an in-place *mutating* `applyFieldDelta`. The phase-3 nav/edit surface is
   consumed off the backend-build `LiveSession` directly (as PR-H's spike does);
   forwarding it through the canonical layer is a later pass.

## Sequencing invariant

- Spike-A reports before PR-B freezes change-log entry shape (text delta only
  until PR-D, but the log slot must accommodate mark/line ops). **Closed.**
- PR-B (minimal diff) before PR-C (log stores deltas worth replaying). **Closed.**
- PR-D (mark/line ops) before PR-E (mutators emit them). **Both landed.**
- PR-F (revision stamp) before PR-G (WASM exposes revision-aware apply).
  **Both landed.**
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
   block-capable editors can use `LineOp` split/join (landed PR-D); block canvas
   remains out of scope.
3. **Coexistence with whole-doc `apply`.** Markdown/MCP path stays
   `diff_import` at document granularity; session must apply stale-text and
   field-delta edits in a defined order (revision monotonicity across both).

## Related

- #831 (this rework), #829 (regions — phase 2)
- [INDEX.md](INDEX.md), [phase-0.md](phase-0.md), [phase-1.md](phase-1.md),
  [phase-2.md](phase-2.md)
- Branch `spike/richtext-phase-3` (origin) — runnable PR-A + PR-H probes
- `prose/canon/PREVIEW.md`, `DOCUMENT_STORAGE.md`, `SCHEMAS.md`
- `crates/richtext/src/delta.rs`, `crates/richtext/src/change_log.rs`,
  `crates/richtext/src/ops.rs`, `crates/core/src/document/edit.rs`,
  `crates/core/src/region.rs`
