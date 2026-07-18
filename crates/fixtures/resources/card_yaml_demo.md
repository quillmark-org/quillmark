~~~
$quill: test_quill
$kind: main
title: Quillmark Card-YAML Demo
author: Quillmark Team
date: 2024-01-01
version: 1.0
tags:
  - demo
  - card-yaml
  - yaml
  - markdown
description: >
  This document demonstrates Quillmark's ability to parse
  card-yaml metadata blocks and separate them from markdown content.
~~~

# Welcome to the Quillmark Card-YAML Demo

This document demonstrates the card-yaml parsing capabilities of Quillmark.

## How It Works

Quillmark parses a **`~~~` block** at the beginning of the document. The payload fields are extracted into a dictionary, while the markdown body is converted to the target format.

### Key Features

- **YAML Parsing**: Supports standard YAML syntax in the payload
- **Field Extraction**: All payload fields are available as dictionary entries
- **Body Separation**: Only the markdown body (not the payload) gets converted
- **System Metadata**: `$quill` and `$kind` are reserved `$`-prefixed keys, extracted from the YAML payload into typed metadata

## Example Usage

When you process this document with Quillmark:

1. The **payload** is parsed into fields like `title`, `author`, `date`, etc.
2. The **body** (this markdown content) is converted to the target format
3. Backends can access both the payload dictionary and the converted body

### Supported YAML Types

- **Strings**: `title: "My Document"`
- **Numbers**: `version: 1.0`
- **Arrays**: `tags: [demo, yaml]`
- **Objects**: `author: {name: "John", email: "john@example.com"}`
- **Multi-line**: Using `>` or `|` syntax

## Implementation

The parsing logic is implemented in `quillmark-core`:

```rust
use quillmark_core::Document;

let doc = Document::parse(markdown_content)?.document;
let title = doc.main().payload().get("title");
let body_content = doc.main().body();
```

This enables clean separation of concerns between document metadata and content.

---

*This document was generated to demonstrate Quillmark's card-yaml capabilities.*