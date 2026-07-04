# Richtext corpus model — greenfield design

Greenfield design for the content-field model, assuming the settled product
vision: **a web form with rich prose fields is the primary authoring surface**;
the LLM/MCP whole-document markdown flow and human `.qmd` files are co-equal
writers; a Notion-class block canvas is a non-goal. Successor position to
#830's block tree; context in
[`../review/issue-830-grounding.md`](../review/issue-830-grounding.md) and
[`../review/issue-830-content-model-alternatives.md`](../review/issue-830-content-model-alternatives.md).

## Shape

One `RichText` value per richtext field — today's "content fields," card bodies
included; the document level keeps its existing
corpus-with-`$id`-card-islands shape. A `RichText` is four aligned pieces over
**one coordinate space**:

```
RichText {
  text:    String        // the corpus; '\n' = line boundary, U+FFFC = island slot
  lines:   Vec<LineAttrs> // one per '\n', in order: what each line is
  marks:   Vec<Mark>      // formatting + annotations over char ranges
  islands: Vec<Island>    // one per U+FFFC, in order: structured objects
}
```

The single coordinate space is the load-bearing decision, taken from the
Delta/Docs/Automerge lineage: boundaries and islands occupy character
positions *in* the text, so every edit is a splice and all structure moves
with it. There is no second addressing scheme to keep synchronized.

### Coordinates

Positions and ranges count **Unicode scalar values**. Pinned in the wire
contract because the bindings disagree by default (JS editors are UTF-16,
Rust is UTF-8); each binding converts at its boundary. Invariants: no `\r`,
no bidi control characters, no U+FFFC except island slots — enforced at every
codec, established once by import normalization.

### Lines

`LineAttrs { kind, containers }`. `kind` names what the line is: `para`,
`heading{level}`, `code{lang}`, `island`. `containers` is the ancestor path —
`[ListItem{ordered, start?}, Quote, …]` — the Automerge block-marker encoding
rather than a bare indent integer, so a multi-paragraph list item is two
`para` lines sharing a `ListItem` container. The container tree is *derived*,
never stored as a tree: split and join are one-character edits whose new line
inherits its neighbor's attrs. Paragraph identity survives Enter and
Backspace because paragraphs are not objects.

### Marks

`Mark { range, type, attrs }`. Known types: `strong`, `emph`, `underline`,
`strike`, `code`, `link{url}`. The type set is open: unknown mark types
round-trip opaque and render as plain text. Same-type overlaps normalize by
union; different types overlap freely. Canonical order `(start, end, type)`.

Marks are also the identity mechanism: **identity is a mark, not a block
property.** A comment, a suggestion, a review thread, a stable permalink
anchor — each is a mark (possibly zero-width) with an `id` in its attrs. Marks
rebase across edits like all positions, so identity costs nothing on
split/join and exists only where something actually points at the text. There
are no paragraph IDs.

### Islands

`Island { id, type, props: QuillValue, loss }`. Structured objects with no
honest prose encoding: tables, figures, page breaks, future embeds. `id` is
opaque, minted at creation — the `$id` pattern one level down. `type` is open;
unknown islands round-trip opaque and render as a placeholder. `loss` declares
the markdown projection class: `lossless` (table ⇄ pipe table), `degraded`
(figure → image + caption), `unrepresentable` (placeholder on export,
preserved on import). Tables are islands, not line runs — they are grid data
with typed cells (`array<object>`-shaped props), edited by a grid widget in
the form, not by prose gestures. An island is one character wide; a block
island is a line of `kind: island`, an inline island sits mid-line.

## Revision and the edit surface

The document carries a **monotonic revision**, bumped per applied edit batch.
Every session read (`regions`, `field_at`) is tagged with the revision it was
computed against.

The single edit language is a **delta** per field — `retain(n)` /
`insert(text | island, marks?, line_attrs?)` / `delete(n)`, Quill-Delta
semantics — submitted against a stated base revision. Every writer reduces to
it:

- The **form editor** emits deltas natively (Quill) or via a thin translation
  (ProseMirror/Lexical transactions map 1:1 onto splice + mark ops).
- A **stale text writer** (MCP `update_document`, a saved `.qmd`) is handled
  by parsing its markdown to a corpus and taking a **character diff** against
  the current corpus — island slots are atomic, id-keyed tokens the diff
  cannot confuse or textually delete — yielding a delta like any other.
  There is no block matcher and no mint policy; marks and islands rebase
  across the diff mechanically, so annotations survive an LLM full-document
  rewrite without any preservation contract on the LLM.
- **Collab, later**: a text CRDT (Automerge rich text, Loro, yrs) lives at
  the sync layer and materializes the same delta stream. The model is
  deliberately isomorphic to Automerge's rich-text shape (sequence + block
  info at boundaries + range marks + embeds), so the binding is mechanical
  and the engine stays CRDT-free.

The engine keeps a bounded **change log** of applied deltas and exposes
position mapping across revisions (the CodeMirror `ChangeDesc` mechanism).
That is what replaces stable block IDs for the preview loop: a click resolved
against the compile at revision N maps through the log to coordinates at the
editor's revision. Beyond the log's window, consumers re-anchor by diff.

## Codecs

- **Markdown import (cold)**: normalize (CRLF, bidi strip, comment-fence
  repair) → pulldown parse → corpus. The `<u>` allowlist and `***` adjacency
  fixups run here, once, at the boundary. Deterministic; no randomness for
  text (island minting occurs only when islands are authored, which markdown
  cold import does not produce today).
- **Markdown import (against a base)**: cold parse + corpus diff, above.
- **Markdown export**: text is text; marks split at line boundaries and emit
  syntax (`<u>` for underline); islands emit per `loss` class; annotation
  marks are omitted (they are not content) and survive the round trip via
  diff-rebase, not via the projection.
- **Typst emit**: walk lines grouped by container path → markup, escaping
  text runs as today; record per-line generated windows plus a **source
  map** — one `(corpus range ↔ generated range)` pair per emitted text run.
  Corpus runs are syntax-free, so within a run the escape transform is
  deterministic and invertible by recomputation: the map is run-aligned and
  character-exact with no per-character tables.

Two preview queries ride on the emit, with very different machinery costs:

- **Navigation — character-exact, no run machine.** Every drawn glyph
  carries `span: (Span, u16)`: source node *plus byte offset within it*
  (typst 0.15 `Glyph`; `Glyph::range` disambiguates within ligatures). The
  scan currently uses only the node (`span_scan.rs:197`); the offset is
  unused headroom. Point → corpus char: hit-test the glyph (the `field_at`
  walk), resolve node range + offset to a generated byte, invert the source
  map. Corpus char → page point: forward-map to a generated byte, find the
  covering glyph among the page hits (per-glyph boxes are already computed),
  return its page and box. Granularity is the shaping cluster — the same
  resolution every real caret has. Ink with no corpus origin (list markers,
  plate decorations, package-generated numbering) is ignored for navigation:
  `position_at` answers only for glyphs that map to a corpus character.
- **Highlight boxes (paragraph regions) — leaf line windows only.** The
  single-cursor run machine keeps its existing disjoint-leaf invariant;
  marker ink goes unclassified (accepted); field boxes derive by unioning
  their lines'. The hierarchy-tolerant scan rework (grounding §3.2) is
  needed only to *attribute* marker glyphs — optional polish, off the
  critical path.

Regions key on `(field, corpus range, revision)`; a `locate(field,
corpus_pos) → (page, rect)` / `position_at(page, x, y) → (field,
corpus_pos)` pair exposes navigation. Paragraph navigation — #829's goal —
is the line-window degenerate case of the same map.

## Storage

`RichText` serializes canonically: text verbatim, `lines`/`islands`
positionally aligned with their sentinels (validated on read), marks in
canonical order. Byte-deterministic — no randomness exists outside island
creation, so content hashing (DOCUMENT_STORAGE.md) holds. Migration from
markdown-string bodies is a cold import: deterministic, mint-free (legacy
documents contain no islands), and pre-1.0 a one-shot cutover. The
generative-migration problem the block tree carries (grounding §4.1) does not
arise.

## Schema surface

Field type `richtext` (rename from `markdown`; pre-1.0 hard cutover), lowering
to a `$ref` of the corpus schema. `richtext(inline)` = no `\n`, no block islands
— a validation rule, not a sibling type. The blueprint inline annotation
carries the authoring-surface encoding in the existing format slot —
`bio: !must_fill # richtext<markdown>` — so the type names the role and the
refinement tells the writer what syntax this surface accepts. `default:`,
`example:`, `body.example`, and `$seed.<kind>.$body` stay authored
**markdown**, imported at `Quill` load and cached; since text needs no
minting, defaulted richtext is stable across compiles.

## Naming

`richtext` names the type at every author-facing surface (`type: richtext`,
`array<richtext>`, `richtext(inline)`, blueprint `# richtext<markdown>`) and
in code (Rust/bindings `RichText`); **corpus** stays the internal shape term
(the `text` sequence, "corpus coordinates" in module docs). The one-word
collapse follows the `datetime` precedent, and the rich-text tradition has
included lists, tables, and embedded objects for decades, so the name covers
the full block set. The token is lexically unique — `richtext` is not an
English word, so the term of art never collides with ordinary usage in
running prose, greps precisely, and `RichText` self-describes in code with
no collision in Rust, JS, or Python (none with `typst::foundations::Content`).
The blueprint's format slot carries the surface encoding
(`# richtext<markdown>`): the type names the role, the refinement tells the
writer what syntax this surface accepts. The rename sweeps canon once
("content fields" → "richtext fields").

Rejected alternatives, with the criterion each failed:

- `markdown` — names the encoding of one projection surface, not the role.
- `content` — ambiguous and saturated; near-zero signal to authors or LLMs;
  permanent `typst::foundations::Content` collision in the backend.
- `prose` — implies flowing text, underselling bullets, tables, islands.
- `corpus` — names the implementation shape, and its outsider prior is a
  *collection* of documents; kept as internal vocabulary only.
- `rich` — bare adjective still alive in the ordinary tech-writing register
  ("rich diagnostics"), so the term of art and the sell collide in running
  prose; the compound removes the ambiguity for one syllable.

## What this kills, keeps, and adds

**Kills** relative to today: markdown re-parse per `apply`; the escape/fixer
concerns on the editor path (the form editor edits the model, never markdown
syntax); `#829`-style raw byte fragility (ranges become revision-mapped).
**Kills** relative to #830: the ID-anchored matching importer, block-ID mint
and uniqueness policy, the split/join ID question, the generative migration.

**Keeps**: the import stack (normalize + pulldown + fixer) at the boundary,
forever — markdown stays first-class; `escape_markup` on emit; the
canonical-codegen and span-excluding `page_hashes` guarantees (#801), since
nothing random enters generated source.

**Adds** (the honest new machinery): the delta type and per-field apply path;
the revision counter and bounded change log with position mapping; the
corpus-diff importer; island infrastructure when the first island type ships
— each individually small and each with decades of prior art, versus one
novel matcher.

**Weak spots, stated**: a *moved* paragraph is delete + insert to the diff,
so annotations on text that a stale writer moved do not follow it (move
detection in the differ is a heuristic patch, same class of heuristic the
block matcher would have been — but confined to move-plus-annotate
concurrency instead of sitting on every import). Deep custom nesting beyond
containers-path expressiveness would force island-ification. Mark expand
semantics at range edges (does typing at a bold edge extend it?) are an
editor/CRDT concern the model deliberately does not encode; the editor spike
must confirm each candidate binding can own them.

## Sequencing

1. `RichText` type + canonical serialization + markdown⇄corpus codecs +
   invariant/property suite (round-trip modulo loss class; diff-import
   preserves marks/islands). Engine-off, exercised against the fixture
   corpus.
2. Engine consumes `RichText`; Typst emit with line windows + run source map;
   navigation queries (`locate` / `position_at`) and regions re-key; storage
   cutover.
3. Delta edit surface + revision/change-log mapping; form editor binding
   (the spike from grounding §7 gates the mark-semantics freeze).
4. Islands (first real type: tables), then the collab binding when wanted.

Step 1 freezes nothing an editor hasn't validated; step 2 delivers #829's
feature; nothing in any step is discarded by a later one.
