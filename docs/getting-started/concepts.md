# Concepts

## The Format-First Philosophy

Quillmark separates content from presentation:

- **Quills control structure and styling** — a Quill format defines layout and produces the output artifact.
- **Markdown provides content** — authors write plain Markdown with YAML frontmatter; the Quill renders it.

The same Markdown can be rendered by different Quills to produce different outputs.

## Core Components

### Quill Formats

A **Quill** is a format bundle that defines how Markdown content should be rendered. It contains:

- **Metadata** (`Quill.yaml`) - Configuration including name, backend, and field schemas
- **Plate file** - Backend-specific plate that receives document data as JSON
- **Assets** - Fonts, images, and other resources needed for rendering
- **Packages** - Backend-specific packages (e.g., Typst packages)

### YAML Frontmatter

Quillmark documents use YAML frontmatter to provide structured metadata:

```markdown
---
title: My Document
author: John Doe
date: 2025-01-15
---

# Content starts here
```

This metadata is accessible in formats and is validated against native schema rules defined in the Quill.

### Backends

A backend compiles the plate plus injected JSON data into the final artifact. The Typst backend is currently the only one — it produces PDF, SVG, and PNG, and converts fields declared `type: markdown` to Typst markup during compilation.

### Required `QUILL` Reference

Each document must declare its target format in top-level frontmatter using `QUILL`.

```markdown
---
QUILL: my_custom_format
title: My First Document
author: Jane Doe
---
```

If `QUILL` is missing, parsing fails. Quill names must be `snake_case` (`[a-z][a-z0-9_]*`); hyphens are not allowed.

## The Rendering Pipeline

Quillmark follows a three-stage pipeline:

1. **Parse & Normalize** - Extract YAML frontmatter/body, apply schema coercion/defaults, strip bidi characters, fix HTML fences
2. **Compile** - Backend receives plate content + JSON data and converts them into final artifacts (PDF, SVG, PNG, etc.)
3. **Output** - Return artifacts with metadata

```
Markdown + YAML → Parse/Normalize → Compile (Backend) → Artifacts
```

## Next Steps

- [Create your first Quill](../format-designer/creating-quills.md)
- [Learn Quill versioning](../format-designer/versioning.md)
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
- [Explore the Typst backend](../format-designer/typst-backend.md)
