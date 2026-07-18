# Document Storage Serialization

> **Implementation**: `crates/core/src/document/`

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
| `StoredDocument` JSON | Yes — lossless | Yes — frozen per schema version |

Use `StoredDocument` JSON whenever a `Document` must survive a process
restart or a crate upgrade: database rows, caches, message payloads.

`Document::to_plate_json` also exists as a lossy, one-way export to
Plate-shaped backends; it is core-only (not exposed by the WASM or Python
bindings) and never a storage option.

## Design Principles

1. **Versioned envelope** — every blob carries a `schema` tag; readers
   dispatch on it and reject unknown versions.
2. **Frozen DTO per version** — each schema version has its own standalone
   type tree (`DocumentV0_81_0`, `CardV0_81_0`, …). These are never changed
   once shipped.
3. **Decoupled from the live model** — internal refactors of `Document` and
   its components only touch conversion code, never the wire format.
4. **Transparent API** — `Document` serializes through the envelope via
   `#[serde(into / try_from)]`; callers use `serde_json` directly.

## The Format

The current schema (`quillmark/document@0.93.0`) carries each card's full
ordered payload — typed `$` system metadata, user fields, and YAML
comments interleaved in source order — as a single discriminated-union
item list. This is what makes inline-comment preservation symmetric across
the `$`/non-`$` boundary. The payload shape is unchanged since `0.92.0`;
`body` is the canonical richtext corpus embedded structurally (a nested
object, not a markdown string) — see Byte-stability.

```json
{
  "schema": "quillmark/document@0.93.0",
  "main": {
    "payload": {
      "items": [
        { "type": "quill", "value": "usaf_memo@0.1" },
        { "type": "kind",  "value": "main" },
        { "type": "ext",   "value": { "presentation": { "title": "Greeting Card" } } },
        { "type": "field", "key": "title", "value": "Hi" }
      ]
    },
    "body": { "islands": [], "lines": [ { "containers": [], "kind": "para" } ], "marks": [], "text": "Hi" }
  },
  "cards": [ ... ]
}
```

`StoredDocument` is an internally-tagged enum (`#[serde(tag = "schema")]`);
each variant carries a frozen DTO tree. Quill references are stored as
strings (parsed back via `QuillReference::from_str`). The discriminator on
payload items is `type` (not `kind`) to keep it unambiguous next to the
`$kind` metadata semantic. The full variant set is `quill | kind | id |
ext | seed | field | comment`; the `ext` and `seed` variants carry the
`$ext` / `$seed` maps verbatim and are stripped from `to_plate_json()`
before backends see it.
Parse-time warnings live on `Document` (`warnings: Vec<Diagnostic>`) but
are excluded from `PartialEq` and not serialized, so they never reach this
format.

### Legacy schema (V0_81_0)

Documents written by `quillmark-core` `0.81.x` carry
`"schema": "quillmark/document@0.81.0"` and a separate `sentinel` + `frontmatter`
shape. Readers accept them and migrate forward to V0_82_0 on load via
`From<DocumentV0_81_0> for DocumentV0_82_0`; writers do not produce this
shape. The migration is structural — no defaults are invented and no
field-level information is dropped — so a `0.81.x`-stored document and the
same document re-parsed from its Markdown source produce equal `Document`
values.

## Byte-stability

Serialization is **byte-deterministic** within a given schema version:
equal `Document`s (by `PartialEq`) produce byte-equal JSON, and the same
document re-serialized in any later patch or minor release tagged with
the same `schema` produces the same bytes. This is load-bearing for
consumers that content-hash stored documents (template-divergence
detection, cache keys).

**Two disciplines in one envelope.** The outer envelope — struct field
order, the `cards` array, payload field values — stays compact,
insertion-ordered `serde_json`: `serde_json::Value` inside payload field
values keeps YAML insertion order via the workspace's
`serde_json/preserve_order` feature, and no whole-envelope key sort is
applied. Every `body` subtree, by contrast, is the recursively key-sorted
**canonical richtext form** (`CanonicalRichText` in `dto.rs`) — byte-identical
to `rt.to_canonical_json()` and independent of `preserve_order`, even in a
consumer crate graph that lacks the feature. Sortedness is semantic
*inside* the corpus (mark/island/attribute order carries no meaning, so the
serializer commits to one bit pattern); insertion order is semantic
*outside* it (payload item order is source order, and matters).

The guarantee follows from: struct field order is fixed in the frozen
DTO tree; `Vec` fields preserve order by definition; the two disciplines
above each hold at their respective level. No whitespace normalization is
applied — the output is `serde_json`'s compact form otherwise. Bumping the
`schema` version is the only event that may change the byte layout of a
document written by the current writer.

**Migrated rows: a conditional caveat.** The guarantee above is unconditional
for a document the current writer serializes directly. A row still carrying
a legacy schema tag migrates forward on read, and the `0.92.0 → 0.93.0` hop
cold-imports the stored markdown `body` string through the same
Markdown → richtext path `Document::parse` uses. Byte-stability of
*that* row across a crate upgrade is therefore conditional on
`pulldown-cmark` parsing the body the same way: a forced parser or security
bump can move the migrated bytes even though the schema tag does not
change. Two ways to manage this:

- **Read-repair.** Rewrite a row under its current schema tag once it has
  been read and migrated, so the corpus form — not the legacy markdown
  string — becomes its byte-stable resting state.
- **Accept the movement.** For rows left un-repaired, treat a forced
  parser/security bump as either a schema-version event (if a hard
  guarantee is required) or an accepted, logged hash movement on
  not-yet-migrated rows.

## Schema Versioning

The schema version is tied to the **crate version at which the `Document`
wire format was last changed** — not the running crate version. The
current format was fixed in `0.93.0`, so the version tag is
`quillmark/document@0.93.0`; every later patch release writes that same
value, because patches do not change the format.

The first schema version was `0.81.0`. `0.82.0` migrated `Document` to a
unified payload-item list (typed `$` entries living alongside user fields
and comments in a single `Vec<PayloadItem>` instead of a separate
`sentinel + frontmatter` pair). `0.92.0` added two things to the `Field`/
payload model: a per-field `nested_fills` list on the `Field` item, so
`!must_fill` markers nested inside a field value survive a storage
round-trip (the JSON `value` projection is fill-free), and the `seed`
payload-item variant (the `$seed` per-card-kind overlay map). `0.93.0`
leaves the payload model unchanged and instead embeds the card `body` as
the **canonical richtext corpus** — structurally, as a nested object, not a
markdown string (see Byte-stability).

The V0_82_0 → V0_92_0 migration is structural — old payload items map 1:1,
`nested_fills` defaults to empty (no 0.82.0 document carried nested
markers) and the new `seed` variant is never produced from an older blob.
The V0_92_0 → V0_93_0 migration is the one hop that can fail: it
cold-imports the stored markdown `body` string through the same
Markdown → richtext path `Document::parse` uses, so a
pathologically over-nested legacy body is rejected
(`StorageError::Malformed`) rather than silently truncated. Migrations
chain on read (`V0_81_0 → V0_82_0 → V0_92_0 → V0_93_0`), with only the
newest DTO converting to the live `Document`.

## Adding a Schema Version

When the `Document` wire format changes again:

1. **Freeze** the current `DocumentV0_93_0` type tree — leave its struct
   /enum definitions and serde derives untouched so existing rows still parse.
2. **Remove** the conversions binding the old DTO to the *live* `Document`
   (`From<&Document>` and `TryFrom<… for Document>`); they no longer compile
   and are superseded below.
3. **Add** a new frozen tree `DocumentV0_NN_0` reflecting the new model,
   plus its `From<&Document>` and `TryFrom<… for Document>` conversions.
4. **Add** the `StoredDocument::V0_NN_0` variant, tagged
   `#[serde(rename = "quillmark/document@0.NN.0")]`.
5. **Write the migration** — `From<DocumentV0_93_0> for DocumentV0_NN_0` if
   the mapping cannot fail (a purely structural rename/restructure), or
   `TryFrom<DocumentV0_93_0> for DocumentV0_NN_0` if it can reject, as the
   V0_92_0 → V0_93_0 cold-import does for an over-nested legacy body. This is
   the only real labor: it encodes how old fields map to the new model
   (renames, restructures, defaults for new fields, and — for a `TryFrom`
   hop — which malformed inputs get rejected).
6. **Extend** the reader (each older blob migrates one hop, then chains).
   Every arm below the newest already funnels through the V0_92_0 → V0_93_0
   hop, which can reject — so every one of those arms threads `?`, whether
   or not the new V0_93_0 → V0_NN_0 hop (shown here as infallible) adds
   another:
   ```rust
   match stored {
       StoredDocument::V0_NN_0(p) => Document::try_from(p),
       StoredDocument::V0_93_0(p) => Document::try_from(DocumentV0_NN_0::from(p)),
       StoredDocument::V0_92_0(p) => Document::try_from(DocumentV0_NN_0::from(
           DocumentV0_93_0::try_from(p)?,
       )),
       StoredDocument::V0_82_0(p) => Document::try_from(DocumentV0_NN_0::from(
           DocumentV0_93_0::try_from(DocumentV0_92_0::from(p))?,
       )),
       StoredDocument::V0_81_0(p) => Document::try_from(DocumentV0_NN_0::from(
           DocumentV0_93_0::try_from(DocumentV0_92_0::from(DocumentV0_82_0::from(p)))?,
       )),
   }
   ```
   If the new hop is itself a `TryFrom`, thread a second `?` after
   `DocumentV0_NN_0::try_from(...)` in every arm.

A new frozen DTO can also reject at parse time through a custom
`Deserialize` rather than through a `TryFrom` migration — `CanonicalRichText`
(the `body` field's type) normalizes and validates the embedded corpus,
failing with a serde error before any `TryFrom` in the chain above runs.
Design a new DTO's `Deserialize` to fail the same way if it embeds
structured (non-string) data of its own.

Old and new DTOs **coexist permanently** in `dto.rs`. Migrations chain
(`V0_81_0 → V0_82_0 → V0_92_0 → V0_93_0 → V0_NN_0 → …`); only the newest DTO
converts to the live `Document`, so each migration step stays small as
versions accumulate. The cost of this design is one frozen type tree per
schema version plus one migration function per version bump; the benefit
is that a row written by any past version always loads.

## Gotchas

- The schema version is a hand-set constant (`SCHEMA_V0_93_0`), **not**
  `CARGO_PKG_VERSION` — bumping it is a deliberate act tied to a model change.
- Unknown schema versions are rejected on read, never silently ignored.
- DTO type names carry version suffixes with underscores
  (`DocumentV0_81_0`); `non_camel_case_types` is allowed module-wide for this.

## Links

- [ARCHITECTURE.md](ARCHITECTURE.md) — `Document` in the core type overview
- [markdown-spec.md](../references/markdown-spec.md) — Markdown syntax and the in-memory data model
- [VERSIONING.md](VERSIONING.md) — quill version resolution (a separate concern)
- [QUILL_VALUE.md](QUILL_VALUE.md) — value type stored inside payload fields
