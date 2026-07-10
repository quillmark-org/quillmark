# crates/bindings/wasm, crates/backends/pdfform

## Needs judgment

### wasm/src/engine.rs:1240 — `js_to_card`'s `ALLOWED` list shadows `CardWire`'s field set

The local string list exists because `serde_wasm_bindgen` ignores
`deny_unknown_fields`, and it has already diverged from serde's accepted set
(serde accepts the `payload_items`/`body_markdown` aliases; `ALLOWED` rejects
them). Adding a `CardWire` field requires remembering this list. Fix:
deserialize through `serde_json::Value` → `CardWire`, which honors
`deny_unknown_fields` and matches the Python binding
(python/src/types.rs:1230) — a small accepted-input change (aliases become
valid), so pin with a test rather than auto-applying.

### pdfform/src/resolve.rs:197 — richtext detected by parse success instead of schema classification

Every JSON `Object` is sniffed with `from_canonical_value(v).ok()?` (also
lines 169, 190), while the typst backend classifies the same seam value
structurally via the transform schema's
`contentMediaType: application/quillmark-richtext+json`
(typst/src/lib.rs:627, `SchemaMeta.content_fields`). A plain `type: object`
field bound to a text widget renders text or blank depending on whether its
shape happens to validate as a corpus, and any change to richtext
identification must be re-implemented here with different logic. Fix:
pdfform's `open` already holds the Quill — derive the richtext-classified
paths from `build_transform_schema` as typst does; keep `from_canonical_value`
as the decode, not the detector.
