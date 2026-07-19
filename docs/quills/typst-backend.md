# Typst Backend

The Typst backend generates PDF, SVG, and PNG documents using the [Typst](https://typst.app/) typesetting system. It converts card-yaml payload fields to Typst markup, injects them into the plate as JSON via a helper package, and compiles to the requested format.

## Data Access

Plates are plain Typst code. Document metadata reaches the plate as a JSON dictionary exposed by the virtual `@local/quillmark-helper` package:

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.title                                  // direct — errors if missing
#data.at("title", default: "Untitled")       // safe with default
```

Fields declared `type: richtext` in `Quill.yaml` arrive as Typst content (their content lowered to markup, ready to render); `type: date` and `type: datetime` fields arrive as Typst `datetime` values — the backend emits a `datetime()` constructor at codegen, three-component for a `date` and six-component (carrying the wall-clock time) for a `datetime`.

### Checking for Optional Fields

Use Typst's `in` operator to check for optional fields:

```typst
#if "subtitle" in data {
  [Subtitle: #data.subtitle]
}

// Or use spread syntax for function arguments
#show: template.with(
  title: data.title,
  ..if "subtitle" in data {
    (subtitle: data.subtitle,)
  } else {
    (:)
  },
)
```

### Body, arrays, and cards

The document body is exposed under the `$body` key, accessed via `data.at("$body")` because Typst identifiers exclude `$`. Arrays come through as Typst arrays. Cards live under the `$cards` key, each carrying its own `$kind` discriminator, fields, and `$body`:

```typst
#data.at("$body", default: "")

#for author in data.authors [- #author]

#for card in data.at("$cards", default: ()) {
  if card.at("$kind") == "product" {
    [Product: #card.name — #card.at("$body")]
  }
}
```

## Typst Packages

Declare packages in `Quill.yaml`, then `#import` them from the plate:

```yaml
typst:
  packages:
    - "@preview/appreciated-letter:0.1.0"
```

```typst
#import "@local/quillmark-helper:0.1.0": data
#import "@preview/appreciated-letter:0.1.0": letter

#show: letter.with(sender: data.sender, recipient: data.recipient)
```

Browse the full catalog at [Typst Universe](https://typst.app/universe/).

## Fonts

System-installed fonts are available directly (`#set text(font: "Arial")`). To bundle fonts with the Quill, drop them in `assets/fonts/`:

```
my-quill/
└── assets/
    └── fonts/
        ├── CustomFont-Regular.ttf
        └── CustomFont-Bold.ttf
```

Then reference them by family name (`#set text(font: "CustomFont")`).

## Typesetting

Plate authors style output with Typst's standard `#set` directives:

```typst
#set page(paper: "us-letter", margin: 1in, numbering: "1")
#set text(font: "Linux Libertine", size: 11pt, lang: "en")
#set par(justify: true, leading: 0.65em)
```

See the [Typst tutorial](https://typst.app/docs/tutorial/) for the full styling vocabulary. For worked plates that combine data access with real layout, see the `usaf_memo` and `taro` examples in `crates/quillmark/examples/`.

## Signature Fields

Import `signature-field` from the helper package to drop an unsigned PDF signature box anywhere in your plate:

```typst
#import "@local/quillmark-helper:0.1.0": signature-field

Approving authority:
#signature-field("approver")

Witness:
#signature-field("witness", width: 220pt, height: 60pt)
```

PDF output gains a clickable AcroForm SigField widget at each call site. Open the result in Acrobat (or any reader that supports form signing) and the widget presents a "Sign Here" affordance. SVG and PNG outputs reserve the same invisible layout space — useful for preview but no widget visual.

**Important:** the widget is **unsigned**. Quillmark does not perform any cryptography. To produce a signed PDF, run the output through pyHanko, Acrobat, endesive, or another signing tool.

### Positioning

`signature-field` is ordinary Typst inline content sized `width × height`. It participates in layout the same way `#rect(width: 200pt, height: 50pt)` would — content after it gets pushed by the box's dimensions. Two modes:

**In-flow (reserves layout space).** Drop the call where you want to claim that block of space and let the rest of the document flow around it:

```typst
Sign here:
#signature-field("approver")  // reserves 200×50pt below the label
The above signature acknowledges receipt.
```

**Overlay (no displacement).** Wrap in `#place(...)` to anchor the widget without consuming flow. This is what you want when the surrounding template *already* reserves space — for example, the four blank lines above a typed-name signature block in a USAF memo:

```typst
// At the cursor position where the typed-name signature block begins:
#place(dx: 0pt, dy: -3.5in,
       signature-field("approver", width: 3in, height: 0.5in))
```

`#place` without an alignment argument anchors the widget at the current cursor (then offsets by `dx`/`dy`); `#place(top + left, ...)` anchors to the containing block's top-left. Either way, the call consumes no flow space and the surrounding template stays put.

Inside `#box`, `#table`, `#figure`, `#footnote`, `#move`, `#pad` — `signature-field` tracks the layout system normally. Multi-page documents work; each field's `page` is the page it lays out on, not where it was written in source.

### Parameters

| Name | Type | Default | Notes |
|---|---|---|---|
| `name` | `str` | required (positional) | Field name — must be unique within the document and match `[A-Za-z0-9_.]+` (`.` allowed for fully-qualified names). Surfaces as the widget's `/T` entry. |
| `width` | `length` | `200pt` | Must be an absolute length (`pt`, `mm`, `cm`, `in`) — relative lengths like `2em` or `50%` are rejected. |
| `height` | `length` | `50pt` | Same constraint as `width`. |

### Errors

- Two calls with the same `name` raise a compilation error (`typst::duplicate_form_field`). `signature-field` is a thin wrapper over the same `form-field` primitive that backs text/checkbox/choice widgets, so its names share one uniqueness domain with theirs.
- A non-absolute `width` or `height` raises a Typst assert pointing at `form-field`.
- Names violating `[A-Za-z0-9_.]+` raise a Typst assert.

The label `<__qm_field__>` and metadata `kind: "__qm_field__"` are reserved for this hand-off — don't use them for unrelated metadata in your plate.

> `signature-field` emits a document-global `metadata` element (standard Typst
> introspection). If your plate or its packages read config via
> `query(metadata)`, filter to your own elements rather than assuming a single
> or last metadata element.

## Form Fields

`signature-field` is a thin wrapper over the general `form-field` primitive, which backs all four widget kinds — text inputs, checkboxes, choice dropdowns, and signature boxes. Import it from the same helper package:

```typst
#import "@local/quillmark-helper:0.1.0": form-field
```

Each call drops an AcroForm widget at its call site (a clickable field in PDF; reserved invisible layout space in SVG/PNG, same as `signature-field`). Value binding is the plate author's job — pass `value:` straight from your data; there is no resolver on the Typst side.

### Parameters

| Name | Type | Default | Notes |
|---|---|---|---|
| `name` | `str` | required (positional) | Widget `/T` name — unique within the document, matching `[A-Za-z0-9_.]+`. Shares one uniqueness domain with `signature-field`. |
| `type` | `str` | `"text"` | One of `"text"`, `"checkbox"`, `"choice"`, `"signature"`. |
| `value` | per type | `none` | The delivered field value; interpretation depends on `type` (see below). |
| `options` | `array` of `str` | `()` | Display strings for `type: "choice"`; ignored otherwise. |
| `multiline` | `bool` | `false` | Toggles the multi-line flag for `type: "text"`; ignored otherwise. |
| `width` | `length` | `200pt` | Absolute length (`pt`/`mm`/`cm`/`in`); relative lengths (`2em`, `50%`) are rejected. |
| `height` | `length` | `20pt` | Same constraint as `width`. |
| `field` | `str` or `none` | `none` | Schema-field address this widget's region is keyed on (see "Binding to a schema field"). |

Positioning works exactly as for `signature-field` (in-flow reserves space; wrap in `#place(...)` to overlay without displacement) — see the "Positioning" notes above.

### The four field types

`value:` is forwarded verbatim; the Rust adapter maps it to the AcroForm value per `type`:

**Text** — `value` is a string (numbers stringify). A blank value emits no `/V`. Set `multiline: true` for a multi-line box.

```typst
#form-field("full_name", type: "text", value: data.name)
#form-field("bio", type: "text", value: data.bio, multiline: true, height: 80pt)
```

**Checkbox** — `value` is a bool; `true` renders checked.

```typst
#form-field("agree", type: "checkbox", value: data.agree)
```

**Choice** — `value` is a string, bound only if it matches an entry in `options`.

```typst
#form-field("size", type: "choice", options: ("S", "M", "L"), value: data.size)
```

**Signature** — `value` is ignored; the widget is an unsigned SigField (Quillmark performs no cryptography — sign the output with pyHanko, Acrobat, endesive, etc.). `signature-field(name, ...)` is exactly `form-field(name, type: "signature", ...)`.

```typst
#form-field("approver", type: "signature", height: 50pt)
```

### Binding to a schema field

By default a widget's only identity is its `/T` name. Pass `field:` to additionally key the widget's region on a schema-field address, so it surfaces in the geometry sidecar (`session.regions()`) and resolves under `session.fieldAt(...)`:

```typst
#form-field("Signature", type: "signature", field: "signature_block")
```

`field:` is **region-only** — the `/T` widget name stays `name`; only the sidecar entry keys on `field:`. The address must be a real schema field: a bare field name, an array element like `"refs.2"`, or a card path built from the card's `$path` prefix (a bad address raises a Typst assert). Omit `field:` and the widget exposes no region — a click has no schema field to route to.

### Errors

- Duplicate `name` across any `form-field`/`signature-field` calls → `typst::duplicate_form_field`.
- A non-absolute `width`/`height`, a `type` outside the four values, a name violating `[A-Za-z0-9_.]+`, or a `field:` that is not a known schema address → a Typst assert pointing at `form-field`.

The label `<__qm_field__>` and metadata `kind: "__qm_field__"` are reserved for this hand-off — the same `query(metadata)` caveat noted for `signature-field` applies.

## Output Formats

PDF and SVG render as a single artifact. PNG renders one artifact per page.

Python binding (rendering lives on the engine, not the quill):

```python
from quillmark import OutputFormat
result = engine.render(quill, doc, OutputFormat.PDF)   # or .SVG, .PNG
```

WASM/JS binding (rendering lives on the engine, not the quill):

```javascript
engine.render(quill, doc, { format: 'png' });           // 144 PPI
engine.render(quill, doc, { format: 'png', ppi: 300 });  // print quality
```

PNG resolution is set via the `ppi` option (default **144** — 2× at 72pt/inch, suitable for retina previews):

| PPI | Use case |
|-----|----------|
| 72  | Low-res web thumbnails |
| 144 | Retina screen preview (2×) |
| 192 | High-DPI screen display |
| 300 | Standard print quality |
| 600 | High-quality print / archival |

## Resources

- [Typst Documentation](https://typst.app/docs/)
- [Typst Universe](https://typst.app/universe/) — package directory

## Next Steps

- [Create your own Typst Quill](creating-quills.md)
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
