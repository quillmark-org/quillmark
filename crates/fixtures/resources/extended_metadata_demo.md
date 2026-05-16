---
QUILL: test_quill
title: Extended Metadata Demo
author: Quillmark Team
version: 1.0
---

This document demonstrates the new **extended YAML metadata standard** for Quillmark.

The extended standard allows you to define inline metadata sections throughout your document using reserved keys.

## Features Demonstrated

```card features
name: Tag Directives
status: implemented
```

Use the ```` ```card tag_name ```` fenced syntax to create collections of related items. Each card block creates an entry in an array.

```card features
name: Structured Content
status: implemented
```

Break your document into logical sections with their own metadata. Perfect for catalogs, lists, and structured documents.

```card features
name: Backward Compatible
status: stable
```

Documents without tag directives continue to work exactly as before. No breaking changes!

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

- **Card kind pattern**: `[a-z_][a-z0-9_]*`
- **Blank lines**: Allowed within card blocks
- **Card syntax**: a fenced code block with the info string `card <kind>`, preceded by a blank line
- **Reserved names**: Cannot use `QUILL`, `CARD`, `BODY`, or `CARDS` as field names
- **Collections**: The same card kind creates an array of objects
