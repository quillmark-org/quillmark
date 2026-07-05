# Phase 1 — type + codecs, engine-off

`RichText` and its markdown projection, exercised in isolation. The freeze — the
canonical serialization and mark semantics storage commits to — lands here,
gated on the [Spike-A contract](phase-0.md). No engine crate consumes it yet;
Phase 2 wires the seam and storage.

**Status: landed.** Crate `crates/richtext` (`quillmark-richtext`), a workspace
member held outside `default-members` (so `cargo test --workspace` runs it while
the release/publish set skips it), `publish = false`. 74 unit + 8 property /
fixture tests; the property suite runs thousands of randomized cases per
property across seeds. Clippy clean.

---

## What shipped

One crate, six modules, layered on `model.rs` with no cycles:

- **`model.rs`** — `RichText { text, lines, marks, islands }` over a USV
  coordinate space, plus `Line` / `LineKind` / `Container` / `Mark` / `MarkKind`
  / `Island` / `Loss`. Owns `normalize()` (the freeze's semantics) and
  `validate()` (the invariants).
- **`serial.rs`** — canonical, byte-deterministic JSON. One encoding for the
  seam and for storage.
- **`import.rs`** — `from_markdown`: `normalize → pulldown → corpus`, with the
  `<u>` allowlist and `***` fixups ported from the typst backend.
- **`export.rs`** — `to_markdown`: corpus → markdown per island loss class.
- **`delta.rs`** — `Delta` (`retain`/`insert`/`delete` text splices, CodeMirror
  `ChangeSet` semantics — not attributed Quill-Delta; marks rebase *through* a
  delta, not as op attributes), `diff`, `map_pos`, and `diff_import` (the
  stale-text writer: cold-parse + anchor rebase with a block-move detector).
- **`usv.rs`** — char / UTF-8 / UTF-16 conversions.

Depends **down** on `quillmark-core` (reuses `normalize::normalize_markdown`) and
on `pulldown-cmark`. The markdown engine stays **out of `core`** (canon: core has
no markdown-engine dependency in production) — that is why the codecs live in
this crate, not in `core`.

## Locked decisions

### Placement — a new crate, not a `core` module

The parser-dependent codecs cannot live in `core` without breaking its
markdown-engine-free invariant. Phase 1 is maximally additive: a new crate that
touches no existing code. Phase 2 re-homes the *type* (`RichText` + canonical
serialization) into `core` for storage while leaving the codecs parser-side. The
freeze is the **bytes**, not the module path, so re-homing is mechanical.

### The freeze — what storage commits to

- **Shape.** Canonical JSON with **every object key recursively sorted**, so the
  bytes are independent of `serde_json`'s `preserve_order` feature. Pinned by a
  golden-bytes test; changing it bumps the schema version.
- **Mark set** (open): `strong · emph · underline · strike · code · link{url}`
  (formatting) + `anchor{id}` (zero-width identity) + `unknown{tag, attrs}`
  (round-trips opaque). Unknown may not reuse a reserved built-in `type` name
  (validated — else serialization is non-injective).
- **Normalization** (idempotent, the fixed point): same-kind formatting union on
  adjacent/overlap; different kinds overlap freely; identity never merges; sort
  by `(start, end, kind-ord, attrs)`; **formatting-mark edges never sit on a
  `\n`** (trimmed); island `props` / unknown `attrs` recursively key-sorted.
- **Coordinates** count Unicode scalar values throughout.
- **`Line.continues`** — a within-block hard line break (markdown hard break;
  code-fence continuation) distinct from a paragraph boundary. This is a **model
  shape** decision and therefore a one-way door: it exists now so Phase-2 emit
  keeps a `#linebreak()` distinct from paragraph spacing.
- **`ListItem { ordered, start, ordinal }`** — positional item identity, no
  minted ids. A multi-paragraph item is two lines sharing one `ListItem`.
- **Islands** — tables are block islands (own `Island` line), images inline
  slots, both `Lossless`. Ids minted **sequentially** (`isl-N`) so import is a
  pure function; real minting (the hash-nondeterminism source) is Phase 4.

## Independent review outcome

Phase 1 was built **from the written contract**, without the Phase-0 spike code
(the `crates/richtext-spikes/` crate lives on `claude/issue-831-phase-0-tbjjm1`,
not on `main`/`integration` — the `phase-0.md` references to it are to that
branch). An independent review (Fable) then cross-checked the build against that
spike code. It found the freeze not yet correct; every finding is fixed and
pinned:

- **Marks swallowed a block's leading `\n`** — import vs an editor-built corpus
  of the same content serialized to *different* bytes. Fixed (mark starts
  resolve the pending line; edges trim off `\n`); a test asserts import and a
  hand-built corpus now emit identical bytes.
- **Bytes depended on the `preserve_order` feature** — now whole-tree sorted.
- **Hard-break collapse** — now modelled by `Line.continues`.
- **Anchor relocation re-homed onto unrelated surviving text** — now restricted
  to *inserted* regions with a length floor.
- Leading ordered-marker escaping; `map_pos` `Assoc::After`; `is_deleted` left
  edge; table pipe double-escaping; validate-on-parse; loss-class default.

The broadened property suite (the review's other must-fix) then caught four more
real bugs: emphasis exported as `_` (can't do intraword emphasis; now `*`), and
empty heading / empty item / empty quote silently dropped on import (now each
keeps its line). **The two independent derivations of the contract converged** —
the crate is better than the spike on model shape (positional `ordinal`, real
islands, open mark set); the spike was better on whole-tree sort and
deleted-vs-inserted move detection, both now adopted.

## Documented limits (recorded, not hidden)

- **Degenerate, non-authorable corpora** don't round-trip: a mark *spanning* a
  hard break (splits per line), an empty first line in a hard-break block
  (markdown has no blank-then-forced-break). Pinned by a `known_hard_break_limits`
  test.
- **Coarse diff** (prefix/suffix trim): anchor *survival* holds, but the `Delta`
  is not a minimal edit script. Phase 3 brings a Myers/LCS diff.
- **Canonicalizations** (distinct markdown → one corpus): hard break → `continues`
  line (heading hard break → space); adjacent sibling same-shape lists merge;
  adjacent blockquotes merge; empty blocks/containers keep one empty line;
  sequential island ids. All listed in `import.rs`'s module doc.

## Handover to Phase 2

Phase 2 makes the engine consume `RichText` and delivers #829. The authority for
Phase-2 decisions is now [phase-2.md](phase-2.md); items 1 and 4 below are
**revised** there. Carry-forward, in priority order:

1. **Type home — revised.** phase-2.md keeps `quillmark-richtext` a **separate
   leaf crate** that `core` **depends on** (arrow inverted), rather than dissolving
   `RichText`/`serial` into `core`: the model + frozen wire format sit one layer
   below the engine, and the codecs stay with them. The circularity that blocked a
   leaf crate is removed by relocating `normalize_markdown` into it (landed).
   Canonical bytes are unchanged.
2. **Seam (Option A).** Carry structured `RichText`-JSON across
   `Backend::open(source, json_data)` — never a markdown string. The canonical
   serializer is ready; `pdfform` lowers via `RichText.text` minus island slots
   (zero fixture churn — `sample_form` binds no content field).
3. **Typst emit + source map.** Walk lines → markup (escaping as today),
   recording per-line generated windows plus one `(corpus range ↔ generated
   range)` pair per run. Spike-B carry: character precision = add `glyph.span.1`
   (unused today at `span_scan.rs:197`) to the resolved node range, then invert
   the run map (cluster-exact, invertible by recomputation).
4. **Unify the `MarkdownFixer`.** It is **duplicated** in phase 1 (typst backend
   copy + `richtext::import` copy) to stay additive. Phase 2 collapses the two.
5. **`locate` / `position_at` + region re-key** on `(field, corpus range,
   revision)`; the `regions()` run-machine rework for highlight boxes
   (grounding §3.2) is Phase-2 work, out of scope for navigation.
6. **Storage cutover** — new `StoredDocument` version; migration is a
   deterministic cold import (legacy bodies hold no islands → mint-free).
7. **Block-quote emit decision.** RichText captures block quotes as
   `Container::Quote`, but the current `mark_to_typst` has no `BlockQuote` arm —
   it **flattens** quotes to plain paragraphs. So the superset is real: Phase-2
   emit must *choose* to render quotes (a deliberate behavior change) or keep
   flattening for bug-for-bug parity. Recommend rendering them, since the
   structure is now first-class; land it as an explicit, tested decision, not a
   silent consequence of consuming the corpus.

Already done here, so Phase 2 inherits it: **island `props` keys are recursively
sorted before serialization** (Spike-C carry — otherwise `preserve_order` leaks
insertion order into the content hash). The only remaining determinism boundary
is island-id mint, which does not appear until Phase 4.

**Residual Spike-A gate still stands** (Phase 3, not Phase 2): bind one live rich
editor and confirm none forces an edge-expand / adjacent-merge semantic back into
the model. Phase 1 froze only what the contract commits to — the mark set, the
three rules, USV, `continues` — and delegates every disputed typing-time semantic
to the editor, so the freeze is editor-independent by construction; the live
binding is confirmation, not a reopening.

## Verification

- Unit tests per module (model normalization/invariants, serial fixed-point +
  golden bytes, import/export round-trips, delta rebase/move).
- Property suite (`tests/properties.rs`): corpus fixed point + invariants;
  canonical determinism + order-insensitivity; anchor survival; USV boundary /
  surrogate correctness. Generator spans special chars, hard breaks, nested
  containers, astral chars.
- Fixture corpus: `sample.md` and other resource bodies import to valid corpora
  and are fixed points.
