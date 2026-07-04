# Content model alternatives — block tree vs. text corpus with embedded blocks

Companion to [issue-830-grounding.md](issue-830-grounding.md). Compares #830's
canonical block tree against the alternative it displaces without naming: a
**text corpus with embedded blocks** — content as a linear text sequence
carrying anchored marks, with structured objects (tables, figures, future
embeds) as identified islands *in* the sequence. Pre-1.0 posture assumed: hard
cutovers are on the table, so this weighs model fit and risk, not migration
politeness.

## 1. The two shapes

**Block tree (#830).** Content = flat sequence of `{id, type, props}` block
objects; text lives *inside* blocks as runs with marks; identity = block IDs,
minted once, preserved by every transformation. Prior art: Notion,
ProseMirror/Lexical document models, Yjs `XmlFragment`.

**Text corpus with embedded blocks.** Content = one text sequence per content
field; paragraph/heading/list structure rides on boundary markers in the
sequence (block attributes on the newline, the Quill-Delta/Google-Docs
encoding) or is derived by parse; inline formatting = marks anchored to
sequence ranges; anything with no honest text encoding is an island object
embedded at a position, carrying its own `$id`-style identity. Identity =
sequence positions stabilized by anchors (op-rebase or diff), island IDs for
islands. Prior art: Google Docs, Quill Delta, Word's run model, CodeMirror
decorations, and — decisively — Peritext and Automerge's rich-text design.

Quillmark already *is* the second shape one level up: a `.qmd` file is a
markdown corpus with embedded card-yaml islands, and cards carry `$id`. The
corpus model extends the existing architecture downward into bodies; the block
tree replaces the body model wholesale.

## 2. A load-bearing correction to #830's collab premise

#830 claims marks-over-runs-per-block is "the merge-correct (Peritext) shape."
It is not. Peritext defines marks over a **contiguous text sequence**;
Automerge's rich-text extension deliberately embeds block markers *inside* the
text sequence rather than splitting text across block objects — i.e. the
research #830 cites converged on the **corpus** shape. The reason is the
**split/join problem**, and it lands directly on #830's ID discipline:

- User splits a paragraph (the single most common structural edit in prose:
  press Enter). In a block-object model, half the text moves to a *new* block
  — which half keeps the ID is undefined in #830, and every region, comment,
  or anchor on the other half dangles. Join (press Backspace at a boundary)
  kills an ID. Concurrent edits in the moved half must be migrated across
  object boundaries by special-case machinery.
- In a corpus model, split is an inserted boundary character and join is a
  deleted one. Text identity is untouched; anchors and concurrent edits merge
  by ordinary sequence semantics.

Block IDs are stable under the edits block models are designed for (drag a
block somewhere else) and unstable under the edits *prose* actually receives
(split, join, merge-forward). Quillmark's content fields are memo and letter
bodies — prose. The instability #830 attributes to byte ranges reappears, for
the common case, as block-ID churn.

Where the tree genuinely wins merge-wise: **moves**. Sequence CRDTs have no
move primitive (move = delete + reinsert; concurrent edits in the moved range
are lost or duplicated); flat-with-parent-refs makes reorder a reference
update. If drag-reorder of blocks is a primary interaction, that matters. In
typeset memo prose, it is a rare edit.

## 3. Consumer alignment — who writes content today

The consumers evidenced in-tree are text-shaped:

- The MCP/LLM flow authors **whole markdown documents** against the blueprint
  (BLUEPRINT.md; TongueToQuill's create/update take markdown built from the
  blueprint). LLMs emit text.
- The web-app integration passes (#782, #801) drive `LiveSession` with
  snapshot `apply`; no block editor exists in-tree.

Under the block model, every one of these snapshot/text writers must pass
through the **ID-anchored matching importer** — #830's most novel, most
heuristic machinery — and until the phase-3 editor exists, *nothing* preserves
IDs across edits, so the matcher (or cold re-mint) sits in the hot loop of
every keystroke and every MCP update. Under the corpus model the equivalent
operation is a **character diff**: mature, deterministic, and exactly how
editors already rebase decorations across text edits. The riskiest component
of #830 dissolves into `diff-match-patch`.

Same asymmetry at the storage seam: card bodies are *already* markdown
corpora (`dto.rs`: `body: String` in every schema version). Corpus migration
is near-identity — no paragraph IDs exist to mint, islands reuse the plumbed
`$id` pattern — so the generative-migration/determinism problem
(grounding §4.1) does not arise. Pre-1.0 hard cutover softens that problem
for the tree (one-shot rewrite instead of read-chain minting) but does not
remove the matcher from the loop or the split/join churn from the model.

## 4. Axis-by-axis

| Axis | Block tree | Corpus + islands |
|---|---|---|
| Prose edits (split/join/type) | ID churn, undefined split policy | stable by construction |
| Block move / drag-reorder | reference update, clean | delete+reinsert, weak |
| Deep nesting (toggles, columns) | native | markers strain past ~list depth |
| Snapshot writers (LLM, textarea, MCP) | heuristic block matcher | character diff |
| Op-based editor binding | ProseMirror/Lexical native | Quill/Docs-proven; PM bindable |
| Collab substrate | tree CRDTs (younger; split/join warts) | text CRDT + Peritext marks (most mature CRDT class) |
| Typst emit | tree walk | parse-at-emit (status quo, cheap) |
| Regions key | `(field, block id)` | `(field, anchored range)` — #829's shape, hardened |
| Regions run-machine rework (grounding §3.2) | required | required — not a differentiator |
| Storage migration | first generative migration | near-identity |
| Unknown-block passthrough | open union | islands are opaque by construction |
| Markdown projection | lossy (IDs), matcher inverse | near-lossless for text; loss confined to islands |
| Table/figure growth | new block types | new island types — equivalent |

The target medium is a signal too: Quillmark renders linear typeset documents
through Typst, whose own model is markup text. Word, Docs, LaTeX, Typst — the
typeset-prose world is corpus-shaped; Notion, Coda, Craft — the app-canvas
world is block-shaped. Quillmark's content fields live in the first world.

## 5. The honest costs of the corpus model

- **Anchors need an op or a diff.** Mark ranges over a mutable sequence are
  fragile *unless* the engine sees edits as operations or diffs snapshots
  itself. But #830 pays the same bill — its intent vocabulary ("replace text
  range") is the ops surface, and block IDs survive snapshots only when a
  cooperating editor preserves them. Both models require the same new edit
  surface; they differ in what re-anchoring costs when a writer bypasses it
  (char diff vs. block matcher).
- **List nesting via boundary attributes is uglier than a tree.** Indent
  levels on markers (the Delta/Docs encoding) are a known wart; the Typst
  emit for nested lists is less direct than a tree walk. pulldown-cmark's
  nested events flatten into this encoding mechanically, but it is the
  corpus model's least elegant corner.
- **Two identity mechanisms** (anchors for text, IDs for islands) versus the
  tree's one. Mitigated by the fact that the island mechanism already exists
  and is spec'd (`$id` on cards).

## 6. The staged path the corpus model unlocks

The corpus model decomposes into independently shippable steps; the tree does
not (grounding §7's phase-1 cliff).

1. **Now, cheap:** markdown text stays the canonical content encoding. Land
   #829's sub-window regions keyed on `(field, source range)` — dozens of
   lines. Add engine-side char-diff on `apply` if cross-compile anchor
   stability is wanted early. Define the island syntax (a fenced block inside
   bodies, the card-yaml move repeated) for the first non-markdown block type
   when one actually ships.
2. **When a structured editor is real:** lift inline formatting out of
   markdown syntax into typed anchored marks over the same corpus (the
   Peritext shape), killing the fixer and the markup-escape concerns exactly
   as #830 promises — same benefit, same seam, no model replacement. Corpus
   coordinates from step 1 carry forward as anchors.
3. **When collab is real:** bind the corpus to a text CRDT (Loro/Automerge/
   yrs text + marks), islands as atomic embeds. No retrofit: the model is
   already the shape those CRDTs serialize.

Step 1 is throwaway-free under steps 2–3 — the coordinates and the island
mechanism persist. #830's phase 1 must be right about editor-binding
semantics *before* any editor exists; the corpus path defers exactly those
freezes until the consumer that constrains them arrives.

## 7. Verdict

For what Quillmark is — schema-driven typeset prose whose flagship writers
are LLMs and snapshot-based editors — **the corpus model is the stronger
default**, and #830's own collab citation (Peritext) points at it. It wins on
prose-edit identity, snapshot-writer robustness (diff vs. matcher), migration
risk, collab-substrate maturity, and continuity with the document-level
architecture (corpus + `$id` islands) and the Typst target. The block tree is
the right call only under a specific product bet: a Notion-class block editor
as the primary authoring surface, with drag-reorder and deep nesting as core
interactions — in which case its move semantics and tree-native editor
bindings pay for the matcher, the split/join policy burden, and the
first-generative migration.

Pre-1.0 licenses a hard cutover in either direction; it does not change which
direction fits. Decisiveness is not step size. The gate remains the phase-0
editor spike (grounding §7), now with a sharper question: bind a
ProseMirror/Lexical *and* a CodeMirror-style surface against both model
shapes and observe where the impedance actually is — especially what each
does to identity on split/join, and what each costs the MCP full-document
rewrite flow.
