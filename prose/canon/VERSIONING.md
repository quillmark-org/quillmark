# Quill Versioning System

> **Implementation**: `crates/core/src/version.rs`

## TL;DR

Quills declare a semantic `version` in `Quill.yaml`, and documents carry an optional `$quill: name@selector` reference. The selector is parsed and stored on `QuillReference`, but never **resolved** — the engine loads exactly one Quill from a path or in-memory file tree, never picking among versions. It is **enforced**: at render time the reference's two components are checked against the loaded Quill, and either a *name* mismatch or a *version* outside the selector is a hard error. The document is valid; it was paired with the wrong Quill, which is a footgun.

## Version Format

Semantic versioning: `MAJOR.MINOR.PATCH` (two-segment `MAJOR.MINOR` also validates). Always quote the value in `Quill.yaml`: an unquoted `1.0` is read as a YAML number and stringified to `"1"`, which fails validation.

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

`QuillMismatch` is distinct from `ValidationFailed` (a malformed document): here the document is well-formed but paired with the wrong Quill, so the remedy is to render with the referenced Quill or amend `$quill`. A bare name or `@latest` matches any version, so correctly-targeted documents never trip either check.

## Quill.yaml

```yaml
quill:
  name: my_format
  version: "2.1.0"
  backend: typst
  description: "Short description of this format"
  author: "..."          # optional
  ui: { ... }            # optional

typst:
  plate_file: "plate.typ" # optional; the Typst template, read by the backend
```

`name`, `backend`, `version`, and `description` are required. `author` and `ui` are optional. Unknown keys under `quill:` are a hard error. A backend's own settings (e.g. the Typst `plate_file`) live under the backend-named section, not in `quill:`. `version` must parse as `MAJOR.MINOR.PATCH` (or `MAJOR.MINOR`); an invalid or missing value fails at load.

## Error Handling

Three distinct failure paths:

- **`Quill.yaml` version invalid** → `quill::invalid_version` diagnostic → surfaces as `RenderError::QuillConfig` at Quill load.
- **Document `$quill` reference invalid** (e.g. `my_format@bad`) → `ParseError::InvalidQuillReference`, returned directly by the parser, never as `RenderError::QuillConfig`.
- **Loaded Quill does not satisfy a well-formed `$quill`** (wrong name, or version outside the selector — e.g. `my_format@2` against a `3.0.0` Quill) → `quill::name_mismatch` / `quill::version_mismatch` diagnostic → surfaces as `RenderError::QuillMismatch` from `render`/`dry_run`.

See [ERROR.md](ERROR.md) for error patterns.

## Ref Immutability

A canonical ref (`name@version`) is **immutable content**, at least within the
lifespan of a runtime: once any layer has materialized a Quill for a ref, the
content behind that ref never changes for that process. Publishing different
content requires a version bump.

Every cache between a document and its rendered output keys on this invariant,
and none of them exposes an invalidation API — **by design**:

- quiver's quill cache holds one `Quill` per canonical ref for the `Quiver`
  instance's lifetime;
- app-level services cache that same instance per canonical ref;
- the wasm `Engine` caches backend-memory clones in a `WeakMap` keyed on the
  canonical `Quill` instance, so a clone's lifetime follows the instance.

"Invalidate" therefore means *replace the instance* — a new `Quill` at a new
ref, or a new `Quiver` — and the downstream caches follow automatically
(WeakMap + weak refs). There is no invalidation API. One must arrive
end-to-end with its first real consumer (republish-at-same-ref), which this
immutability invariant deliberately rules out.

## Links

- [QUILL.md](QUILL.md) — Quill structure
- [ERROR.md](ERROR.md) — error patterns
