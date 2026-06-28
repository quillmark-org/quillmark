# Branch review — open items

Findings from the swarm review of `claude/branch-review-agent-swarm-h9rbk4`
(the `pdfform` backend + `quillmark-pdf` stamping spine work) relative to
`origin/main`. Each file is a single, self-contained item we can discuss and
action independently.

The base of the review was commit `2baba0e` (`origin/main`); the branch HEAD at
review time was `d2ff1cb`, plus the follow-up simplification `458064f`
(flatten-flag collapse) landed during review.

## Index

| # | Item | Severity | Category | Status |
|---|------|----------|----------|--------|
| 01 | [Spine re-emits overwritten objects at generation 0](01-spine-nonzero-generation.md) | major | correctness | Resolved — hard-error guard (option 1) |
| 02 | [Canvas capability contract not closed by construction](02-canvas-capability-contract.md) | major | architecture | Partially resolved — cheap fallback landed; shared-path refactor deferred |
| 03 | [WASM region type: `skip_serializing_if` without `serde(default)`](03-wasm-region-serde-roundtrip.md) | minor | correctness | Open |
| 04 | [Irrefutable `let` on the extensible `RegionKind`](04-region-kind-irrefutable-let.md) | minor | smelly | Resolved |
| 05 | [Stale `as_any`/downcast doc on the canvas seam](05-stale-canvas-downcast-doc.md) | nit | smelly | Resolved |
| 06 | [Docs & canon drift](06-docs-and-canon-drift.md) | low | docs | Resolved |
| 07 | [Test coverage gaps](07-test-coverage-gaps.md) | low–medium | coverage | Mostly resolved — tractable Rust gaps closed |

## Already resolved during review (not tracked here)

- **Cross-binding `flatten` inconsistency** — dissolved by the flatten-flag
  collapse (`458064f`): PDF output is now always AcroForm and the public
  `flatten` knob is gone, so there is no longer an option for Python/.NET to be
  missing.
- **Proposal self-contradiction on flattening** (`§7` listing an "optional
  flattened-PDF" deliverable while the body treated flattening as internal) —
  reconciled in `458064f`.
- **Direct flatten byte-level coverage** — the collapse deleted the public-path
  flatten tests; restored at the `flatten()` unit level (preview-gated) as the
  finalization of that change.
