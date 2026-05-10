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

## Plate Files

Typst plate files are pure Typst code that access document data via a helper package:

```typst
#import "@local/quillmark-helper:0.1.0": data

#set document(title: data.at("title", default: "Untitled"))

#data.at("BODY", default: "")
```

## Data Access

Quillmark injects your document's frontmatter as JSON data via the `@local/quillmark-helper` virtual package.

### Importing the Helper

```typst
#import "@local/quillmark-helper:0.1.0": data
```

The helper provides:
- `data` - Dictionary containing all frontmatter fields, with markdown fields automatically converted to Typst content objects
- Date fields declared with `type: date` are automatically converted to Typst `datetime` values

### Accessing Fields

Access frontmatter fields directly from the `data` dictionary:

```typst
// Direct access (may error if field missing)
#data.title
#data.author

// Safe access with defaults (recommended)
#data.at("title", default: "Untitled")
#data.at("author", default: "Anonymous")
```

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

### Rendering Body Content

The document body (Markdown content after frontmatter) is stored in `data.BODY`. Markdown fields are automatically converted to Typst content objects by the helper package, so you can use them directly:

```typst
#data.at("BODY", default: "")
```

### Date Fields

Date fields are auto-converted to Typst `datetime` values by the helper package:

```typst
// Input: date: "2025-01-15" in frontmatter
#data.date  // datetime(year: 2025, month: 1, day: 15)
```

### Working with Arrays

Arrays from YAML frontmatter are accessible as Typst arrays:

```yaml
authors:
  - Alice
  - Bob
  - Charlie
```

```typst
#for author in data.authors {
  [- #author]
}
```

### Working with CARDS

If your document uses the CARDS feature, access them via `data.CARDS`:

```typst
#for card in data.at("CARDS", default: ()) {
  if card.CARD == "product" {
    [Product: #card.name - #card.BODY]
  }
}
```

## Typst Packages

Typst packages extend functionality with pre-built templates and utilities. Specify packages in `Quill.yaml`:

```yaml
typst:
  packages:
    - "@preview/appreciated-letter:0.1.0"
    - "@preview/bubble:0.2.2"
    - "@preview/fontawesome:0.5.0"
```

Then import and use them in your plate file:

```typst
#import "@local/quillmark-helper:0.1.0": data
#import "@preview/appreciated-letter:0.1.0": letter
#import "@preview/fontawesome:0.5.0": fa-icon

#show: letter.with(
  sender: data.sender,
  recipient: data.recipient,
)

#fa-icon("envelope") Contact: info@example.com
```

Browse the full catalog at [Typst Universe](https://typst.app/universe/).

## Fonts

### System Fonts

Typst can use system-installed fonts:

```typst
#set text(font: "Arial")
```

### Custom Fonts

Include custom fonts in your Quill's `assets/fonts/` directory:

```
my-quill/
└── assets/
    └── fonts/
        ├── CustomFont-Regular.ttf
        └── CustomFont-Bold.ttf
```

Reference them in your plate:

```typst
#set text(font: "CustomFont")
```

## Output Formats

The Typst backend supports three output formats:

### PDF

```python
from quillmark import OutputFormat

result = quill.render(doc, OutputFormat.PDF)
pdf_bytes = result.artifacts[0].bytes
```

### SVG

```python
result = quill.render(doc, OutputFormat.SVG)
svg_bytes = result.artifacts[0].bytes
```

SVG output is useful for web applications and scalable graphics.

### PNG

PNG renders each page to a raster image. The resolution is controlled via the `ppi` (pixels per inch) option, which defaults to **144 PPI** (2× at 72pt/inch, suitable for retina screen previews). Use 300 PPI or higher for print-quality output.

**Python**:

```python
from quillmark import OutputFormat

# Default PPI (144 — retina screen preview)
result = quill.render(doc, OutputFormat.PNG)

# One artifact per page
for i, artifact in enumerate(result.artifacts):
    artifact.save(f"page-{i}.png")
```

**JavaScript/WASM:**

```javascript
// Default PPI (144 — retina screen preview)
const result = quill.render(doc, { format: 'png' });

// Print quality at 300 PPI
const printResult = quill.render(doc, { format: 'png', ppi: 300 });

// One artifact per page
for (const artifact of printResult.artifacts) {
  console.log(artifact.mimeType);  // 'image/png'
}
```

**PPI guidelines:**

| PPI | Use case |
|-----|----------|
| 72  | Low-res web thumbnails |
| 144 | Default — retina screen preview (2×) |
| 192 | High-DPI screen display |
| 300 | Standard print quality |
| 600 | High-quality print / archival |

## Advanced Features

### Page Setup

Control page size, margins, and orientation:

```typst
#set page(
  paper: "us-letter",
  margin: (x: 1in, y: 1in),
  numbering: "1",
)
```

### Text Styling

Apply global text styles:

```typst
#set text(
  font: "Linux Libertine",
  size: 11pt,
  lang: "en",
)
```

### Paragraph Settings

Configure paragraph spacing and alignment:

```typst
#set par(
  justify: true,
  leading: 0.65em,
  first-line-indent: 1.8em,
)
```

### Custom Functions

Define reusable Typst functions:

```typst
#let highlight(content) = {
  rect(fill: yellow, inset: 8pt)[#content]
}

#highlight[Important information]
```

## Error Handling

The Typst backend provides detailed error diagnostics:

```
Compilation error at line 12, column 5:
  unknown function: `invalidFunc`
```

Errors include:
- **Syntax errors** - Invalid Typst syntax
- **Type errors** - Type mismatches in function calls
- **Package errors** - Missing or incompatible packages
- **Resource errors** - Missing fonts or assets

## Examples

### Simple Letter

```typst
#import "@local/quillmark-helper:0.1.0": data

#set page(margin: 1in)
#set text(font: "Arial", size: 11pt)

#data.date.display("[month repr:long] [day], [year]")

#data.recipient

Dear #data.recipient,

#data.at("BODY", default: "")

Sincerely,

#data.sender
```

### Academic Paper

```typst
#import "@local/quillmark-helper:0.1.0": data

#set page(paper: "a4", margin: 1in)
#set text(font: "Linux Libertine", size: 12pt)
#set par(justify: true)

#align(center)[
  #text(size: 18pt, weight: "bold")[#data.title]

  #text(size: 12pt)[#data.author]
]

#data.at("BODY", default: "")
```

## Resources

- [Typst Documentation](https://typst.app/docs/)
- [Typst Tutorial](https://typst.app/docs/tutorial/)
- [Typst Universe](https://typst.app/universe/) - Package directory
- [Typst Discord](https://discord.gg/2uDybryKPe) - Community support

## Next Steps

- [Create your own Typst Quill](creating-quills.md)
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
