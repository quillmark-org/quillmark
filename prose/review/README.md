# Branch review — open items

Findings from the swarm review of `claude/branch-review-agent-swarm-h9rbk4`
(the `pdfform` backend + `quillmark-pdf` stamping spine work) relative to
`origin/main`. Each file is a single, self-contained item.

Items 01–06 are resolved (write-ups removed, on
`claude/prose-review-items-6svv5p`); only the partially-resolved coverage
item remains tracked here.

## Index

| # | Item | Severity | Category | Status |
|---|------|----------|----------|--------|
| 07 | [Test coverage gaps](07-test-coverage-gaps.md) | low–medium | coverage | Partially resolved — mostly non-Rust harness gaps remain |

## Resolved (01–06)

- **01 — Spine re-emits overwritten objects at generation 0** (major) — hard-error
  guard: overwriting a non-zero-generation catalog/page/`/Info` fails cleanly
  (`pdf::nonzero_generation`).
- **02 — Canvas capability contract not closed by construction** (major) —
  `Backend::supports_canvas()` deleted; capability is derived from the session
  seam (`LiveSession::supports_canvas()` + the pre-session
  `formats_support_canvas()` hint), so it cannot disagree with `paint`. The
  shared PDF→canvas extraction is intentionally deferred.
- **03 — WASM region `skip_serializing_if` without `serde(default)`** (minor) —
  added `#[serde(default)]` so the declared `from_wasm_abi` round-trip is total.
- **04 — Irrefutable `let` on the extensible `RegionKind`** (minor) — refutable
  `if let`/`let-else` at all three sites. (`RegionKind` does not exist:
  regions are geometry-only; see
  [07 — Test coverage gaps](07-test-coverage-gaps.md).)
- **05 — Stale `as_any`/downcast doc on the canvas seam** (nit) — `LiveSession::handle`
  rustdoc realigned to the generic seam.
- **06 — Docs & canon drift** (low) — PNG/canvas docs row split, crate inventory
  updated, PREVIEW.md overlay formula fixed.

## Already resolved during the original review (not tracked here)

- **Cross-binding `flatten` inconsistency** — dissolved by the flatten-flag
  collapse (`458064f`): PDF output is always AcroForm and the public
  `flatten` knob is gone.
- **Proposal self-contradiction on flattening** — reconciled in `458064f`.
- **Direct flatten byte-level coverage** — exists at the `flatten()` unit
  level (preview-gated).
