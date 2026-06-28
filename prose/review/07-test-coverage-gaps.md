# 07 — Test coverage gaps

**Severity:** low–medium **Category:** coverage **Status:** Partially resolved

The tractable Rust-level gaps are closed (see "Closed" below). What remains
needs a non-Rust harness (a stricter PDF validator, a CI matrix entry, a JS
assertion, or a binding surface) and is left open.

## Still open

### Structural PDF validity

- **No stricter validator over multi-type output.** Widgets are validated by a
  tolerant `lopdf` reparse (which "silently tolerates" malformed dicts) plus a
  byte-level duplicate-`/Subtype` check. The new text/checkbox/choice widgets
  get no byte-level duplicate-key check, and no qpdf / MuPDF / pdfium lint runs
  over multi-field-type output. A qpdf/MuPDF lint would harden the fence across
  all four field types.
- **`usaf_memo` multi-signature plate not exercised end-to-end** in
  `sig_field.rs` — the real regression target uses several `Ind_<i>_Signature`
  widgets on a page; the current tests cover one field per page.

### Preview / canvas (`pdfform`, `preview` feature)

- **Multiline / auto-size layout untested.** Flatten tests assert presence of
  `BT`/`Tj`/`re W n`/WinAnsi bytes but not the multiline line-advance
  (`0 -line_h Td`) or the `0 Tf` auto-size clamp; a regression collapsing all
  lines onto one baseline would pass.
- **Canvas "complete raster" heuristic is too weak.** `canvas.test.js:284`
  asserts `inkPixels > 0`, which the background's own borders/labels satisfy
  even if no field value is painted. Sample a known field coordinate (from
  `FieldRegion.rect` → device space) and assert non-white, or floor `inkPixels`
  by expected glyph coverage.

### Build matrix / bindings

- **Headless `pdfform`-only build (no preview) untested.** The motivation for
  the feature split — a Typst-free, raster-free form-filling bundle — has no CI
  step. A break gated on `#[cfg(feature = "pdfform")]` without
  `pdfform-preview` would go undetected. Add a `cargo check`/`test` matrix entry
  (and ideally a wasm size-budget check for the pdfform artifact, analogous to
  the `core` budget guard).
- **Python `regions` not exposed (confirmed).** `PyRenderResult` exposes only
  `artifacts` / `warnings` / `format` / `render_time_ms` — there is no `regions`
  getter (`crates/bindings/python/src/types.rs`). Intentionally pending until
  `pdfform` ships in the Python binding; expose it then.
- **No non-ASCII value end-to-end through the `pdfform` backend.** The WinAnsi
  transcode and the UTF-16BE `/V` encoding are unit-tested (`writer.rs`,
  `flatten.rs`), but the integration tests (`sample_form.rs`,
  `canvas_conformance.rs`) only use ASCII values, so no test drives an accented
  value through the full backend render to the AcroForm `/V` (UTF-16BE decode).

## Closed (landed on `claude/prose-review-items-6svv5p`)

- **Spine (`quillmark-pdf`)** — non-zero-generation rejection (catalog + page),
  xref-stream and encrypted rejection, non-zero `/MediaBox` origin through
  `page_media_boxes`, multi-subsection xref coalescing, inline-`/Annots` merge
  and indirect-`/Annots` hard error, and the `pdf_text_string` UTF-16BE /
  surrogate-pair branch.
- **Regions sidecar** — the pdfform `sample_form` integration test asserts a
  non-empty `regions` list with name / field-type / value, now extended with
  page + non-degenerate-rect geometry.
- **Coercion / resolver (`pdfform`)** — `is_truthy` string/number variants,
  `coerce_text` mixed/all-null arrays, and the numeric-`$kind` card-addressing
  limitation (tested + documented on `lookup_card`).
- **Flatten byte-level coverage** — restored at the `flatten()` unit level as
  the finalization of the flatten collapse.

## Closed (landed on `claude/code-review-main-a3rokh`)

- **Spine (`quillmark-pdf`)** — `endobj`-inside-a-string no longer truncates an
  object (string-aware `find_object_bytes`); cyclic / shared-node `/Pages` trees
  are rejected (visited-set, closing the O(nodes × file) amplification); a
  non-zero page `/Rotate` is rejected rather than mis-stamped; and the
  producer-only (no-fields) `/Info` `/Producer` success path is asserted.
- **pdfform** — duplicate `form.json` field names are rejected at parse; JSON
  number stringification matches the Typst producer (integral floats drop the
  trailing `.0`), so the two backends bind identical text and choice options.
