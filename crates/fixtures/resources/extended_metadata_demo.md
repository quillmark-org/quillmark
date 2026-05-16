---
QUILL: test_quill
title: Extended Metadata Demo
author: Quillmark Team
version: 1.0
---

This document demonstrates the **extended YAML metadata standard** for Quillmark.

The extended standard allows you to define inline metadata records throughout your document using `` ```card `` fenced code blocks.

## Features Demonstrated

```card features
name: Tag Directives
status: implemented
```

Use `` ```card <name> `` fenced code blocks to create collections of related items. Each block creates an entry in an array keyed by its kind.

```card features
name: Structured Content
status: implemented
```

Break your document into logical sections with their own metadata. Perfect for catalogs, lists, and structured documents.

```card features
name: Backward Compatible
status: stable
```

Documents without card blocks continue to work exactly as before.

## Use Cases

```card use_cases
category: Documentation
example: Technical specifications with multiple sections
```

Perfect for API documentation, user manuals, and technical guides where you need structured metadata for each section.

```card use_cases
category: Content Management
example: Product catalogs, blog posts, portfolios
```

Ideal for content-heavy sites where each item needs its own metadata (price, category, tags, etc.).

## Technical Details

- **Kind pattern**: `[a-z_][a-z0-9_]*`
- **Fence shape**: CommonMark fenced code block with info string `card`
- **Reserved names**: `BODY`, `CARDS` are populated by the parser and forbidden as input keys
- **Collections**: Same `KIND` value groups blocks into an ordered array under `cards.<kind>`
