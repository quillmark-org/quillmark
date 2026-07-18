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

## The write surface: two tiers over one primitive

The mutation surface has a stated default, decided by one sentence:

> **Tier 1 speaks names, values, and markdown. Tier 2 speaks addresses,
> corpora, and receipts.**

Tier 1 is the typed writer — `quill.writer(doc)`, mirroring core's
`quill.writer(&mut doc)`. It is the documented default: bare
`set` / `set_all` / `setBody` / `addCard` / `card(i)`, names and markdown in,
diagnostics out — a consumer here never meets an `Addr`, a corpus object, or a
`Delta`. Tier 2 is the corpus lane — the addressed `install` / `revise` /
`applyChange` verbs plus the `importMarkdown` / `exportMarkdown` / `rebase` /
`mapPos` codec — for the audience that has anchor identity to preserve.

**The tiers are strata, not a partition.** Tier 1 is *sugar over* tier 2 and the
typed-commit path — `writer.setBody(md)` is `revise({}, md)` with the receipt
discarded; `writer.set` is `commitField` with the quill bound once. Anything
tier 1 writes, tier 2 can write with more control, so the decision tree picks a
*default*, not a *cage*: a live editor legitimately writes fields through the
writer and bodies/splices through the addressed verbs in one interaction. Reads
(`get` / `getMarkdown` / `isFill` / `getExt`) need no schema, so they sit on
`Document`, not the writer. The quill-free `storeField` primitive stays the
third lane — verbatim storage, coercion deferred to render.

**The verb carries the lane.** One vocabulary rule, stated once here: **store**
= verbatim (the quill-free opaque write, coercion deferred to render), **set** =
typed (the writer's strict commit at the write), **install / revise / apply** =
corpus (identity-aware). `remove_*` has no tier — one verb serves every lane.
So `store_field` / `store_fields` / `store_fill` (+ `store_ext` / `store_seed_*`)
are the opaque store, `set` / `set_all` / `set_body` the typed writer, and a name
never needs per-verb disambiguation against its neighbor (the opaque batch
`store_fields` and the typed batch `set_all` are not near-homographs).

**Writers and card cursors are ephemeral — bind, write, discard.** They hold an
address (the quill + document, or an index), never a cache; every call reads
through the document, so a `removeCard` / `addCard` between binding a cursor and
writing through it silently retargets it. Durable addressing is `$id` stamped at
build and re-resolved at patch time ([PROGRAMMATIC.md](PROGRAMMATIC.md)), not a
held handle.

**The hand-written runtime is the real API; the wasm class is its ABI.** The
`commit*` verbs are the stable ABI under the writer's `set` / `set_all` — dropped
from the documented surface, not from the binary. This design commits to that
split rather than merely tolerating it.

### Parity table

Every binding verb is *identical* to its core twin or names its one forced
difference — **FFI** (a wasm-bindgen / pyo3 constraint) or **idiom** (a language
ergonomic), nothing else admitted. Drift is a reviewable diff to this table.

| Concept | Core | Bindings | Class |
|---|---|---|---|
| Typed writer front door | `quill.writer(&mut doc)` | `quill.writer(doc)` | **idiom** — core holds `&mut Document` under the checker; the bindings re-borrow per call (pyo3/wasm objects carry no lifetime), so the guarantee becomes the ephemerality convention |
| Scalar / batch write | `set` / `set_all` | `set` / `setAll` (JS), `set` / `set_all` (py) | identical |
| Receipt-free body write | `set_body(md)` | `setBody(md)` / `set_body(md)` | identical — core also exposes the delta via `revise_body` |
| Card creation | `add_card(kind, fields, body?, at?)` | `addCard(kind, fields?, body?, at?)` | identical — fused make + typed-commit + insert, transactional (`at` appends when absent, else inserts at the index — one atomic positioned insert, not `addCard` + `moveCard`) |
| Card insertion | `push_card(card)` / `insert_card(i, card)` | `insertCard(card, at?)` | **idiom** — the binding folds core's append + positional-insert verbs into one; absent `at` appends |
| Card removal (writer) | `writer.remove_card(i)` | `writer.removeCard(i)` | identical |
| Card cursor | `writer.card(i)?` (eager check) | `writer.card(i)` (lazy check) | **FFI** — no borrow to validate against; the index is checked at the write |
| Cursor kind | `writer.card(i)?.kind()` | `writer.card(i).kind` | identical — the JS getter reads through `doc.card(i)` |
| Reads (value / markdown / fill / `$ext`) | `field_markdown(..)` / `payload().get(..)` / `payload().is_fill(..)` / `card.ext()` (borrow chain; index for a card) | `doc.get(addr?)` / `doc.getMarkdown(addr?)` / `doc.isFill(addr)` / `doc.getExt(cardAddr?)` / `doc.getExtNamespace(cardAddr, ns)` (JS); name-keyed twins (py) | **idiom** / **FFI** — WASM fuses every read onto the one `Addr` (a bare string ⇒ `{field}`), *total over the field axis* (absent field → `undefined`, `isFill` → `false`; only an out-of-range card throws); Python stays name-keyed |
| Reads (whole card / `$id` / seed) | `card(i)` / `find_card(id)` / `main().seed()` | `doc.card(i)` / `doc.cardIndexById(id)` / `doc.seedOverlay(kind)` | **idiom** — the bindings fuse each into one named verb on `Document`; `card(i)` throws out of range, `find_card`/`cardIndexById` return the first `$id` match (non-unique by design) |
| Richtext ops | `revise_field(name, md)?` / `revise_field_checked(name, md, schema)?` (borrow chain) | `doc.revise({card, field}, md)` / `doc.reviseChecked(quill, {card, field}, md)` (addr literal) | **FFI** — same model, flattened navigation; the `*_checked` verb enforces the field schema on the diffed result (typed *and* anchor-preserving), keeping the `Delta` in the corpus lane |
| Opaque store | `store_field` / `store_fields` / `store_fill` | `storeField` / `storeFields` / `storeFill` (JS, `Addr`), `store_field` / `store_fields` / `store_card_field` (py, name-keyed) | identical — the quill-free verbatim write; WASM addresses with `Addr`, Python stays name-keyed |
| Parse + warnings | `Document::parse(md) -> Parsed { document, warnings }` | `Document.fromMarkdown(md)` → `doc.warnings` getter | **FFI** — the wrapper fuses `Parsed` + `Document` into one session object: `fromMarkdown` returns the document directly and stashes the parse `warnings` on it (`doc.warnings`). That getter is a deliberate asymmetry with core, where warnings live only on `Parsed`: it is session state, so `equals` and the storage DTO exclude it and `loadJson`/`fromJson` clear it (a reloaded document carries no parse warnings) |

The single **idiom** row on the front door is the honest cost: the typed writer
is the one shape pyo3 carries worst, so its "identical" is qualified, not
claimed. See the as-built [0.93 → 0.94 migration](../../docs/migrations/0.93-to-0.94.md#the-two-tier-binding-surface-932).

## Python — `bindings/quillmark-python`

PyO3 bindings published as `quillmark` on PyPI. A `snake_case` surface mirroring
the shared model; one-shot `engine.render` (no canvas).

> **Status: experimental, second-class binding.** The Python surface lags the
> WASM binding in coverage and in error-shape uniformity. Do not gate releases
> on Python parity.

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
