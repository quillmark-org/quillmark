# Proposal — Move the Leaf Kind Discriminator into the Info String

> **Status**: Implemented
> **See also**: [MARKDOWN.md](../designs/MARKDOWN.md) (spec), [LEAF_REWORK.md](LEAF_REWORK.md) (current design), [LEAVES.md](../designs/LEAVES.md) (data model)
> **Amends**: LEAF_REWORK.md §3.2, §5; MARKDOWN.md §2, §3.2, §4.2, §4.4

## 1. Summary

Today a leaf record is opened with the bare info string `` ```leaf `` and its
type is carried by a mandatory **first body key**, `KIND:`. This proposal
moves the discriminator into the info string itself:

```text
  before:  ```leaf            after:  ```leaf indorsement
           KIND: indorsement           for: ORG/SYMBOL
           for: ORG/SYMBOL              from: ORG/SYMBOL
           from: ORG/SYMBOL
```

The first info-string token stays `leaf` (classification is unchanged and
still purely lexical). The **second token names the kind**. The `KIND:` body
key is removed as an input key.

## 2. Motivation

The current design (LEAF_REWORK.md §3.2) deliberately rejected this form. Two
arguments carried that decision; both are weaker than they first appear, and
one new argument now outweighs them.

**The symmetry argument was oversold.** "The first body key names the fence's
role" reads as a unifying rule, but LEAF_REWORK.md §3.4 already concedes the
unification is shallow: fence *shape* differs (`---/---` vs `` ```leaf ``) and
discriminator *semantics* differ (`QUILL:` is a template binding; `KIND:` is a
record-type tag). The shared rule is a one-sentence mnemonic, not a structural
invariant — weak load-bearing for a syntax decision.

**LLM-authoring robustness is the decisive factor.** Quillmark documents are
authored by LLMs and by hand. The current implementation is **not yet widely
deployed**, so the change is cheap now and only gets more expensive later.

- A's failure mode is **subtle and likely**: an LLM emits `` ```leaf ``, then
  writes body keys in whatever order reads naturally, and `KIND:` lands
  *second*. That is a hard error. LLMs reorder YAML mapping keys freely —
  there is no strong prior that "first key" is load-bearing.
- B's failure mode is **loud and rare**: omit the token immediately after
  `` ```leaf ``. A model that just wrote `` ```leaf `` is far less likely to
  drop the very next token than to misorder a key three lines down.
- B **co-locates the decision and the commitment**: the model commits to
  "this is an indorsement" as it opens the fence, and generates the body
  conditioned on that token sitting in-context.
- B **rhymes with pretraining**: `` ```lang `` with the language on the info
  string is the most common fenced-block shape in the corpus. `` ```leaf ``
  plus a reserved all-caps first body key is a shape the model has effectively
  never seen.

The cost we knowingly accept (see §7) is that the discriminator leaves YAML's
semantic regime and that frontmatter/leaf symmetry is reduced to zero. We
judge LLM-authoring correctness to be worth more than both.

## 3. Proposed design

### 3.1 Info string

A leaf fence is a CommonMark fenced code block whose info string is exactly
two whitespace-delimited tokens:

1. **First token** — `leaf`. Classification is unchanged: the parser commits
   to leaf-handling on this token alone, before reading any body content. No
   F1-style content peek is introduced.
2. **Second token** — the **kind**, matching `[a-z_][a-z0-9_]*` (the existing
   tag-name pattern, unchanged).

```text
```leaf product
```

### 3.2 Body

The leaf body is YAML data, as today, **minus** the `KIND:` line. The first
body key is now an ordinary data field — it has no positional meaning.

### 3.3 `KIND` as a reserved output-only key

`KIND` moves from *sentinel* to *output-only*, joining `BODY`:

- The parser **populates** `Leaf.KIND` from the info-string token.
- Supplying `KIND:` as an **input** body key is a **hard parse error**,
  identical to the existing treatment of `BODY` / `LEAVES`. This catches
  authors (and LLMs) who carry the old habit forward, rather than silently
  accepting a duplicate or contradictory value.

### 3.4 Data model — unchanged

`Leaf { KIND, BODY, [field]: any }` is byte-for-byte the same shape. Only the
*source* of `KIND` moves (info string instead of body). Templates still
address `leaves.product[0].name`; backend contract, `leaf_kinds:` schema, and
all `Leaf*` Rust types are untouched. This keeps blast radius small.

### 3.5 Failure modes

| Input | Result |
|---|---|
| `` ```leaf `` (no second token) | Hard error: leaf fence requires a kind token. |
| `` ```leaf Indorsement `` (bad pattern) | Hard error: kind must match `[a-z_][a-z0-9_]*`. |
| `` ```leaf a b `` (three+ tokens) | **Open question — see §8.** |
| `KIND:` present as a body key | Hard error: `KIND` is output-only (§3.3). |
| `` ```leef product `` (first token typo) | Unchanged: ordinary code block, unknown language, passed through. Not a near-miss diagnostic. |

The "missing `KIND:` first body key" diagnostic disappears; "missing kind
token in leaf info string" replaces it. Both are hard errors — the
classification/F1 story is unchanged.

## 4. Affected surfaces

The SWE planning the work should scope these; the list is indicative, not a
task breakdown.

| Surface | Change |
|---|---|
| `crates/core/src/document/fences.rs` | Extend info-string parsing to extract + validate the second token. Delete the `first_content_key` / `Some("KIND")` body-key gate (currently ~lines 301-320). Missing/invalid kind becomes a fence-stage hard error. |
| `crates/core/src/document/sentinel.rs` | `KIND` no longer read from body. Add `KIND` to the leaf output-only reserved-key check. |
| `crates/core/src/document/assemble.rs` | `BlockKind::Leaf(kind)` sourced from the info string. |
| Markdown emitter (`to_markdown`) | Emit `` ```leaf <kind> ``; stop emitting `KIND:` as the first body line. |
| `prose/designs/MARKDOWN.md` | The authoritative spec — must change in lockstep with the parser, not after. §2 document grammar (the `LeafFence` production), §3.2 leaves (info-string format, drop `KIND:` as a first body key, list `KIND` as output-only), §4.2 leaf detection (second-token extraction + validation), §4.4 failure modes (replace "missing `KIND:`" with "missing kind token"). Every `` ```leaf `` example in the doc gains a kind token. |
| `prose/designs/LEAVES.md` | Data-model design doc — must be updated alongside the spec. The `Leaf` shape is unchanged (`KIND` still present), but any prose, examples, or diagrams that describe `KIND` as a *body key* / sentinel must be rewritten to describe it as an info-string token surfaced as an output-only field. Reconcile with the `KIND`-as-sentinel framing carried over from LEAF_REWORK. |
| `prose/proposals/LEAF_REWORK.md` | Superseded in part: §3.2 (the rejection of this form) and §5 reserved-key tables (move `KIND` from Sentinel to output-only). Leave a forward-pointer to this proposal rather than silently editing rationale. |
| Fixtures / golden files / conformance probes | All in-repo `.md` leaf examples and `spec_conformance_probe.rs` / `security_tests.rs` / `assemble_tests.rs` cases. |

## 5. Migration

The current `` ```leaf `` + `KIND:`-body form is **not widely deployed**, so
this is an **atomic flip — no deprecation window, no legacy parser path** for
the body-`KIND` form. In-repo fixtures are updated mechanically in the same
change.

This proposal is **orthogonal** to the `---/CARD:---` legacy-migration story
in LEAF_REWORK.md §7: if that legacy parser path still exists, its emitter
target simply becomes `` ```leaf <kind> ``. Confirm its current state during
planning (§8).

## 6. Editor / tooling impact

- **Prettier** — unchanged. The info string still leads with the unknown
  token `leaf`; Prettier passes it through verbatim, body untouched.
- **VSCode / language injection** — the Quillmark extension matches on the
  *first* info-string token, so YAML-grammar injection into leaf bodies still
  works. The kind token is now visible to the grammar for kind-specific
  schema selection — a minor improvement.

## 7. What we don't claim

- **The discriminator leaves YAML's semantic regime.** The kind token is now
  a lexer artifact: it cannot carry a YAML comment, is not schema-validated
  by the YAML layer, and its `[a-z_][a-z0-9_]*` grammar is enforced in the
  fence lexer rather than the data layer. This is a real regression in
  uniformity and the main thing we are trading away.
- **Frontmatter/leaf symmetry drops to zero.** `QUILL:` remains a first body
  key; `KIND` no longer is. The "first body key names the fence's role" rule
  is gone for leaves. We consider it a mnemonic, not an invariant — but it is
  genuinely lost.
- **Two reserved-key surfaces still exist.** `KIND` does not disappear; it
  moves from sentinel to output-only, so it is still a reserved body key an
  author can collide with.

## 8. Open questions for the planner — resolved

1. **Extra-token policy.** `` ```leaf a b `` — **hard error** (`fences.rs`
   `leaf_kind_from_tokens`): a leaf info string must be exactly `leaf <kind>`.
2. **`---/CARD:---` legacy path.** Still present and **reworked**. Because
   `KIND:` as a body key is now a hard error, the legacy path can no longer
   rewrite `CARD:`→`KIND:` in the body. `assemble.rs::extract_legacy_card_leaf`
   instead lifts the kind out of the `CARD:` line and drops that line, so the
   block parses as a canonical leaf; the emitter retargets to
   `` ```leaf <kind> `` as before.
3. **Error code naming.** Hard parse errors in this codebase all collapse to
   the single code `parse::invalid_structure` (distinguished by message);
   there is no granular `parse::*` code per error. The new "missing/invalid/
   extra kind token" errors are `ParseError::InvalidStructure`, matching the
   former "missing `KIND:`" error exactly.
