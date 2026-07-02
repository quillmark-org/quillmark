# Proposal: `pdfform` backend + shared AcroForm stamping spine

> **Status**: **V1 implemented** — issue #749, PR #750 (integration branch
> `feat/pdfform`). §1–§8 are the design and remain the source of truth; **§9
> records what actually shipped, the deviations, and the handover for the
> fast-follows.** Read §9 first if you are picking this up.
> **Scope of this doc**: the V1 engine work. The upstream *qualification* layer
> that produces a quill's `form.pdf` + `form.json` is a separate effort and is
> explicitly out of scope.

## 1. Thesis & scope

Quillmark gains a second backend, `pdfform`, dedicated to filling existing PDF
forms — something the Typst backend fundamentally cannot do (Typst 0.15's
`image()` cannot embed a PDF page, so any Typst path would rasterize the form
and lose fidelity).

The load-bearing structural idea is that **AcroForm widget stamping is a
cross-cutting capability, not a Typst detail.** Today it lives buried in
`crates/backends/typst/src/overlay/` + `pdf_scan.rs`, special-cased to signature
fields, backing `usaf_memo`. We lift it into a standalone crate, `quillmark-pdf`,
whose whole job is one pure operation:

```
(base_pdf_bytes, &[FieldSpec]) -> { pdf_with_widgets, regions }
```

via a single incremental-update append. Both backends become *producers of a
base PDF + a list of field specs*; the shared spine stamps. They differ in
**two** ways, and both collapse at the same `&[FieldSpec]` seam:

| | Geometry source | Value-binding mechanism | Base PDF |
|---|---|---|---|
| **Typst** | introspection (dynamic, reflow) | the plate *template* (Typst code) | typst-pdf output |
| **`pdfform`** | `form.json` (fixed, page-relative) | a declarative *resolver* | the stripped background |

We do **not** route forms through Typst, and we do **not** make `pdfform` a
Typst "mode." The unification is *exactly at the seam*, not above it.

### The two-asset model (strip-and-rebuild)

A `pdfform` quill ships two assets the qualification layer produced upstream:

- **`form.pdf`** — the *stripped background*: the normalized form with its
  `/AcroForm`, widget annotations, and page `/Annots` removed (pure pages +
  content streams).
- **`form.json`** — the complete, value-free field reconstruction spec.

The backend writes the AcroForm **fresh** from `form.json` onto the background.
It never reads or reconciles a foreign AcroForm. This deletes the hardest
runtime work (resolving an existing form, walking the `/T` tree, reading
on-states, upserting) and collapses both backends to one "stamp from spec"
operation.

### In scope for V1

- `quillmark-pdf` spine: `&[FieldSpec]` → stamped PDF + regions, all field types.
- `quillmark-pdfform` backend (Typst-free), with the `form.json` resolver.
- Typst **rewired** onto the spine — signatures only; `usaf_memo` stays green.
- One hand-authored form quill fixture (`form.pdf` + `Quill.yaml` + `form.json`).
- `regions` threaded through `RenderResult`.
- The generalized raster-preview seam in core.
- Per-backend feature-gated engine registration.

### Out of scope for V1 (see §7 for the backlog)

- The qualification layer (produces `form.pdf` + `form.json`).
- 508/PDF-UA accessibility beyond `/TU` tooltips.
- Value flattening for PNG/SVG-with-values; the `@quillmark/wasm` canvas contract.
- Continuation/overflow pages; card-instance value addressing.
- Adding general `form-field(...)` fields to the **Typst** plate API.

## 2. Architecture & crate graph

Two new crates, with distinct roles. `quillmark-pdf` is shared *infra* (not a
backend); `quillmark-pdfform` is the backend.

```
quillmark-core ──────────────────────────────────────────────┐
   ▲                                                          │
quillmark-pdf   (shared stamp spine: &[FieldSpec] → stamped PDF + regions)
   ▲   ▲          leaf infra · Typst-free · pdf-writer only · own PdfError
   │   │
   │   └── quillmark-typst     (Backend; deps: quillmark-pdf + typst)
   │
   └────── quillmark-pdfform   (Backend; deps: quillmark-pdf [+ hayro under `preview`])
                                NEVER depends on typst
quillmark (engine) ── optional deps: quillmark-typst    (feature `typst`)
                                     quillmark-pdfform   (feature `pdfform`)
bindings/wasm ── selects engine features
```

Suggested placement (matching the spike): `crates/quillmark-pdf/` (top-level
shared infra) and `crates/backends/pdfform/` (the backend).

### Feature / dependency story

The motivation is that a WASM consumer doing basic form-filling must not link
the Typst compiler **or** the hayro/vello raster tree. The weight is two-axis:

| Build | Deps | Weight |
|---|---|---|
| `pdfform`, no `preview` | core + quillmark-pdf | **tiny** — the form-filling-only bundle |
| `pdfform` + `preview` | + hayro / hayro-svg / vello_cpu | medium |
| `typst` + `render` | + typst | heavy |

- Engine registration is **per-backend feature-gated** (extends the existing
  `#[cfg]`-gated Typst auto-registration in `orchestration/engine.rs`), so a
  form-only build registers *only* `pdfform` and never links Typst.
- `pdfform`'s `preview` (hayro) is **off by default**; default and Typst builds
  stay free of the hayro tree.

### Layering fixes (clean-slate)

- **`quillmark-pdf` owns a `PdfError`**, mapped to `quillmark_core::RenderError`
  at each backend boundary. Today's leaf code returns core's error type with
  `typst::*` codes — an inversion we do not carry forward.
- **The `/Info /Producer` string is passed down** from the product layer
  (`RenderOptions.producer` already threads it), never defaulted from the leaf
  crate's `CARGO_PKG_VERSION`.

## 3. Data model

### Direction A: definition vs value

Quillmark's core dichotomy is **quill = format/template, document = content/data**.
`form.json` sits on the *format* side: it is the static, value-free
field-**definition** layer. Document values come from the markdown + YAML and
are bound at stamp time. Therefore:

- `FieldType` carries **only definitional data** — never a runtime value.
- The bound value lives in **one uniform slot**, attached by the resolver.

This is the resolution of the spike's biggest wart (checkbox `checked` baked
into `FieldType` while text/choice values lived in a flat slot). One value
representation for all types; `form.json` reuses `FieldType` directly with no
parallel enum.

```rust
// quillmark-pdf — the backend-agnostic currency.
pub struct FieldSpec {
    pub name: String,            // /T, fully-qualified
    pub page: usize,             // 0-based
    pub rect: [f32; 4],          // PDF points, BOTTOM-LEFT origin (final)
    pub field_type: FieldType,   // definition only
    pub value: Option<String>,   // the one uniform bound value (None = blank)
    pub tooltip: Option<String>, // /TU
}

pub enum FieldType {
    Text { multiline: bool },
    Checkbox,
    Choice { options: Vec<String> },
    Signature,
}
```

`quillmark-pdf` only ever sees final **bottom-left** geometry; whoever owns the
geometry source converts before constructing the spec (the spike's best
decision — the crate never reasons about page height or reflow).

### The `form.json` on-disk schema

`form.json` is a durable, public, version-controlled artifact authored two ways
(the qualifier; hand-authoring in V1) and consumed three ways (the stamper, the
regions sidecar, human review). It must be complete enough to rebuild a widget
yet inspectable/diffable. The schema:

```json
{
  "schema": "quillmark/form@<version>",
  "fields": [
    {
      "name": "FullName",
      "schema_field": "full_name",
      "page": 0,
      "rect": { "x": 180, "y": 57, "w": 340, "h": 20 },
      "type": "text",
      "tooltip": "Full legal name of the applicant"
    },
    { "name": "Comments",      "schema_field": "comments",       "page": 0,
      "rect": { "x": 180, "y": 120, "w": 340, "h": 80 }, "type": "text", "multiline": true },
    { "name": "Agree",         "schema_field": "agree",          "page": 0,
      "rect": { "x": 180, "y": 90,  "w": 14,  "h": 14 }, "type": "checkbox" },
    { "name": "FavoriteColor", "schema_field": "favorite_color", "page": 0,
      "rect": { "x": 180, "y": 150, "w": 340, "h": 20 }, "type": "choice",
      "options": ["red", "green", "blue"] },
    { "name": "Signature",                                       "page": 0,
      "rect": { "x": 180, "y": 190, "w": 340, "h": 40 }, "type": "signature" }
  ]
}
```

```rust
// quillmark-pdfform — wire types.
pub struct FormSpec { pub schema: String, pub fields: Vec<FormField> }

pub struct FormField {
    pub name: String,
    #[serde(default)] pub schema_field: Option<String>,  // None = unbound (signer fills)
    pub page: usize,
    pub rect: Rect,                                        // top-left {x,y,w,h}, PDF points
    #[serde(default)] pub tooltip: Option<String>,
    #[serde(flatten)] pub kind: FieldKind,
}

pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldKind {
    Text { #[serde(default)] multiline: bool },
    Checkbox,
    Choice { options: Vec<String> },
    Signature,
}
```

Design decisions baked in:

- **Tagged, not flat-bag.** `flatten` + internally-tagged `type` keeps the JSON
  flat and readable while making invalid combinations (a `signature` with
  `options`) unrepresentable. The opinionated trim (below) makes each variant
  tiny, so this is obviously correct.
- **Unknown keys are ignored**, not rejected — additive evolution needs no
  version bump (old engines skip new keys; new engines default missing ones).
- **`schema` follows the Document DTO convention** (`DOCUMENT_STORAGE.md`):
  field named `schema`, value `quillmark/form@<crate-version-at-last-format-change>`,
  hand-set (never `CARGO_PKG_VERSION`). For V1 we adopt only the field+value
  format; the frozen-DTO/chained-migration machinery is built only when a
  *breaking* change first lands. Its job is to turn a future breaking change
  from a silent misparse into a clean rejection.
- **Top-left `{x,y,w,h}` rect**, PDF points (1/72"), page-relative. The pdfform
  loader flips to PDF bottom-left when building the `FieldSpec`, reading page
  height from the background (`y0 = pageH - (y+h)`, `y1 = pageH - y`). This
  defuses the single biggest hand-authoring footgun structurally. Cost: the
  eventual machine qualifier (native bottom-left) does one extra flip —
  acceptable, as the hand-author is V1's actual author.

### The resolver (value binding)

The resolver is the bind step: for each `form.json` field, dereference
`schema_field` against the document data, coerce to the field's value, attach.
Output feeds both the stamped `/V` and the regions sidecar from one resolution.

- **Binds against `compile_data`'s JSON** — the *same* validated, zero-filled
  object the Typst plate reads as `data.*`. This inherits zero-fill, schema
  validation, defaults, and core scalar coercion for free; it does **not** build
  a second data pipeline.
- **Addressing** is a shallow path whose root segment is a schema field name.
  **V1: root scalar + array index** (`field`, `field.<i>`). Card-instance
  addressing (`$cards.<i>.<field>`) is deferred — it is entangled with the
  deferred continuation/overflow problem, so there is no V1 consumer.
- **Coercion** is type-directed, reusing core's scalar rules: text → string
  (array → index/join); checkbox → truthy → engine on-state else `/Off`; choice
  → value must match an option.
- **Unbound = blank.** `schema_field: None`, and (by `PLATE_DATA` semantics) an
  absent-or-null bound value both render an empty field — two origins, identical
  outcome.
- **No shared resolver is extracted.** Typst has no resolver — it has a template
  engine; addressing happens in plate code. The shared layers are `compile_data`
  (in) and `FieldSpec` (out); resolution between them is per-backend by nature.
  A bonus of the declarative form: an unresolvable `schema_field` can be caught
  centrally at quill-load, which template addressing cannot.

### Two type systems (documented expectation)

`schema_field`'s Quill.yaml type should match the widget `type`:
`string→text`, `enum→choice`, `boolean→checkbox`, `array`/`markdown[]→text{multiline}`,
signature unbound. V1 documents this; a compatibility **warning** at quill-load
is a clean future add.

### Deferred overlaps (denormalized for V1)

`form.json` is a **self-contained** reconstruction spec and may carry data that
duplicates Quill.yaml (choice `options` ↔ `enum`, etc.). Deciding a single
source of truth (*normalization*) is deferred. `max_len` is **dropped outright**
(its only V1 use is comb fields, which we don't reproduce), not deferred-as-
duplicated.

## 4. The stamp spine: `quillmark-pdf`

```rust
pub fn stamp(base: Vec<u8>, fields: &[FieldSpec], opts: &StampOptions)
    -> Result<StampResult, PdfError>;

pub struct StampResult { pub pdf: Vec<u8>, pub regions: Vec<RenderedRegion> }
pub struct StampOptions { pub producer: Option<String> }
```

- Writes a fresh `/AcroForm` onto the base via one incremental-update append
  (strip-and-rebuild; never reconciles a foreign form). All four field types
  with `/DR` font registration. **`pdf-writer`** builds the objects — a robust,
  typed dependency, light next to typst/hayro, and safer than hand-rolled bytes
  across four field types.
- **Technique A is locked**: style the *real* AcroForm fields and set
  `/NeedAppearances`; no baked `/AP` appearance streams. **Technique B is out of
  scope.**

### Opinionated rendering (the simplification cascade)

The background PDF owns all visual chrome (boxes, rules, labels). Under
Technique A the widget is a transparent input over that chrome. So we declare
one **house style** and make the stamper opinionated, which deletes most
configurability — and most of the `form.json` surface:

| Dropped from the wire | Because |
|---|---|
| `/DA` string, border/background colors | engine picks one font + `0 Tf` auto-size; background owns chrome |
| `/Ff` flags bitfield | we don't honor arbitrary flags (`multiline` is the one retained text trait) |
| checkbox `on_state` | a fixed export name we choose; we own the rebuilt form |
| choice `combo`, export≠display | choice is always a dropdown; options are bare strings |

Engine-policy constants (in `quillmark-pdf`, not `form.json`): house font + `0 Tf`,
fixed checkbox on-state, dropdown choice, `NeedAppearances`, value-coercion
rules. This is consistent with the locked "opinionated over fidelity-parity"
stance, narrows preview divergence (§5), and is low-regret: every dropped key
is an *additive* re-introduction later (radio groups bring back `on_state`; comb
brings back `max_len`).

### Engine input contract (what the spine assumes of the base PDF)

The reader/appender is a deliberately-minimal byte scanner (lifted from the
Typst crate's `pdf_scan.rs`, not `hayro-syntax` — which is read-only and exposes
no byte spans, so it cannot drive a byte-splice append). It **hard-errors** on
shapes a modern PDF can have: xref *streams*, encryption, indirect
`/Annots`, deeply nested page trees. The V1 contract is therefore that the
background is **traditional-xref, unencrypted, inline-annots, flat-tree**. The
qualification layer guarantees this (its mandatory `qpdf --decrypt` already
emits traditional xref); the V1 hand-authored fixture must satisfy it. This
contract is the precise inverse of the scanner's error branches and is part of
the spike's "normalize upstream, keep the runtime reader light" recommendation.

## 5. Render targets, preview, regions

### Targets

| Target | Typst | `pdfform` |
|---|---|---|
| PDF (deliverable) | typst-pdf + `quillmark-pdf` stamp | stripped bg + `quillmark-pdf` stamp |
| SVG | `typst-svg` | `hayro-svg` (background, `preview` feature) |
| PNG / canvas | `typst-render` | `hayro` raster (background, `preview` feature) |

`pdfform` V1 `SUPPORTED_FORMATS` = `[Pdf]` in the core build; background-only
`Svg`/canvas under the `preview` feature. **PNG/SVG-with-values requires
flattening (§7), a fast-follow** — not V1.

### Generalized raster-preview seam

The canvas painter (`PREVIEW.md`) currently reaches the Typst rasterizer via an
`Any` downcast (`typst_session_of`) "because canvas is Typst-only." `pdfform` is
the second implementor that invalidates that premise. V1 promotes
`render_rgba` / `page_size_pt` to **default-`None` methods on `SessionHandle`**;
`paint` dispatches generically. The capability flag (`Backend::supports_canvas`)
and the impl (`SessionHandle::render_rgba`) must be wired together (they can
currently disagree); fold them so the gap closes by construction.

### Technique A consequence (ratified)

Empirically, hayro (and most non-interactive rasterizers) render
`NeedAppearances` fields **blank** — a stamped PDF rasterized to byte-identical
output as the empty background. Only interactive viewers (Acrobat, Chrome/pdfium,
Preview.app, pdf.js's forms layer) synthesize appearances. **Ratified for V1**:
the deliverable PDF shows values in appearance-synthesizing viewers; getting
values into any *flat/non-interactive* output requires compositing the values
from regions (the flattening fast-follow). This sharpens the regions sidecar
from a preview nicety into the only path to values in non-interactive rendering.

### Regions sidecar

`RenderResult` gains `regions: Vec<RenderedRegion>` (defaulted empty; the spike
confirmed this is non-breaking — all four bindings build unchanged). The types
live in `quillmark-core` (they ride on `RenderResult`); `quillmark-pdf`
re-exports.

```rust
pub struct RenderedRegion { pub name: String, pub page: usize, pub rect: [f32;4], pub kind: RegionKind }
pub enum RegionKind { Field { field_type: String, value: Option<String> } }  // enum from day one
```

Regions ride on **every** render regardless of format — the GUI overlay needs
field geometry whether it shows the PDF or a rastered background. V1 ships this
minimal shape; a presentation enrichment (resolved font/size/align, so preview
and flattening agree exactly) rides *with* the flattening fast-follow.

### The preview/compositing contract

Preview diverges from the deliverable only in **value typography, never in field
geometry** — and that is a narrower divergence than it sounds, because "the PDF"
has no single appearance under Technique A (Acrobat ≠ Chrome). Geometry (rect)
is authoritative and shared by every surface; only how the value text is typeset
inside an otherwise-identical box varies. Opinionated rendering (§4) narrows even
that, by giving our surfaces one styling source of truth.

Who draws the bound values depends on the rendering path, **not** on using
`pdfform`:

| Path | Who draws values |
|---|---|
| Stamped PDF in an interactive viewer | the viewer synthesizes them |
| Quillmark canvas/raster preview | the **GUI** composites from regions |
| Server-side flat PNG/SVG | the **engine** flattens them in (§7) |

This per-backend `render_rgba` semantic (Typst: complete; pdfform: background-only,
compose from regions) is an implicit contract today and must be made **explicit**
at the trait, so a consumer wiring `pdfform` doesn't expect Typst's
self-sufficiency.

## 6. Typst rewire & legacy deletion

The prior AcroForm work is a private post-process in the Typst crate:
`overlay/{mod,inject,extract}.rs`, `pdf_scan.rs`, wiring in `compile.rs`/`lib.rs`,
backing `usaf_memo` (`plate.typ` `signature-field`) + `sig_field.rs` /
`producer_meta.rs` / `usaf_memo_signature_test.rs`.

V1 resolution:

- **Extract** stamping into `quillmark-pdf` as a clean, designed layer (not a
  salvage of `pdf_scan.rs`).
- **Rewire** Typst's signature feature onto it: `overlay` becomes a thin adapter
  (introspection → top-left→bottom-left flip → `FieldSpec::Signature` → `stamp`).
  Coordinate ownership moves into the backend; the spine stops importing
  `typst_layout`.
- Delete `pdf_scan.rs` + `overlay/inject.rs` from the Typst crate.
- `usaf_memo` signatures stay green — the regression proof the seam is real.

**V1 keeps Typst signature-only.** Generalizing the Typst plate API to arbitrary
`form-field(...)` fields (the issue's `region(name)[…]` Phase 2 generalizes the
same `<__qm_sig__>` query/position mechanism) is post-V1; the spine already
supports every field type, exercised through `pdfform`.

## 7. V1 deliverables, phasing, backlog

### V1 (this buildout)

1. `quillmark-pdf` spine (`stamp`, all field types, `PdfError`, `pdf-writer`).
2. Typst rewired onto it; legacy deleted; `usaf_memo` green.
3. `quillmark-pdfform` backend + `form.json` resolver.
4. One hand-authored form quill fixture (`form.pdf` + `Quill.yaml` + `form.json`).
5. `regions` threaded through `RenderResult` (types in core).
6. Generalized raster-preview seam; `supports_canvas` reconciled with the impl.
7. Per-backend feature-gated engine registration; wasm can render `pdfform`→PDF
   behind a feature.

### Acceptance

- `usaf_memo` and existing Typst tests unchanged and green.
- The fixture renders to a structurally-valid filled PDF (lopdf-validated);
  field styling + `0 Tf` auto-size verified across Acrobat / Chrome / Preview
  (human eyes — not possible headless).
- All four bindings compile unchanged.

### Fast-follow / tooling backlog (deferred, not V1)

- **Value flattening** for PNG/SVG-with-values: draw values into page content
  via `pdf-writer` over the background, then hayro/hayro-svg render them. This is
  the appearance-synthesis the issue punted on (text layout, `0 Tf` auto-size
  emulation) — the single biggest *new* piece of work. It is **internal
  preview-only machinery, not a PDF-output deliverable**: PDF output is always an
  interactive AcroForm (Technique A). Lives in `pdfform`'s `preview` feature,
  **never** in the spine.
- **Regions presentation enrichment** (font/size/align) — rides with flattening.
- **Surface `regions` in the WASM typed API** (then Python) — opt-in,
  additive; wasm first as the GUI consumer.
- **Canvas contract in `@quillmark/wasm`** — background paint + value
  compositing from regions + DPR/clamp math + the per-backend "complete vs
  background-only" semantics, shipped *inside* `@quillmark/wasm` so the binding
  is the reference implementation. Plus golden-image conformance fixtures.
- **`form.json` → `Quill.yaml` scaffold** — a self-contained schema codegen
  (no PDF work): `schema_field`→key, `type`→Quill.yaml type, `options`→`enum`,
  `tooltip`→`description`, geometry→field order. A one-time scaffold (then
  hand-owned), guaranteeing `schema_field`↔schema consistency by construction.
  **Belongs in the quillification pipeline (separate repo) alongside the
  qualification layer that produces `form.json`, not in this engine** — a brief
  `quillmark` CLI subcommand for it was prototyped here (#756) and removed.
- **Card-instance value addressing** (`$cards.<i>.<field>`, `$cards.<kind>.<i>.<field>`)
  — landed in `resolve.rs`. Binds one card instance per form field, so a *static*
  multi-page form can lay out a bounded number of card slots across its existing
  pages and place each instance's value on its page.
- **Continuation/overflow pages, page composition, and PDF merging are out of
  scope.** `pdfform` fills *static* forms only: it stamps over a fixed,
  pre-existing page set and never composes content, appends continuation pages,
  or merges PDFs. A document carrying more card instances than the form has slots
  is the author's concern, not the engine's.

## 8. Decisions ratified & deferred

### Ratified

1. `form.json` is a complete, value-free field-definition spec (Direction A).
2. Byte-preservation is replaced by **visual-fidelity + clean reconstruction**
   (strip and rebuild; background preserved structurally, never rasterized).
   Preservation is path-dependent: the single-form path keeps original bytes
   (incremental append); the future continuation path re-serializes via
   hayro-write (still structural, never rasterized).
3. Technique A is locked; Technique B out of scope. Consequence (blank in
   non-synthesizing renderers) is accepted; flattening is the flat-output path.
4. Opinionated rendering over fidelity-parity; the qualifier owns
   stripped-background fidelity ("rects land on printed boxes").
5. Two crates; `pdfform` is Typst-free; `pdf-writer` for the writer; per-backend
   feature-gated registration.

### Deferred (acknowledged, not solved here)

- The qualification layer (decrypt/strip/extract/verify) — separate effort.
- 508/PDF-UA accessibility beyond `/TU` (a tagged struct tree is a larger piece).
- Fixed AcroForm capacity / continuation pages; card-instance addressing.
- `form.json` ↔ Quill.yaml normalization (a single source of truth for overlaps).
- Radio groups (reintroduce `on_state`); comb fields (reintroduce `max_len`).
- Adding general `form-field(...)` to the Typst plate API.

## 9. V1 implementation notes & handover

> Post-implementation record (issue #749 / PR #750). The design above shipped
> essentially as specified; this section captures **what landed**, the
> **deviations** a future contributor must know, the **partially-scaffolded
> deferrals** (so nobody re-discovers them), an **extension-point map**, and the
> **way forward**.

### What shipped

- `quillmark-pdf` — the Typst-free stamp spine (`stamp`, all four field types,
  `/DR` Helvetica, Technique A, own `PdfError`, lifted byte reader/appender).
- `quillmark-pdfform` — the backend (`form.json` parse → `compile_data`-bound
  resolver → `&[FieldSpec]` → `stamp`).
- Typst rewired onto the spine; `pdf_scan.rs` + `overlay/inject.rs` deleted;
  `usaf_memo` signatures green (the regression proof).
- `regions` threaded through `RenderResult` (`RenderedRegion`/`RegionKind` in
  core); the generalized raster-preview seam; per-backend feature-gated
  registration; the `sample_form` fixture.
- Gates: `cargo test --workspace --all-features --locked` green; `cargo doc
  -Dwarnings` clean; new crates clippy-clean; all four bindings compile.

### Deviations & decisions made during implementation

1. **`pdf-writer` pinned to `0.15`** in `[workspace.dependencies]`. The proposal
   named `pdf-writer` without a version; the Typst toolchain transitively forces
   `0.15` (via `typst-pdf → krilla`), so the spine adopts `0.15` to link a single
   copy rather than a second `0.14` one.
2. **Non-zero `/MediaBox` origin is honoured.** The proposal's flip
   (`y0 = pageH - (y+h)`) assumed a `(0,0)` page origin. The reader returns full
   normalized boxes (`page_media_boxes`) and the flip offsets by the box origin,
   so a translated background (`[10 20 622 812]`) places widgets correctly.
3. **Robustness hard-errors** were added to match the reader's "reject
   out-of-contract input cleanly" stance: checked object-id allocation (a
   near-`u32::MAX` `/Size` → clean `PdfError`, not an overflow panic / silent
   xref corruption) and a bounded `startxref` offset.
4. **Multiline / newline-bearing text `/V` serializes as UTF-16BE** (pdf-writer's
   encoding for values outside the literal-safe set) — valid, viewer-decoded. The
   flattening fast-follow must decode it accordingly.
5. **Checkbox**: the fixed on-state (`Yes`) drives **both** `/V` and `/AS` (the
   bound value only signals checked/unchecked), plus `/MK /CA (4)` for the
   ZapfDingbats check the viewer synthesizes under `NeedAppearances`.

### Deferred but partially scaffolded — now landed (see dated note 2026-06-27)

The three items below were scaffolded-but-incomplete at V1 (PR #750) and have
since shipped on this branch; kept here for the historical trail.

- **`pdfform`'s `preview` raster.** *Landed.* Under the `preview` feature
  `pdfform` wires real `hayro`/`hayro-svg` deps, implements
  `SessionHandle::{render_rgba, page_size_pt}` plus `render_svg`, reports
  `supports_canvas() == true`, and `SUPPORTED_FORMATS == [Pdf, Svg]`. The session
  pre-flattens values into the page (see `flatten.rs` / `typography.rs`), so the
  raster is *complete* rather than background-only.
- **Typst-free wasm `pdfform` bundle.** *Landed.* `wasm` Cargo.toml splits
  `typst` / `pdfform` / `pdfform-preview` features (the old `render` feature is
  now `typst`); `build-wasm.sh` builds the Typst-free artifact and `runtime.js`
  registers it.
- **`regions` surfaced in a binding's typed API.** *Landed (wasm).* `wasm`
  exposes `FieldRegion`/`FieldRegionKind` (`From<RenderedRegion>`), populated on
  `RenderResult.regions` in both render paths. python still pending.

### Extension-point / file map

- Spine `crates/quillmark-pdf/`: `stamp.rs` (the op, per-field-type widget
  writers, Technique A, id allocation), `reader.rs` (byte scanner + input
  contract + `page_media_boxes`), `lib.rs` (`FieldSpec`/`FieldType`,
  `page_media_boxes`), `error.rs` (`PdfError`). House-style policy constants
  (`CHECKBOX_ON_STATE`, `DEFAULT_APPEARANCE`) live in `stamp.rs`.
- Backend `crates/backends/pdfform/`: `form.rs` (`form.json` wire types +
  schema-tag check), `resolve.rs` (binding, coercion, top-left→bottom-left
  flip), `lib.rs` (`Backend` + session).
- Core: `crates/core/src/region.rs` (sidecar types), `session.rs` (the canvas
  seam), `backend.rs` (`supports_canvas` static/dynamic contract).
- Registration: `crates/quillmark/src/orchestration/engine.rs`
  (`#[cfg(feature = "pdfform")]`); features in `quillmark` + `wasm` Cargo.toml.
- Fixture: `crates/fixtures/resources/quills/sample_form/0.1.0/`
  (`form.pdf` + `Quill.yaml` + `form.json`, all four field types).

### Way forward (priority order)

The full backlog is §7; each item below is filed as a tracking issue.

1. **Value flattening** (PNG/SVG-with-values) — the single biggest new piece and
   the only path to values in flat output; unblocks regions presentation
   enrichment and the `pdfform` `preview` raster. Lives in `pdfform`'s render
   feature, never the spine. → **#752**
2. **The qualification layer** (decrypt/strip/extract/verify → `form.pdf` +
   `form.json`) — the upstream dependency for *real* PDF-form quills; today's
   fixture is hand-authored. → **#753**
3. **`pdfform` preview raster** (hayro background, the `preview` stub) **+
   typst-free WASM bundle.** → **#754**
4. **Surface `regions` in the bindings + the `@quillmark/wasm` canvas contract**
   (value compositing + golden-image fixtures). → **#755**
5. **`form.json` → `Quill.yaml` scaffold** (#756) — prototyped as a `quillmark`
   CLI subcommand, then removed: it belongs in the quillification pipeline
   (separate repo) with the qualification layer, not this engine.
6. **Card-instance addressing** (`$cards.<i>` / `$cards.<kind>.<i>`) → **#757** —
   *landed; see the dated note below.* Continuation/overflow pages, page
   composition, and PDF merging are **out of scope** (§7): `pdfform` fills static
   forms only.
7. **General `form-field(...)` in the Typst plate API.** → **#758**

Longer-term, acknowledged-but-unscheduled items stay in §8 (508/PDF-UA
accessibility, `form.json` ↔ Quill.yaml normalization, radio groups, comb
fields).

### Card-instance addressing (2026-06-27, #757)

**Card-instance addressing landed; page composition / continuation is out of
scope.** `pdfform` fills *static* forms only.

- **Card-instance addressing** is in `resolve.rs`: a `schema_field` rooted at
  `$cards` binds one card instance, by absolute index (`$cards.<i>.<field>`) or
  by kind + index (`$cards.<kind>.<i>.<field>`). It binds against the same
  `$cards` array the Typst plate iterates, so it inherits zero-fill, validation,
  and coercion.
- A **static multi-page form** can therefore lay out a bounded number of card
  slots across its existing pages — each slot a `form.json` field with its own
  `page`, bound to a distinct instance — and each instance's value lands on its
  page. No new engine code: per-field `page` + card-instance addressing carry it
  (proven through `field_spec` in `resolve.rs`'s tests).
- **Continuation/overflow pages, page composition, and PDF merging are out of
  scope and not implemented.** The engine stamps over a fixed, pre-existing page
  set; it never composes content, appends continuation pages, or merges PDFs. A
  document with more card instances than the form has slots is the author's
  concern, not the engine's. (`pdfform` consequently has **no** `lopdf` / page-
  merge dependency — it is `pdf-writer`-only for stamping.)

### Open verification (not possible headless)

Field styling and `0 Tf` auto-size must be eyeballed in appearance-synthesizing
viewers — render the `sample_form` fixture and confirm in **Acrobat, Chrome
(pdfium), and Preview.app**: the text/choice values are visible and auto-sized
inside their boxes, the checkbox shows a check, and every widget lands on its
printed box. Flat/non-interactive renderers show the fields blank by design
(Technique A); that is expected, not a regression.
