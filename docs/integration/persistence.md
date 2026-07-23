# Persistence

A `Document`'s in-memory layout tracks the evolving Quillmark model and is not a stable interface. To store a document — a database row, a cache, a message payload — without persisting Markdown (whose syntax also evolves), serialize to a **versioned JSON envelope**.

| Form | Round-trips? | Stable for storage? |
|---|---|---|
| Markdown (`to_markdown`) | yes | no — syntax evolves |
| Storage JSON (`to_json`) | yes, lossless | yes — frozen per schema version |

## Round-trip

=== "Python"

    ```python
    blob = doc.to_json()              # versioned JSON string
    # … store blob …
    doc = Document.from_json(blob)    # exact reconstruction
    ```

=== "JavaScript"

    ```javascript
    const blob = doc.toJson();
    const doc2 = Document.fromJson(blob);   // or tryFromJson → null on bad input
    ```

Every blob carries a `schema` tag (`quillmark/document@<version>`). Readers dispatch on it, accept every still-supported past version by migrating forward on read, and **reject an unknown version** rather than guessing. The current tag is `quillmark/document@0.93.0`.

## Byte-stability

Within a schema version, serialization is **byte-deterministic**: equal documents produce byte-equal JSON, and the same document re-serialized under any later patch or minor release carrying the same `schema` tag produces the same bytes. This is load-bearing for content-hashing stored documents — cache keys, template-divergence detection. Only a `schema`-version bump may change the byte layout of a document the current writer produces.

A row still carrying an older schema tag migrates forward on read; that migrated form's bytes become stable once you **rewrite the row under its current tag** (read-repair).

!!! note "`to_plate_json` is not storage"
    `Document.to_plate_json` (Rust core only) is a lossy, one-way export to backends — never a persistence format. Use the storage JSON above.

Full model: [DOCUMENT_STORAGE.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/DOCUMENT_STORAGE.md).
