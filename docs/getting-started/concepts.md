# Concepts

## How Quillmark separates concerns

Quillmark separates content, schema, and presentation:

- **A Quill's schema drives the data** — it validates the card-yaml metadata and applies coercion, defaults, and scaffolding. The schema is the contract every render consumes.
- **A Quill's plate controls presentation** — it defines layout and typesets the output artifact.
- **Markdown provides content** — authors write plain Markdown with card-yaml blocks; the Quill renders it into a fully typeset document.

## Core Components

### Quill Formats

A **Quill** is a format bundle that defines how Markdown content should be rendered. It contains:

- **Metadata** (`Quill.yaml`) - Configuration including name, backend, and field schemas
- **Plate file** - Backend-specific plate that receives document data as JSON
- **Assets** - Fonts, images, and other resources needed for rendering

### card-yaml Blocks

Quillmark documents use **card-yaml blocks** to provide structured metadata. A
card-yaml block is delimited by bare `~~~` / `~~~` fences and may begin
with a run of `$`-prefixed system metadata lines followed by a YAML payload.
(`~~~card-yaml` is also accepted as a non-canonical alias; the
canonical opener is a bare `~~~`. To write a literal fenced *code* block in
prose, use a backtick fence or a `~~~` fence with a language info string —
adding more tildes does not escape, as a `~~~~` block is still a card.)

```markdown
~~~
$quill: my_format
$kind: main
title: My Document
author: John Doe
date: 2025-01-15
~~~

# Content starts here
```

This metadata is accessible in formats and is validated against native schema rules defined in the Quill. See [card-yaml Blocks](../authoring/card-yaml.md) for the full syntax.

### Backends

A backend compiles the plate plus injected JSON data into the final artifact. The Typst backend is currently the only one — it produces PDF, SVG, and PNG, and converts fields declared `type: markdown` to Typst markup during compilation.

### Required `$quill` Metadata

Each document must declare its target format in the root block's `$quill` system metadata line. If missing, parsing fails. Quill names must be `snake_case` (`[a-z_][a-z0-9_]*`); hyphens are not allowed.

## The Rendering Pipeline

Quillmark follows a three-stage pipeline:

1. **Parse & Normalize** - Extract card-yaml blocks and body prose; apply schema coercion/defaults, strip bidi characters, fix HTML fences. Absent fields are zero-filled in the backend projection (never persisted) — partial documents are always renderable.
2. **Compile** - Backend receives plate content + JSON data and converts them into final artifacts (PDF, SVG, PNG, etc.)
3. **Output** - Return artifacts with metadata

```
Markdown + YAML → Parse/Normalize → Compile (Backend) → Artifacts
```

## Next Steps

- [Create your first Quill](../quills/creating-quills.md)
- [Learn Quill versioning](../quills/versioning.md)
- [Learn about Markdown syntax](../authoring/markdown-syntax.md)
- [Explore the Typst backend](../quills/typst-backend.md)
