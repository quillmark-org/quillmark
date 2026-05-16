# quillmark-fixtures

Test fixtures and sample Quill templates for [Quillmark](https://github.com/nibsbin/quillmark).

## Overview

This crate contains sample Quill templates and markdown files used for testing and examples in the Quillmark ecosystem. It provides helper functions for accessing fixture paths programmatically in Rust projects.

## Usage

Add the crate as a dev-dependency and use the provided helper functions to access fixture paths:

```rust
// Access a resource file by name
let sample_md = quillmark_fixtures::resource_path("sample.md");

// Access a versioned quill template (resolves to the latest version automatically)
let usaf_memo = quillmark_fixtures::quills_path("usaf_memo");
```

## Available Resources

The package includes:

- **Quill Templates**: Sample Quill templates under `resources/quills/`, each with `plate.typ`, `Quill.yaml`, and assets (versioned subdirectories, e.g. `0.1.0/`)
  - `quills/usaf_memo/` - US Air Force memo template
  - `quills/taro/` - Custom template example
  - `quills/classic_resume/` - Classic resume template
  - `quills/cmu_letter/` - CMU letter template

- **Legacy Quill Template** (unversioned, directly under `resources/`)
  - `appreciated_letter/` - A formal letter template (uses `glue.typ` instead of `plate.typ`)

- **Sample Markdown Files**: Example markdown files for testing
  - `sample.md` - Basic markdown example
  - `frontmatter_demo.md` - Demonstrates YAML frontmatter
  - `extended_metadata_demo.md` - Extended metadata examples
  - `*.md` - Various markdown test files

## License

Licensed under the MIT License. See LICENSE for details.
