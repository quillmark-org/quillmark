# PDF Signature Fields

> **Status**: Implementation-ready  
> **Scope**: Unsigned AcroForm SigField widgets in Quillmark-generated PDFs

## Approach

Plate authors call `signature-field()` from `quillmark-helper`. It renders a
visible placeholder box and emits a `metadata` sentinel labelled `<qm-sig>`.
After Typst compiles the document the Rust backend queries the `PagedDocument`
introspector for those sentinels, converts their coordinates to PDF space, and
post-processes the PDF bytes with `lopdf` to inject AcroForm `SigField` widget
annotations. No subprocess, no external tool, no Typst rebuild.

## Authoring API

```typst
#import "@local/quillmark-helper:0.1.0": data, signature-field

#signature-field("approver")                          // 200 × 50 pt default
#signature-field("co-signer", width: 150pt, height: 40pt)
```

Each call produces one unsigned signature box in the output PDF. Field names
must be unique within a document (PDF AcroForm requirement).

## Data flow

```
plate.typ
  └─ signature-field(name, w, h)
       ├─ box (visible, 0.5pt grey stroke)
       └─ metadata(("qm-sig", name, w.pt(), h.pt())) <qm-sig>   ← sentinel

compile_to_pdf()
  ├─ typst::compile()             → PagedDocument
  ├─ sig_overlay::extract_sig_fields()
  │    └─ introspector.query(Label("qm-sig"))
  │         → Vec<SigFieldPlacement> { page, x_pt, y_pt, w_pt, h_pt, page_h_pt }
  ├─ typst_pdf::pdf()             → Vec<u8>  (raw PDF)
  └─ sig_overlay::inject_sig_fields()
       └─ lopdf: /AcroForm + /SigField widget per placement  → Vec<u8>
```

## Coordinate conversion

Typst: top-left origin, y down. PDF: bottom-left origin, y up.

```
pdf_rect = [x_pt,  page_h − y_pt − h_pt,  x_pt + w_pt,  page_h − y_pt]
```

`page_h` comes from `doc.pages[i].frame.size().y.to_pt()`.

## Already implemented (this branch)

| File | What changed |
|---|---|
| `lib.typ.template` | `signature-field()` Typst function + sentinel |
| `sig_overlay.rs` | `extract_sig_fields`, `inject_sig_fields`, `SigFieldPlacement` |
| `compile.rs` | overlay wired into `compile_to_pdf`; falls back on lopdf error |
| `Cargo.toml` | `lopdf = "0.34"` |

## Remaining work

**Must-do before shipping:**

1. **Cover `render_document_pages`** — the `TypstSession::render` path
   (`compile.rs:render_document_pages`) calls `typst_pdf::pdf` directly and
   bypasses the overlay. Extract `extract_sig_fields` before `typst_pdf::pdf`
   and pass fields through, or store them on `TypstSession`.

2. **Integration test** — compile a minimal plate with `signature-field()`,
   call `compile_to_pdf`, parse the result with `lopdf`, assert:
   - `/AcroForm` dict present in document catalog
   - one `/SigField` widget annotation on the correct page
   - `/Rect` coordinates within 1pt of expected values

**Nice-to-have:**

3. **`/DR` default resources** — add a minimal `/DR` dict referencing Helvetica
   to the `/AcroForm` object. Required by strict PDF validators; harmless to omit
   for Acrobat/Adobe Sign.

4. **`[feature]` gate** — `lopdf` adds ~150 KB to the WASM bundle. Gate the
   overlay behind `features = ["pdf-signatures"]` if WASM size is a concern.

## Out of scope

**Cryptographic signing** is a separate step. The output of this feature is an
*unsigned* SigField. Signing options:
- **Client-side**: user opens the PDF in Acrobat, Preview, or Adobe Sign
- **Server-side**: pass the unsigned PDF to `pyHanko` (Python) or `endesive`
  (Rust) with a PKCS#12 certificate — both accept AcroForm SigFields as input
