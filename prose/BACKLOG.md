# Backlog

Planned work not yet implemented. The next release ships **without** these.

## Absent Unendorsed-field warnings surface

An absent Unendorsed field is currently **zero-filled silently** on the render
path (see `prose/canon/SCHEMAS.md` § "Zero-filled render"); the omission is
not surfaced back to the author from a successful render. Add a non-fatal
**warnings** channel that reports absent Unendorsed fields alongside the
rendered artifact, so LLM/MCP and UI consumers can prompt to complete them.
`resolve_fields` in `quillmark::orchestration` already distinguishes the
authored / default / zero tiers, so the signal is available at the seam.

## Strict-completeness query + finalize gate

Expose a standalone "is every Unendorsed field present?" query, independent of
render, for any future finalize / publish / submit gate. Today the
`validation::field_absent` diagnostics from `Quill::validate` are the
de-facto doneness signal and no gate consumes a dedicated API; add one when a
finalize step exists.
