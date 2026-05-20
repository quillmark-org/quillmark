~~~card-yaml
#@quill: test_quill
#@kind: main
title: Extended Metadata Demo
author: Quillmark Team
version: 1.0
~~~

This document demonstrates the **card-yaml metadata format** for Quillmark.

The format isolates structured metadata from markdown prose using `~~~card-yaml` blocks throughout your document.

## Features Demonstrated

~~~card-yaml
#@kind: features
name: Tag Directives
status: implemented
~~~

Use the `~~~card-yaml` block syntax with a `#@kind:` metadata key to create collections of related items. Each card block creates an entry in an array.

~~~card-yaml
#@kind: features
name: Structured Content
status: implemented
~~~

Break your document into logical sections with their own metadata. Perfect for catalogs, lists, and structured documents.

~~~card-yaml
#@kind: features
name: Stable Generation
status: stable
~~~

Isolating structured metadata from prose keeps LLM generation stable and prevents state corruption.

## Use Cases

~~~card-yaml
#@kind: use_cases
category: Documentation
example: Technical specifications with multiple sections
~~~

Perfect for API documentation, user manuals, and technical guides where you need structured metadata for each section.

~~~card-yaml
#@kind: use_cases
category: Content Management
example: Product catalogs, blog posts, portfolios
~~~

Ideal for content-heavy sites where each item needs its own metadata (price, category, tags, etc.).

## Technical Details

- **Card kind pattern**: `[a-z_][a-z0-9_]*`
- **Blank lines**: Allowed within card-yaml blocks
- **Card syntax**: a `~~~card-yaml` block declaring `#@kind: <kind>`, preceded by a blank line
- **Reserved names**: Cannot use `QUILL`, `CARD`, `BODY`, or `CARDS` as field names
- **Collections**: The same card kind creates an array of objects
