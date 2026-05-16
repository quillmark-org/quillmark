# Typst Backend

The Typst backend generates PDF, SVG, and PNG documents using the [Typst](https://typst.app/) typesetting system. It converts Markdown frontmatter fields to Typst markup, injects them into the plate as JSON via a helper package, and compiles to the requested format.

## Basic Usage

Specify `backend: typst` in your `Quill.yaml`:

```yaml
quill:
  name: my_typst_quill
  version: "1.0.0"
  backend: typst
  description: Document format using Typst
  plate_file: plate.typ

typst:
  packages:
    - "@preview/appreciated-letter:0.1.0"
```

## Data Access

Plates are plain Typst code. Frontmatter reaches the plate as a JSON dictionary exposed by the virtual `@local/quillmark-helper` package:

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.title                                  // direct — errors if missing
#data.at("title", default: "Untitled")       // safe with default
```

Fields declared `type: markdown` in `Quill.yaml` arrive as Typst content (ready to render); `type: date` fields arrive as Typst `datetime` values.

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

The document body is at `data.BODY`. Arrays come through as Typst arrays. Cards live under `data.CARDS`, each carrying its own `CARD` discriminator, fields, and `BODY`:

```typst
#data.at("BODY", default: "")

#for author in data.authors [- #author]

#for card in data.at("CARDS", default: ()) {
  if card.CARD == "product" {
    [Product: #card.name — #card.BODY]
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

System-installed fonts work out of the box (`#set text(font: "Arial")`). To bundle fonts with the Quill, drop them in `assets/fonts/`:

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

See the [Typst tutorial](https://typst.app/docs/tutorial/) for the full styling vocabulary. For worked plates that combine data access with real layout, see the `appreciated_letter`, `usaf_memo`, and `taro` examples in `crates/quillmark/examples/`.

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
| `name` | `str` | required (positional) | Field name — must be unique within the document and match `[A-Za-z0-9_]+`. Surfaces as the widget's `/T` entry. |
| `width` | `length` | `200pt` | Must be an absolute length (`pt`, `mm`, `cm`, `in`) — relative lengths like `2em` or `50%` are rejected. |
| `height` | `length` | `50pt` | Same constraint as `width`. |

### Errors

- Two calls with the same `name` raise a compilation error (`typst::duplicate_signature_field`).
- A non-absolute `width` or `height` raises a Typst assert pointing at `signature-field`.
- Names violating `[A-Za-z0-9_]+` raise a Typst assert.

The label `<__qm_sig__>` and metadata `kind: "__qm_sig__"` are reserved for this hand-off — don't use them for unrelated metadata in your plate.

## Output Formats

PDF and SVG render as a single artifact. PNG renders one artifact per page.

```python
from quillmark import OutputFormat
result = quill.render(doc, OutputFormat.PDF)   # or .SVG, .PNG
```

PNG resolution is set via the `ppi` option (default **144** — 2× at 72pt/inch, suitable for retina previews):

```javascript
quill.render(doc, { format: 'png' });             // 144 PPI
quill.render(doc, { format: 'png', ppi: 300 });   // print quality
```

| PPI | Use case |
|-----|----------|
| 72  | Low-res web thumbnails |
| 144 | Default — retina screen preview (2×) |
| 192 | High-DPI screen display |
| 300 | Standard print quality |
| 600 | High-quality print / archival |

## Resources

- [Typst Documentation](https://typst.app/docs/)
- [Typst Universe](https://typst.app/universe/) — package directory

## Next Steps

- [Create your own Typst Quill](creating-quills.md)
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
