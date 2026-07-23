# Blueprint & Seeding

A quill's schema yields two ready-made documents: a **blueprint** (an annotated form to fill) and a **seed** (a filled-out starter). Both come from `Quill.yaml` alone — no one hand-writes them.

## Blueprint — the authoring surface

`blueprint()` emits an annotated Markdown document, the same shape an author writes, pre-filled with placeholders, examples, and type hints. It is the authoring surface for LLM and MCP consumers: fill in the placeholders and the structure, `$` metadata, and body markers come for free. The emitted document is itself valid — it parses, round-trips, and renders.

```
~~~
$quill: cmu_letter@0.1.0 # keep verbatim
$kind: main
# The recipient's name and full mailing address.
recipient: !must_fill # array<string>
  - Mr. John Doe
  - 123 Main St
# The department name for the letterhead.
# e.g. Department of Electrical and Computer Engineering
department: "" # string
~~~

Write main body here.
```

Two annotation slots, disjoint by purpose: **leading `# …` lines** carry prose (a description, an `# e.g.` example); the **inline `# …`** at the end of a value line carries structure — the field's `# <type>[<format>]`.

The reader's one rule: a **`!must_fill`** marker present → replace it before shipping; a concrete value present → shippable as-is. A field with a `default:` is **Endorsed** and renders that default (keep or override); a field without one is **Unendorsed** and carries the marker, its value the field's `example:` when present (a suggested value), else bare. A surviving marker never blocks render — it raises only the non-fatal `validation::must_fill` warning; a strict consumer (an LLM authoring loop) treats any outstanding marker as "not done."

## Seeding — the filled-out twin

Seeding materializes a real `Document` (committed, structured content) rather than an annotated string. It commits each field's `example:` and leaves every other field absent, so the render floor fills `default:`, else the type-empty zero, underneath. Hand it to an editor as a "new document" starter, or render it directly.

| Projection | Intent | Output |
|---|---|---|
| `blueprint` | "give me the form to fill" | annotated Markdown string |
| seeding | "give me a filled-out one" | committed `Document` |

## Accessors

| | Blueprint | Seed |
|---|---|---|
| Python | `quill.blueprint` | `quill.seed_document()` |
| JavaScript | `quill.blueprint` | `quill.seedDocument()` |
| Rust | `QuillConfig::blueprint()` | `Quill::seed_document()` |
| CLI | `quillmark blueprint <quill>` | `quillmark render <quill>` (no input file) |

## The empty-document contract

`blueprint()` guarantees the emitted document renders — but that also depends on the quill's `plate.typ`. The quill authoring contract: **a plate MUST render an empty document** (just `$quill` / `$kind: main`, no fields) without error. Under zero-filled render every absent field becomes its type-empty value, so the empty document is the type-minimal valid input; a plate that renders it degrades gracefully on every valid shape. No template may assert an Unendorsed field is *non-empty* — the schema guarantees presence, not non-emptiness. Bundled quills are checked against this by fixture tests.

Full model: [BLUEPRINT.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/BLUEPRINT.md); the seeding cascade is in [SCHEMAS.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/SCHEMAS.md) § "Document seeding".
