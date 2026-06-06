# Quill Versioning System

> **Implementation**: `crates/core/src/version.rs`

## TL;DR

Quills declare a semantic `version` in `Quill.yaml`, and documents carry an optional `$quill: name@selector` reference. The version selector is parsed and stored on `QuillReference` but is **not** resolved at runtime — the engine loads exactly one Quill from a path or in-memory file tree, and the only runtime check on `$quill` compares the *name* against the loaded Quill.

## Version Format

Semantic versioning: `MAJOR.MINOR.PATCH`. Two-segment `MAJOR.MINOR` passes validation in `Quill.yaml` (the raw string is stored as-is; no normalization occurs).

| Increment | When |
|-----------|------|
| **MAJOR** | Breaking changes: layout changes, removed fields, incompatible types |
| **MINOR** | New optional fields, enhancements (backward-compatible) |
| **PATCH** | Bug fixes, corrections (backward-compatible) |

## Document Syntax

The version selector rides on the root block's `$quill` system-metadata line (see [markdown-spec.md](../references/markdown-spec.md) §3.3):

```
$quill: my_format@2.1.0    # exact
$quill: my_format@2.1      # 2.1.x
$quill: my_format@2        # 2.x.x
$quill: my_format@latest   # latest (explicit)
$quill: my_format          # latest (default)
```

The selector parses into the `VersionSelector` on `QuillReference`. No registry consumes it: there is no collection of installed versions to match against, and the selector is never compared at render time. Treat it as an informational pin. The engine emits a `quill::ref_mismatch` warning only when the reference *name* differs from the loaded Quill; the selector is ignored.

## Quill.yaml

```yaml
quill:
  name: my_format
  version: "2.1.0"
  backend: typst
  description: "Short description of this format"
  author: "..."          # optional
  plate_file: "plate.typ" # optional; conventional name
  ui: { ... }            # optional
```

`name`, `backend`, `version`, and `description` are required. `author`, `plate_file`, and `ui` are optional. Unknown keys under `quill:` are a hard error. `version` must parse as `MAJOR.MINOR.PATCH` (or `MAJOR.MINOR`); an invalid or missing value fails at load.

## Error Handling

Two distinct failure paths:

- **`Quill.yaml` version invalid** → `quill::invalid_version` diagnostic → surfaces as `RenderError::QuillConfig` at Quill load.
- **Document `$quill` reference invalid** (e.g. `my_format@bad`) → `ParseError::InvalidQuillReference`, returned directly by the parser, never as `RenderError::QuillConfig`.

See [ERROR.md](ERROR.md) for error patterns.

## Links

- [QUILL.md](QUILL.md) — Quill structure
- [ERROR.md](ERROR.md) — error patterns
