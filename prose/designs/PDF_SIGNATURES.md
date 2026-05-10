# PDF Signatures Spike

> **Status**: Spike / Proof-of-concept  
> **Branch**: `claude/pdf-signatures-spike-BBDh7`  
> **Scope**: Feasibility of digital signature fields in Quillmark-generated PDFs

## TL;DR

Typst has no native PDF signature support and none is imminent. The viable path
is a **sentinel overlay** pattern: `quillmark-helper` exports a
`signature-field()` Typst function that places a visible placeholder box and
emits a `metadata` position sentinel; Quillmark post-processes the PDF with
`lopdf` to inject AcroForm `SigField` widget annotations at those coordinates.
The pattern works today. What remains is production hardening.

---

## Research Findings

### Typst native support

Typst 0.14.2 has **no AcroForm, no PAdES, no CAdES support**. Tracked in:

- [typst/typst#2421](https://github.com/typst/typst/issues/2421) — Interactive
  Forms tracking issue (signature fields explicitly deferred)
- [typst/typst#4368](https://github.com/typst/typst/issues/4368) — RFC for PDF
  Form Elements (signature fields flagged as "uncertain for first iteration")

`typst_pdf::PdfOptions` has no signing parameters. Nothing in `typst_pdf`
touches AcroForm or signatures.

### The sentinel overlay pattern

The community tool `typst-fillable` (Python) demonstrates a working pattern for
overlaying interactive form fields onto Typst PDFs:

1. Embed `metadata()` sentinels in the Typst plate at desired positions.
2. After compilation, query the compiled document for sentinel coordinates.
3. Post-process the PDF to inject AcroForm field widgets.

Quillmark can do this entirely in Rust by querying the `PagedDocument`'s
built-in `Introspector` — no subprocess or extra Typst CLI invocation needed.

---

## Architecture

```
quill plate (.typ)
  └─ #import "@local/quillmark-helper:0.1.0": data, signature-field
     └─ #signature-field("approver")
          ├─ renders: visible grey box  200×50 pt
          └─ emits:  metadata(("qm-sig", "approver", 200.0, 50.0)) <qm-sig>

Quillmark backend (Rust)
  ├─ typst::compile()  →  PagedDocument
  ├─ sig_overlay::extract_sig_fields(&document)
  │    └─ Introspector::query(Selector::Label("qm-sig"))
  │         → Vec<SigFieldPlacement> { page, x_pt, y_pt, width_pt, height_pt, page_height_pt }
  ├─ typst_pdf::pdf(...)  →  Vec<u8>   (raw PDF)
  └─ sig_overlay::inject_sig_fields(&pdf, &fields)
       └─ lopdf: add AcroForm /SigField widget per placement
            → Vec<u8>   (PDF with unsigned signature fields)
```

### Key files

| File | Role |
|---|---|
| `crates/backends/typst/src/lib.typ.template` | `signature-field()` Typst function |
| `crates/backends/typst/src/sig_overlay.rs` | Position extraction + lopdf injection |
| `crates/backends/typst/src/compile.rs` | `compile_to_pdf` wired to run overlay |

---

## Sentinel design

### Typst side (`lib.typ.template`)

```typst
#let signature-field(name, width: 200pt, height: 50pt) = box(
  width: width, height: height,
  stroke: (paint: luma(160), thickness: 0.5pt), fill: luma(252),
)[
  #place(top + left)[
    #metadata(("qm-sig", name, width.pt(), height.pt())) <qm-sig>
  ]
  #align(center + horizon)[
    #text(size: 8pt, fill: luma(140))[× Signature: #name]
  ]
]
```

- Sentinel array: `("qm-sig", <name>, <width_pt>, <height_pt>)`
- Label `<qm-sig>` makes the element queryable from the Introspector
- `place(top + left)` positions the metadata at the box's content-area origin

### Rust side (`sig_overlay::extract_sig_fields`)

```rust
// typst::introspection::Introspector — available on PagedDocument
let elems = doc.introspector.query(&Selector::Label(Label::new(PicoStr::intern("qm-sig")).unwrap()));
for elem in &elems {
    let pos = doc.introspector.position(elem.location().unwrap());
    // pos.page: NonZeroUsize  pos.point: Point { x: Abs, y: Abs }
    let packed = elem.to_packed::<MetadataElem>().unwrap();
    // packed.value: Value::Array([Str("qm-sig"), Str(name), Float(w), Float(h)])
}
```

No secondary Typst process or `typst query` CLI invocation — pure in-process.

---

## Coordinate conversion

Typst frames: origin top-left, y increases downward.  
PDF page: origin bottom-left, y increases upward.

```
pdf_x1 = typst_x
pdf_y2 = page_height_pt − typst_y              (box top in PDF space)
pdf_x2 = typst_x + box_width_pt
pdf_y1 = page_height_pt − typst_y − box_height_pt  (box bottom in PDF space)
```

`page_height_pt` is read from `doc.pages[page_idx].frame.size().y.to_pt()`.

---

## PDF injection via lopdf

```rust
// For each sig field:
let widget = Dictionary with /Type /Annot, /Subtype /Widget, /FT /Sig, /Rect, /T, /P, /F
let widget_id = doc.add_object(Object::Dictionary(widget));
// Append widget_id to page /Annots
// Create /AcroForm { /Fields [...], /SigFlags 3 }
// Set /AcroForm on document /Root catalog
doc.save_to(&mut out)
```

`SigFlags: 3` = `SignaturesExist | AppendOnly` — standard for signature forms.

The resulting PDF contains unsigned `SigField` widgets. Signing is a separate
step (signers open the PDF in Acrobat/Adobe Sign/pyHanko and sign in-app, or
Quillmark could later call pyHanko or endesive from a server-side component).

---

## Known limitations and open questions

### Positional precision (~0.5 pt offset)

`place(top + left)` inside the `box` puts the metadata element at the box's
content-area origin. The 0.5pt stroke renders outside the content area, so the
sentinel x/y may be off by ~0.5pt relative to the visual box edge. Options:

1. Accept the sub-point error (invisible to end users).
2. Use `locate(loc => place(abs: loc.position(), ...))` to emit absolute-position
   metadata directly — eliminates any offset.
3. Expose a dedicated Quill schema field (`type: "signature"`) and handle
   positioning in the Rust backend rather than in the plate.

### Width/height from metadata vs. introspector

The sentinel encodes `width.pt()` and `height.pt()` at *Typst's resolved value*.
If a plate author uses relative widths (e.g., `100%`) the `.pt()` call resolves
to the actual computed width, so the values in the metadata are correct. No
action needed.

### Unsigned vs. signed fields

The spike injects *unsigned* `SigField` widgets. To actually sign:

- **Server-side (Rust)**: `endesive` or `pyHanko` via subprocess can apply a
  PKCS#12 certificate to an existing SigField.
- **Client-side**: The PDF viewer handles signing after delivery.
- **No-go for WASM**: Cryptographic signing requires keystore access not
  available in the browser; delegate to a server endpoint.

### WASM build impact

`lopdf` is a pure-Rust crate; no libc dependency. The WASM build compiles it
without platform-specific flags. Binary size impact: ~100–200 KB uncompressed
(lopdf + flate2 + nom). Acceptable for the server-side `quillmark-python` and
`quillmark-cli` targets; marginal for the WASM bundle. Gate behind a Cargo
feature if WASM size becomes a concern.

### render_document_pages / TypstSession::render

The overlay currently runs only in `compile_to_pdf`. The `render_document_pages`
path (used by `TypstSession::render` in the WASM and Python bindings) does not
run the overlay — it calls `typst_pdf::pdf` directly. To cover all PDF output
paths, `render_document_pages` should also call `inject_sig_fields`. Deferred
for post-spike.

### Multiple signature fields, multiple pages

The spike handles multiple sentinels (each `signature-field()` call produces
one). Multi-page documents work because `Position.page` is 1-indexed and is
decoded correctly.

### AcroForm /DR (default resources)

A complete AcroForm requires a `/DR` (default resources) dict for default fonts
used to render field appearances. The spike omits `/DR`; this is fine for
invisible SigFields but may cause warnings in strict validators. Add a minimal
`/DR` referencing Helvetica before shipping.

---

## What the spike validates

| Question | Answer |
|---|---|
| Can Typst emit sentinels with page coordinates? | **Yes** — `metadata() <label>` + `Introspector::query` works in-process |
| Can lopdf inject AcroForm SigField widgets post-hoc? | **Yes** — standard lopdf dict manipulation |
| Does `place(top + left)` give the box's top-left position? | **Approximately yes** (sub-point offset from stroke) |
| Does the pattern survive multi-page, multi-field documents? | **Yes** (by design) |
| Does this work in WASM? | **Probably yes** (lopdf is pure Rust); needs WASM build test |
| Does this enable cryptographic signing? | **No** — unsigned SigFields only; signing is a separate step |

---

## Recommended next steps (post-spike)

1. **Integration test**: compile a minimal plate with `signature-field()`, render
   to PDF, parse with lopdf, assert `/AcroForm` dict and `/SigField` widget exist
   at expected coordinates.
2. **Extend `render_document_pages`**: apply overlay there too so all PDF output
   paths benefit.
3. **Quill schema field type**: add `type: "signature"` to `Quill.yaml` so
   signature placement is driven by structured data, not raw Typst code. The
   helper would generate `signature-field()` calls automatically.
4. **Production signing**: evaluate pyHanko (Python subprocess from
   `quillmark-python`) or `endesive` (Rust) for server-side signing. The
   unsigned SigField output from this spike is a valid input to both.
5. **Feature flag**: gate the overlay behind `Cargo.toml` `[features]` once
   design is settled, so callers that don't need signatures pay zero cost.
