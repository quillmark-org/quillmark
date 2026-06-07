# Quill Versioning System

> **Implementation**: `crates/core/src/version.rs`

## TL;DR

Quills declare a semantic `version` in `Quill.yaml`, and documents carry an optional `$quill: name@selector` reference. The selector is parsed and stored on `QuillReference`, but never **resolved** — the engine loads exactly one Quill from a path or in-memory file tree, never picking among versions. It is **enforced**: at render time the reference's two components are checked against the loaded Quill, and either a *name* mismatch or a *version* outside the selector is a hard error. The document is valid; it was paired with the wrong Quill, which is a footgun.

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

No registry consumes the selector — there is no collection of installed versions to pick from, so it is a pin, not a resolver. *Resolution* (matching `name@selector` against a set of installed versions) belongs to a higher layer; the engine loads one Quill and *enforces* the reference against it. Detection needs no registry — the engine has the loaded Quill's name and version and the document's reference — so `render` and `dry_run` both reject a mismatch with [`RenderError::QuillMismatch`](ERROR.md), carrying one diagnostic. They check in order:

- **`quill::name_mismatch`** — the reference *name* differs from the loaded Quill. The name is the prerequisite (a selector belongs to a *named* Quill), so a name mismatch short-circuits and the version is left unevaluated.
- **`quill::version_mismatch`** — names agree but the Quill's `version` falls outside the selector (e.g. `name@2` against `3.0.0`). `VersionSelector::matches` decides: `Exact` the identical version, `Minor` any patch in the `MAJOR.MINOR` series, `Major` any version in the `MAJOR` series, `Latest` (the default) anything.

Both are hard errors, applied consistently: rendering a (valid) document against a Quill it was not written for — a different format, or an incompatible version of one — yields undefined output, so it is rejected rather than warned. The check runs before schema validation and compilation, where diagnostics computed against the wrong Quill would be noise. `QuillMismatch` is distinct from `ValidationFailed` (a malformed document): the remedy is to render with the referenced Quill or amend `$quill`, not to edit a field. A bare name or `@latest` matches any version, so correctly-targeted documents never trip either check.

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

Three distinct failure paths:

- **`Quill.yaml` version invalid** → `quill::invalid_version` diagnostic → surfaces as `RenderError::QuillConfig` at Quill load.
- **Document `$quill` reference invalid** (e.g. `my_format@bad`) → `ParseError::InvalidQuillReference`, returned directly by the parser, never as `RenderError::QuillConfig`.
- **Loaded Quill does not satisfy a well-formed `$quill`** (wrong name, or version outside the selector — e.g. `my_format@2` against a `3.0.0` Quill) → `quill::name_mismatch` / `quill::version_mismatch` diagnostic → surfaces as `RenderError::QuillMismatch` from `render`/`dry_run`.

See [ERROR.md](ERROR.md) for error patterns.

## Links

- [QUILL.md](QUILL.md) — Quill structure
- [ERROR.md](ERROR.md) — error patterns
