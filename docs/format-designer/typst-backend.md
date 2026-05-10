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
  if card.CARD == "product" [
    Product: #card.name — #card.BODY
  ]
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
