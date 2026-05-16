# Main File Data Injection

> **Status**: Implemented  
> **Scope**: How parsed document data reaches main files/backends

## Overview

The **main file** is the entry-point Typst file a quill declares via
`main_file` in `Quill.yaml` — the file the backend compiles. It is distinct
from any helper or include `.typ` files a quill may also ship, which are
imported by the main file rather than compiled directly.

Quillmark does not use a template engine for main files. Data flows in two stages:

1. `Quill::compile_data()` coerces, validates, normalizes, and applies schema defaults to frontmatter, producing a plain JSON object.
2. `Backend::open()` receives that JSON and performs any backend-specific field transformations (e.g., converting markdown strings to Typst markup) before compilation.

### Data Shape

- Keys mirror normalized frontmatter fields (including `BODY` and `CARDS`)
- Defaults from the Quill schema are applied before serialization in stage 1
- Markdown-to-Typst conversion and date parsing happen in stage 2, inside the backend

## Typst Helper Package

The Typst backend injects a virtual package `@local/quillmark-helper:<version>` that exposes the JSON to the main file and provides helpers.

```typst
#import "@local/quillmark-helper:0.1.0": data

#data.title          // plain field access
#data.BODY           // BODY is automatically converted to content
#data.date           // date fields are auto-converted to datetime
```

Helper contents (generated in `backends/typst/helper.rs` from `lib.typ.template`):
- `data`: parsed JSON dictionary of all fields; a `__meta__` key injected by the backend carries the list of markdown and date fields to process, then is consumed by the helper before `data` is exposed to the main file — the main file never sees `__meta__`
- Markdown fields (`contentMediaType: text/markdown`) are auto-evaluated into Typst content; date fields (`format: date`) are converted to Typst `datetime`

## Guarantees

- The main file sees no internal shadow keys; `__meta__` is injected by the backend and consumed by the helper package before `data` is exposed
- `Quill::compile_data` returns the pre-transformation JSON (coerced + normalized + defaults); markdown/date conversion occurs inside `Backend::open` and is not separately observable
