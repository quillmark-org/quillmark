# Quill Versioning System

> **Status**: Implemented
> **Implementation**: `crates/core/src/version.rs`, `crates/quillmark/src/orchestration/engine.rs`

## Version Format

Semantic versioning: `MAJOR.MINOR.PATCH` (two-segment `MAJOR.MINOR` also accepted, treated as `MAJOR.MINOR.0`).

| Increment | When |
|-----------|------|
| **MAJOR** | Breaking changes: layout changes, removed fields, incompatible types |
| **MINOR** | New optional fields, enhancements (backward-compatible) |
| **PATCH** | Bug fixes, corrections (backward-compatible) |

## Document Syntax

```yaml
QUILL: "my_format@2.1.0"    # exact version
QUILL: "my_format@2.1"      # latest 2.1.x
QUILL: "my_format@2"        # latest 2.x.x
QUILL: "my_format@latest"   # latest overall (explicit)
QUILL: "my_format"          # latest overall (default)
```

## Resolution

Given versions `[1.0.0, 1.0.1, 1.1.0, 2.0.0, 2.1.0, 2.1.1, 3.0.0]`:

| Selector | Resolves To |
|----------|-------------|
| `@3` | `3.0.0` |
| `@2` | `2.1.1` |
| `@2.1` | `2.1.1` |
| `@2.1.0` | `2.1.0` |
| `@latest` | `3.0.0` |
| (none) | `3.0.0` |

## Quill.yaml

`version` and `description` are both required:

```yaml
quill:
  name: my_format
  version: "2.1.0"
  backend: typst
  description: "Short description of this format"
```

`version` must parse as `MAJOR.MINOR.PATCH` (or `MAJOR.MINOR`); an invalid or missing value is a hard error at load time.

## Error Handling

Version parse errors surface as `RenderError::QuillConfig` with a descriptive message. There is no runtime version-registry lookup — each Quill is loaded directly from a path or in-memory file tree.

See [ERROR.md](ERROR.md) for error handling patterns.

## Links

- [QUILL.md](QUILL.md) — Quill structure
- [ERROR.md](ERROR.md) — error patterns
