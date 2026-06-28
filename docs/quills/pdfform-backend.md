# PDF Form Backend

The `pdfform` backend fills existing PDF forms — something the Typst backend fundamentally cannot do (Typst cannot embed a PDF page, so a Typst path would rasterize the form and lose fidelity). Instead of generating a page from a plate, `pdfform` stamps a fresh AcroForm onto a pre-existing background and binds your document's values into the widgets.

It is Typst-free: a `pdfform` quill never compiles Typst code and a form-only build never links the Typst compiler.

## The two-asset model

A `pdfform` quill ships **two assets at its root** instead of a plate:

```
my-form/
├── Quill.yaml
├── form.pdf     # the stripped background — pages + chrome, no form fields
└── form.json    # the value-free field reconstruction spec
```

- **`form.pdf`** — the *stripped background*: the normalized form with its `/AcroForm`, widget annotations, and page `/Annots` removed (pure pages, rules, boxes, and labels).
- **`form.json`** — the complete, value-free description of every field: name, page, geometry, type.

At render time the backend writes the AcroForm **fresh** from `form.json` onto the background, then binds each field's value from your document data. It never reads or reconciles a form already in `form.pdf`.

!!! note "Where the assets come from"
    Producing a clean `form.pdf` + `form.json` from a raw source PDF (decrypt, strip, extract, verify) is the job of a separate *qualification* layer and is out of scope for the engine. V1 quills hand-author both assets; the `sample_form` fixture in `crates/fixtures/resources/quills/sample_form/` is a worked example.

## `Quill.yaml`

A `pdfform` quill declares `backend: pdfform` and has **no plate file**. The document body is typically disabled — a form is filled from fields, not prose. Fields under `main.fields` define the document schema exactly as for any other backend:

```yaml
quill:
  name: sample_form
  version: 0.1.0
  backend: pdfform
  description: "Demo PDF form filled by the Typst-free pdfform backend."

main:
  body:
    enabled: false
  fields:
    full_name:
      type: string
      default: ""
      example: Ada Lovelace
      description: Full legal name of the applicant. Binds the FullName text field.

    comments:
      type: array
      items:
        type: string
      default: []
      ui:
        multiline: true
      description: "Free-form comments; each element becomes one line of the Comments field."

    agree:
      type: boolean
      default: false
      description: Whether the applicant agrees to the terms. Binds the Agree checkbox.

    favorite_color:
      type: string
      enum:
        - red
        - green
        - blue
      default: red
      description: Favorite color. Binds the FavoriteColor dropdown.
```

See the [Quill.yaml Reference](quill-yaml-reference.md) for the full field-type vocabulary.

## `form.json`

`form.json` is a durable, version-controlled artifact — complete enough to rebuild every widget, yet readable and diffable. Each entry describes one field:

```json
{
  "schema": "quillmark/form@0.1.0",
  "fields": [
    {
      "name": "FullName",
      "schema_field": "full_name",
      "page": 0,
      "rect": { "x": 180, "y": 100, "w": 340, "h": 20 },
      "type": "text",
      "tooltip": "Full legal name of the applicant"
    },
    {
      "name": "Comments",
      "schema_field": "comments",
      "page": 0,
      "rect": { "x": 180, "y": 140, "w": 340, "h": 80 },
      "type": "text",
      "multiline": true
    },
    {
      "name": "Agree",
      "schema_field": "agree",
      "page": 0,
      "rect": { "x": 180, "y": 240, "w": 14, "h": 14 },
      "type": "checkbox"
    },
    {
      "name": "FavoriteColor",
      "schema_field": "favorite_color",
      "page": 0,
      "rect": { "x": 180, "y": 280, "w": 340, "h": 20 },
      "type": "choice",
      "options": ["red", "green", "blue"]
    },
    {
      "name": "Signature",
      "page": 0,
      "rect": { "x": 180, "y": 330, "w": 340, "h": 40 },
      "type": "signature"
    }
  ]
}
```

### Field keys

| Key | Required | Notes |
|---|---|---|
| `name` | yes | The widget's `/T` entry — unique within the document. |
| `page` | yes | 0-based page index into `form.pdf`. |
| `rect` | yes | `{x, y, w, h}` in **PDF points** (1/72"), **top-left origin**, page-relative (see below). |
| `type` | yes | One of `text`, `checkbox`, `choice`, `signature`. |
| `schema_field` | no | The document field this widget binds to. Omit for unbound fields (e.g. a signer-filled signature). |
| `tooltip` | no | The widget's `/TU` tooltip. |
| `multiline` | text only | `true` for a multi-line text box. |
| `options` | choice only | The dropdown options, as bare strings. |

### Top-left coordinates

`rect` is authored **top-left origin** — `x`/`y` measured from the top-left corner of the page, the way a human reads a form. The backend flips to PDF's native bottom-left origin when it builds the widget, reading the page height from `form.pdf` and honouring a non-zero `/MediaBox` origin. This defuses the single biggest hand-authoring footgun, so you never reason about page height or coordinate flipping yourself.

### Schema versioning and unknown keys

`schema` follows the convention `quillmark/form@<version>`, hand-set at the last format change (never auto-derived from a crate version). Unknown keys are **ignored, not rejected**, so the format can grow additively — old engines skip new keys and new engines default missing ones.

### Opinionated styling

The background owns all visual chrome; each widget is a transparent input over it. The backend therefore picks one house style (a standard font, auto-sized text, a fixed checkbox on-state, `NeedAppearances`) and `form.json` carries **no** styling: no fonts, colors, borders, flag bitfields, or per-field appearance. Keep `form.json` to geometry and type.

## Binding values

Each field's value comes from the **resolver**: for every `form.json` field with a `schema_field`, the backend dereferences that path against your document data and coerces it to the field's type.

- **Bound against the same validated data the Typst plate sees.** Schema validation, defaults, zero-fill, and scalar coercion are all inherited — there is no second data pipeline.
- **Addressing** is a shallow path rooted at a schema field name, optionally with an array index or nested key: `full_name`, `comments.0`.
- **Coercion is type-directed:**

| Type | Binding |
|---|---|
| `text` | String value; numbers/bools stringify; an **array joins with newlines** (one element per line — the multiline fill). Empty → blank. |
| `checkbox` | Truthy value → checked; otherwise unchecked. |
| `choice` | The value must match one of `options` exactly, else the field is left blank. |
| `signature` | Never bound — always an empty, signer-fillable widget. |

- **Unbound = blank.** A field with no `schema_field`, or whose bound value is absent or `null`, renders empty.

### Card-instance addressing

A `schema_field` rooted at the reserved `$cards` key binds one card instance from the document's `$cards` array (the same array the Typst plate iterates):

- **By absolute index:** `$cards.0.from` — the first card overall.
- **By kind + index:** `$cards.indorsement.1.from` — the second card whose `$kind` is `indorsement`, skipping intervening cards of other kinds.

This lets a **static, fixed-capacity** form lay out a bounded number of card slots across its existing pages — each slot a `form.json` field with its own `page`, bound to a distinct instance.

!!! warning "Static forms only"
    `pdfform` stamps over a fixed, pre-existing page set. It never composes content, appends continuation pages, or merges PDFs. A document carrying more card instances than the form has slots is the author's concern, not the engine's.

## Signature fields

A `type: signature` field (with no `schema_field`) produces a clickable, **unsigned** AcroForm signature widget. Open the result in Acrobat — or any reader that supports form signing — and the widget presents a "Sign Here" affordance.

The widget is unsigned: Quillmark performs no cryptography. To produce a signed PDF, run the output through pyHanko, Acrobat, endesive, or another signing tool.

## Output formats

| Format | Support |
|---|---|
| **PDF** | Always — the deliverable; always an interactive AcroForm (Technique A). |
| **SVG** | A `render()` output format, under the `preview` feature only. |

`OutputFormat::Png` is **not** supported: under `preview` the backend's formats
are `[Pdf, Svg]`, so `render` with `output_format: Some(Png)` returns
`FormatNotSupported`.

**Canvas** is a separate surface from the `render()` output formats above: it is
the WASM `paint()` raster path (`render_rgba`), not an `OutputFormat`. It is
likewise available only under `preview` (which the WASM build enables via
`pdfform-preview`). See [PREVIEW.md](../../prose/canon/PREVIEW.md).

The PDF is the real deliverable. By design (Technique A — real fields plus `NeedAppearances`, no baked appearance streams), **values appear only in viewers that synthesize appearances** — Acrobat, Chrome/pdfium, Preview.app, pdf.js's forms layer. A flat, non-interactive rasterizer renders the fields blank.

To get values into flat output, the `preview` feature pre-flattens them: it bakes each value into the page content stream so the SVG/PNG raster is complete rather than background-only. The `preview` build links a raster tree (hayro) and is off by default, keeping form-only and Typst builds lean.

### Regions sidecar

Every render returns a `regions` list on the `RenderResult` — one entry per field, carrying its name, page, geometry, and resolved value:

```rust
pub struct RenderedRegion { pub name: String, pub page: usize, pub rect: [f32; 4], pub kind: RegionKind }
pub enum RegionKind { Field { field_type: String, value: Option<String> } }
```

Regions ride on **every** render regardless of format, so a GUI can overlay or composite field values onto whatever surface it shows. The `pdfform_preview` example (`crates/quillmark/examples/`) renders the `sample_form` fixture and prints its regions for cross-checking against a viewer.

## Resources

- [PDF AcroForm reference](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf) — §12.7, Interactive Forms
- [pyHanko](https://github.com/MatthiasValvekens/pyHanko) — signing a stamped PDF

## Next steps

- [Creating Quills](creating-quills.md) — quill bundle layout and workflow
- [Quill.yaml Reference](quill-yaml-reference.md) — full field types and constraints
- [Typst Backend](typst-backend.md) — the other backend, for generated documents
