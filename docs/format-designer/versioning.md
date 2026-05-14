# Quill Versioning

Versioning helps format designers evolve Quills safely while keeping document rendering predictable.

## Version Field in `Quill.yaml`

Each Quill must declare a semantic version. Both `version` and `description` are required fields:

```yaml
quill:
  name: my_quill
  backend: typst
  description: A professional document format
  version: "1.2.0"
```

Quill names must be `snake_case` (lowercase letters, digits, and underscores only — hyphens are not allowed).

Use semantic versioning (`MAJOR.MINOR.PATCH`) to communicate compatibility:

- **MAJOR**: breaking changes to fields, leaves, or expected document shape
- **MINOR**: backward-compatible additions (new optional fields, non-breaking behavior)
- **PATCH**: fixes and small improvements without shape changes

## How Authors Select Versions

Authors can target versions through the `QUILL` key in frontmatter:

```markdown
---
QUILL: my_quill@1.2
title: Quarterly Report
---
```

Supported selectors:

| Selector | Meaning |
|---|---|
| `my_quill` | Latest available version |
| `my_quill@latest` | Latest available version (explicit) |
| `my_quill@1` | Latest 1.x.x |
| `my_quill@1.2` | Latest 1.2.x |
| `my_quill@1.2.0` | Exact version |

## Practical Guidelines

1. Start at `1.0.0` for your first stable internal format release.
2. Increase versions on every format change, even if small.
3. Treat field renames/removals as breaking (`MAJOR`) changes.
4. Prefer additive changes (new optional fields/leaves) to reduce migration work.
5. Keep example documents updated for the latest major/minor versions.

## Related Pages

- [Creating Quills](creating-quills.md)
- [Quill.yaml Reference](quill-yaml-reference.md)
- [YAML Frontmatter](../authoring/yaml-frontmatter.md)
