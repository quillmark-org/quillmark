# Document Storage Serialization

> **Status**: Implemented
> **Implementation**: `crates/core/src/document/dto.rs`

## Overview

`Document` is the typed in-memory model of a Quillmark Markdown file. Its
layout tracks the evolving Quillmark model and is **not** a stable interface.
To persist documents — e.g. in a database — without storing Markdown (whose
syntax also evolves), `Document` serializes to a **versioned JSON envelope**,
`StoredDocument`, whose wire format is frozen per schema version.

## When to use it

| Form | Round-trips? | Stable for storage? |
|---|---|---|
| Markdown (`Document::to_markdown`) | Yes | No — syntax evolves |
| Plate JSON (`Document::to_plate_json`) | No — lossy, one-way export to backends | — |
| `StoredDocument` JSON | Yes — lossless | Yes — frozen per schema version |

Use `StoredDocument` JSON whenever a `Document` must survive a process
restart or a crate upgrade: database rows, caches, message payloads.

## Design Principles

1. **Versioned envelope** — every blob carries a `schema` tag; readers
   dispatch on it and reject unknown versions.
2. **Frozen DTO per version** — each schema version has its own standalone
   type tree (`DocumentV0_81_0`, `CardV0_81_0`, …). These are never changed
   once shipped.
3. **Decoupled from the live model** — internal refactors of `Document` and
   its components only touch conversion code, never the wire format.
4. **Transparent API** — `Document` serializes through the envelope via
   `#[serde(into / try_from)]`; callers just use `serde_json`.

## The Format

```json
{
  "schema": "quillmark/document@0.81.0",
  "main":  { "sentinel": { ... }, "frontmatter": { ... }, "body": "..." },
  "cards": [ ... ]
}
```

`StoredDocument` is an internally-tagged enum (`#[serde(tag = "schema")]`);
each variant carries a frozen DTO tree. Quill references are stored as
strings (parsed back via `QuillReference::from_str`). Parse-time `warnings`
are excluded — they are observations about source text, not document
content, and are repopulated on the next parse.

```rust
use quillmark_core::Document;

let doc = Document::from_markdown(src)?;
let json = serde_json::to_string(&doc)?;          // store
let restored: Document = serde_json::from_str(&json)?;  // load
assert_eq!(doc, restored);
```

## Schema Versioning

The schema tag is tied to the **crate version at which the `Document` model
was last changed** — not the running crate version. The current model was
fixed in `0.81.0`, so the tag is `quillmark/document@0.81.0`; every `0.81.x`
patch release writes that same tag, because patches do not change the model.

`0.81.0` is the **baseline** schema: it has no predecessor and requires no
migration. The first migration work occurs at the next model change.

## Adding a Schema Version

When the `Document` model changes (planned for `0.82.0`):

1. **Freeze** the `DocumentV0_81_0` type tree — leave its struct/enum
   definitions and serde derives untouched so existing rows still parse.
2. **Remove** the conversions binding the old DTO to the *live* `Document`
   (`From<&Document>` and `TryFrom<… for Document>`); they no longer compile
   and are superseded below.
3. **Add** a new frozen tree `DocumentV0_82_0` reflecting the new model, plus
   its `From<&Document>` and `TryFrom<… for Document>` conversions.
4. **Add** the `StoredDocument::V0_82_0` variant, tagged
   `#[serde(rename = "quillmark/document@0.82.0")]`.
5. **Write the migration** — `From<DocumentV0_81_0> for DocumentV0_82_0`.
   This is the only real labor: it encodes how old fields map to the new
   model (renames, restructures, defaults for new fields).
6. **Extend** the reader:
   ```rust
   match stored {
       StoredDocument::V0_82_0(p) => Document::try_from(p),
       StoredDocument::V0_81_0(p) => Document::try_from(DocumentV0_82_0::from(p)),
   }
   ```

Old and new DTOs **coexist permanently** in `dto.rs`. Migrations chain
(`V0_81_0 → V0_82_0 → …`); only the newest DTO converts to the live
`Document`, so each migration step stays small as versions accumulate. The
cost of this design is one frozen type tree per schema version plus one
migration function per version bump; the benefit is that a row written by
any past version always loads.

## Gotchas

- The schema tag is a hand-set constant (`SCHEMA_V0_81_0`), **not**
  `CARGO_PKG_VERSION` — bumping it is a deliberate act tied to a model change.
- Unknown schema tags are rejected on read, never silently ignored.
- `warnings` are dropped on serialize and default to empty on deserialize.
- DTO type names carry version suffixes with underscores
  (`DocumentV0_81_0`); `non_camel_case_types` is allowed module-wide for this.

## Links

- [ARCHITECTURE.md](ARCHITECTURE.md) — `Document` in the core type overview
- [MARKDOWN.md](MARKDOWN.md) — Markdown syntax and the in-memory data model
- [VERSIONING.md](VERSIONING.md) — quill version resolution (a separate concern)
- [QUILL_VALUE.md](QUILL_VALUE.md) — value type stored inside frontmatter fields
