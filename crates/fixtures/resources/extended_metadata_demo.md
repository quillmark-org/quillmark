---
QUILL: test_quill
title: Extended Metadata Demo
author: Quillmark Team
version: 1.0
---

This document demonstrates the **extended YAML metadata standard** for Quillmark.

The extended standard allows you to define inline metadata records throughout your document using `` ```leaf `` fenced code blocks.

## Features Demonstrated

```leaf
KIND: features
name: Tag Directives
status: implemented
```

Use `` ```leaf `` fenced code blocks with `KIND: <name>` to create collections of related items. Each block creates an entry in an array keyed by `KIND`.

```leaf
KIND: features
name: Structured Content
status: implemented
```

Break your document into logical sections with their own metadata. Perfect for catalogs, lists, and structured documents.

```leaf
KIND: features
name: Backward Compatible
status: stable
```

Documents without leaf blocks continue to work exactly as before.

## Use Cases

```leaf
KIND: use_cases
category: Documentation
example: Technical specifications with multiple sections
```

Perfect for API documentation, user manuals, and technical guides where you need structured metadata for each section.

```leaf
KIND: use_cases
category: Content Management
example: Product catalogs, blog posts, portfolios
```

Ideal for content-heavy sites where each item needs its own metadata (price, category, tags, etc.).

## Technical Details

- **Kind pattern**: `[a-z_][a-z0-9_]*`
- **Fence shape**: CommonMark fenced code block with info string `leaf`
- **Reserved names**: `BODY`, `LEAVES` are populated by the parser and forbidden as input keys
- **Collections**: Same `KIND` value groups blocks into an ordered array under `leaves.<kind>`
