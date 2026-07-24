# Bindings

> **Implementation**: `crates/bindings/`

## TL;DR

Quillmark exposes one core engine through several language surfaces — Python
(PyO3), WebAssembly (wasm-bindgen), and a CLI binary. Every surface drives the
same `quillmark` core: the same
`Document`/`Quill`/`Card` model, the same `serde` diagnostics, and the same
capability principle. Surfaces differ only in language idiom, packaging, and
which extras they expose (canvas preview is WASM-only).

## Shared model

- **Capability principle.** A `Quill` is portable, declarative config data.
  Its format capability (`supportedFormats`) and rendering are resolved by the
  `Quillmark` engine *against* a quill at render time — never by the quill
  itself. So `quill.metadata` is a pure, infallible config snapshot, while
  `render` / `supportedFormats` can fail for an unregistered backend.
- **One model, serialized across every boundary.** The `Document`/`Card` model
  and `Diagnostic`s cross each language boundary as the same core `serde`
  shapes (`CardWire`, the versioned storage DTO, `Diagnostic`) — so a card or
  an error reads identically no matter which surface emits it. See
  [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md), [CARDS.md](CARDS.md),
  [ERROR.md](ERROR.md).
- **Uniform errors.** Each binding raises a single error type that always
  carries a non-empty diagnostic list (`QuillmarkError.diagnostics` /
  thrown `Error.diagnostics`).

The WASM binding is the reference surface; Python mirrors it and catches up
on a best-effort basis (see its status notes below). New contract work lands
in WASM first.

## The write surface: object placement over one primitive

Placement decides where a mutation verb lives, in one sentence:

> **If a verb needs a schema, it lives on the writer. `Document` is quill-free
> data.**

`quill.writer(doc)` (mirroring core's `quill.writer(&mut doc)`) is the one
schema-bound door: bare `set` / `set_all` / `setBody` / `reviseField` / `addCard`
/ `card(i)`, names and markdown in, diagnostics out. It resolves each field's type
from the bound quill, so a name the schema does not declare is a typo
(`UnknownField`), not a fallback.

`Document` holds everything quill-free: the opaque `store*` primitive (verbatim,
coercion deferred to render) and the addressed content lane — `install` / `revise`
/ `applyChange` plus the `importMarkdown` / `exportMarkdown` / `rebase` / `mapPos`
codec — which navigate by `Addr` and return `Delta` receipts but never consult a
schema. **Transport** reads (`getStored` / `isFill` / `getExt`) return the stored
value verbatim, need no schema, and sit on `Document` too.

The one **interpreting** read — projecting a field by its type — is a
schema-shaped question ("this field's richtext, as markdown"), so it gains a
schema-bound home: `quill.reader(doc)`, the read twin of `quill.writer(doc)`
(mirroring core's `quill.reader(&doc)`). `reader.get(addr)` reads each field by its
declared type — a `richtext` field to markdown, a `plaintext` field to its
literal text (marks verbatim), every other type verbatim — with schema
authority, so a name the schema does not declare throws `UnknownField` instead of
reading back `undefined`, and a content field holding an undecodable value throws
`FieldRichtextDecode`. A field's markdown lives here: `getMarkdown` /
`get_markdown` / `get_card_markdown` are **body-only** (WASM `getMarkdown` takes a
`CardAddr`; a present `field` throws), and the quill-free **body** projection
stays on `Document` (a body's type is a format fact, not a schema fact, so
`reader.getBody` mirrors it rather than gating it on the schema). The placement rule
generalizes: *a verb that needs a schema lives on the writer (writes) or the view
(reads); `Document` is quill-free data.*

`reviseField` is the writer verb that is both typed *and* anchor-preserving: it
rebases surviving anchors like the content `revise`, then conforms the diffed
result to the field schema like `set`. Because it needs the schema, it lives on
the writer — wrapping core's `Card::revise_field_checked` primitive, which
`Document` does not expose.

**The verb carries the lane.** One vocabulary rule, stated once here: **store**
= verbatim (the quill-free opaque write, coercion deferred to render), **set** =
typed (the writer's strict commit at the write), **install / revise / apply** =
content (identity-aware). `remove_*` has no lane — one verb serves every write path.
So `store_field` / `store_fields` / `store_fill` (+ `store_ext` / `store_seed_*`)
are the opaque store, `set` / `set_all` / `set_body` the typed writer, and a name
never needs per-verb disambiguation against its neighbor (the opaque batch
`store_fields` and the typed batch `set_all` are not near-homographs).

**The read side mirrors it.** The verbatim read is `getStored` — the read echo of
`store` — and the interpreted read is `reader.get`, the reader/writer twin of
`set`. So the transport and schema-plane reads carry the lane in the verb the same
way the writes do, rather than collapsing both onto one `get` where only the
receiver (`doc` vs `reader`) tells them apart. (Core needs no such split: its
verbatim read is the map-idiomatic `payload().get`, already lexically distinct from
`reader.get`; the collision is a WASM/pyo3 artifact of fusing the read onto the one
`Document` handle, so only the bindings rename it.)

**Writers and card cursors are ephemeral — bind, write, discard.** They hold an
address (the quill + document, or an index), never a cache; every call reads
through the document, so a `removeCard` / `addCard` between binding a cursor and
writing through it silently retargets it. Durable addressing is `$id` stamped at
build and re-resolved at patch time ([PROGRAMMATIC.md](PROGRAMMATIC.md)), not a
held handle.

**The hand-written runtime is the real API; the wasm class is its ABI.** The
quill-taking `_commitField` / `_commitFields` / `_addCard` / `_reviseField`
methods are the stable ABI under the writer's `set` / `set_all` / `addCard` /
`reviseField` — underscored and dropped from the `.d.ts`, not from the binary.
The visible `Document` class then carries zero quill-taking methods, so the split
is structural, not asserted.

### Parity table

Every binding verb is *identical* to its core twin or names its one forced
difference — **FFI** (a wasm-bindgen / pyo3 constraint), **idiom** (a language
ergonomic), or **scope** (a lane a binding omits by intent — Python is Tier 1 +
storage + render, [#970](https://github.com/borb-sh/quillmark/issues/970)),
nothing else admitted. Drift is a reviewable diff to this table.

| Concept | Core | Bindings | Class |
|---|---|---|---|
| Typed writer front door | `quill.writer(&mut doc)` | `quill.writer(doc)` | **idiom** — core holds `&mut Document` under the checker; the bindings re-borrow per call (pyo3/wasm objects carry no lifetime), so the guarantee becomes the ephemerality convention |
| Typed reader front door | `quill.reader(&doc)` | `quill.reader(doc)` | **idiom** — the read twin; same re-borrow/ephemerality as the writer |
| Interpreted read | `reader.get(name)?` → `ReadValue` (richtext → markdown, plaintext → literal text, else canonical); `reader.card(i)?.get(..)?` | `reader.get(addr)` / `reader.card(i).get(name)` (JS), `reader.get(name)` / `reader.card(i).get(name)` (py) | **idiom** / **FFI** — one `get` interprets by declared type; absent → `undefined`/`None`, unknown name → `UnknownField`, undecodable content → `FieldRichtextDecode`; a field's markdown reads here, not on the body-only `getMarkdown` (#978). Body read (`reader.getBody` / absent-field addr) stays quill-free |
| Scalar / batch write | `set` / `set_all` | `set` / `setAll` (JS), `set` / `set_all` (py) | identical |
| Receipt-free body write | `set_body(md)` | `setBody(md)` / `set_body(md)` | identical — core also exposes the delta via `revise_body` |
| Typed richtext field revise | `TypedWriter::revise_field(name, md)?` / `CardWriter::revise_field(..)?` | `writer.reviseField(name, md)` / `writer.card(i).reviseField(..)` (JS); `writer.revise_field(name, md)` / `writer.card(i).revise_field(..)` (py) | **idiom** — typed *and* anchor-preserving; both wrap `Card::revise_field_checked`. JS returns the `Delta`; Python discards it (the position-mapping receipt is an editor concern, WASM-only) |
| Card creation | `add_card(kind, fields, body?, at?)` | `addCard(kind, fields?, body?, at?)` | identical — fused make + typed-commit + insert, transactional (`at` appends when absent, else inserts at the index — one atomic positioned insert, not `addCard` + `moveCard`) |
| Card insertion | `push_card(card)` / `insert_card(i, card)` | `insertCard(card, at?)` | **idiom** — the binding folds core's append + positional-insert verbs into one; absent `at` appends |
| Card removal (writer) | `writer.remove_card(i)` | `writer.removeCard(i)` | identical |
| Card cursor | `writer.card(i)?` (eager check) | `writer.card(i)` (lazy check) | **FFI** — no borrow to validate against; the index is checked at the write |
| Cursor kind | `writer.card(i)?.kind()` | `writer.card(i).kind` | identical — the JS getter reads through `doc.card(i)` |
| Reads (value / body markdown / fill / `$ext`) | `body_markdown(..)` / `payload().get(..)` / `payload().is_fill(..)` / `card.ext()` (borrow chain; index for a card) | `doc.getStored(addr?)` / `doc.getMarkdown(cardAddr?)` / `doc.isFill(addr)` / `doc.getExt(cardAddr?)` / `doc.getExtNamespace(cardAddr, ns)` (JS) | **idiom** / **FFI** / **scope** — WASM fuses the transport reads onto the one `Addr` (a bare string ⇒ `{field}` for `getStored`/`isFill`) and names the verbatim field read `getStored`, not `get`, so it never collides with the interpreted `reader.get` (core's `payload().get` has no such neighbor); *total over the field axis* (absent field → `undefined`, `isFill` → `false`; only an out-of-range card throws); `getMarkdown` is the **body** read (a `CardAddr`; a present `field` throws). Python has no quill-free field read — interpreted field reads go through `quill.reader(doc).get` (#978, #970), and `$ext` / body content read off the `main` / `cards` dict snapshots |
| Reads (whole card / `$id` / seed) | `card(i)` / `find_card(id)` / `main().seed()` | `doc.card(i)` / `doc.cardIndexById(id)` / `doc.seedOverlay(kind)` | **idiom** — the bindings fuse each into one named verb on `Document`; `card(i)` throws out of range, `find_card`/`cardIndexById` resolve the durable `$id` handle (unique per document — [DOCUMENT_STORAGE.md](DOCUMENT_STORAGE.md) § Card-id identity) |
| Richtext revise (content lane) | `Card::revise_field(name, md)?` (schema-blind, borrow chain) | `doc.revise({card, field}, md)` (addr literal, JS) | **FFI** / **scope** — same model, flattened navigation, schema-blind, `Delta` in hand; WASM-only. Python's anchor-preserving write is the typed `writer.revise_field` (#970) |
| Opaque store | `store_field` / `store_fields` / `store_fill` | `storeField` / `storeFields` / `storeFill` (JS, `Addr`) | **scope** — the quill-free verbatim write, WASM-only; Python has no opaque field store (the typed writer is the only field write, #970). A field write without a loadable quill operates on the storage DTO directly |
| Parse + warnings | `Document::parse(md) -> Parsed { document, warnings }` | `Document.fromMarkdown(md)` → `doc.warnings` getter | **FFI** — the wrapper fuses `Parsed` + `Document` into one session object: `fromMarkdown` returns the document directly and stashes the parse `warnings` on it (`doc.warnings`). That getter is a deliberate asymmetry with core, where warnings live only on `Parsed`: it is session state, so `equals` and the storage DTO exclude it and `loadJson`/`fromJson` clear it (a reloaded document carries no parse warnings) |

The single **idiom** row on the front door is the honest cost: the typed writer
is the one shape pyo3 carries worst, so its "identical" is qualified, not
claimed. See the as-built [0.93 → 0.94 migration](../../docs/migrations/0.93-to-0.94.md#the-two-tier-binding-surface-932).

## Python — `bindings/quillmark-python`

PyO3 bindings published as `quillmark` on PyPI. A `snake_case` surface over the
shared model; one-shot `engine.render` (no canvas).

> **Scope: Tier 1 + storage + render ([#970](https://github.com/borb-sh/quillmark/issues/970)).**
> Field I/O flows through `quill.writer(doc)` / `quill.reader(doc)` exclusively;
> `Document` is quill-free data and structure (parse, the storage DTO,
> `insert_card` / `remove_card` / `move_card` / `make_card`, `remove_field`,
> `$ext` / `$seed`). The opaque store (`store_field` / `store_fields` /
> `store_fill`) and the anchor-preserving content lane (`install` / `revise` /
> `apply_change` + the `import_markdown` / `export_markdown` / `rebase` /
> `map_pos` codec) are **WASM-only by scope, not by lag** — their audience,
> storage/migration tooling holding no quill and live editors preserving anchor
> identity, is not a Python audience. A field write without a loadable quill
> operates on the storage DTO directly.

Every Python verb is identical to its core/WASM twin or names its one difference
in the parity table above: `card=None` selectors fold the composable-card `$ext`
/ `remove_field` twins onto one axis, and `revise_field` discards the `Delta`
(an editor receipt). No half-mirrored lane remains to drift.

## WebAssembly — `bindings/quillmark-wasm`

wasm-bindgen bindings published as `@quillmark/wasm`. Builds with
`--target bundler` and `--weak-refs` so wasm-bindgen handles are reclaimed by
`FinalizationRegistry`; `.free()` remains as the eager teardown hook. Requires
Node 22+ / current evergreen browsers.

Ships **multiple artifacts from one crate** behind a single public root export.
The root `@quillmark/wasm` is a hand-written **canonical runtime layer** that
re-exports the internal Typst-less **core** build's `Document` + `Quill`
(load / validate / schema / seed / blueprint) verbatim and adds an `Engine`
render dispatcher. Each backend (Typst today) is a **private** build with its
own linear memory, lazily loaded on the first render — there is no public
`/core` or `/render` subpath. The core build is ~0.66 MB gzip; the Typst backend
~8 MB (Typst dominates), loaded only when something renders. Backend handles
never escape the `Engine`: it clones the quill tree + `doc.toJson()` into the
backend's memory as serialized data and frees the clones. See the
[as-built 0.90 design](../../docs/migrations/0.89-to-0.90.md).

Beyond the byte-output verbs (`engine.render`, `LiveSession.render`), the
canvas-capable backend builds (Typst, and pdfform under its preview seam)
expose a **live preview** path on `LiveSession` (`apply`, `pageCount`,
`pageSize`, `paint`, …). See [PREVIEW.md](PREVIEW.md).

## CLI — `bindings/quillmark-cli`

Standalone `quillmark` binary. See [CLI.md](CLI.md).

## Links

- [PROGRAMMATIC.md](PROGRAMMATIC.md) — building documents in memory through each surface's mutators
- [CLI.md](CLI.md) — command-line surface
- [PREVIEW.md](PREVIEW.md) — WASM multi-backend canvas preview (Typst, pdfform)
- [ERROR.md](ERROR.md) — the diagnostic model that crosses every boundary
- Per-binding API detail: the respective `crates/bindings/*/` rustdoc and READMEs
