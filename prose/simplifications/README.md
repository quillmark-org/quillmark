# prose/simplifications/

Findings from a four-angle simplification review (reuse, simplification,
efficiency, altitude) of the richtext integration work — the full diff
`main..177efaa`. Point-in-time: line references are accurate at `177efaa` and
drift afterward. No finding is a correctness bug; each names duplicated,
wasted, or misplaced structure and the cheaper or deeper form.

Risk tags:

- **low** — mechanical, behavior-preserving; applicable without design input.
- **review** — touches public API, intentional per-site differences, or a hot
  path whose semantics need pinning before restructuring.

Files, grouped by where the fix lands:

- [`richtext.md`](richtext.md) — `crates/richtext`
- [`core.md`](core.md) — `crates/core`
- [`bindings.md`](bindings.md) — `crates/bindings/wasm`, `crates/backends/pdfform`
- [`seams.md`](seams.md) — cross-crate: the richtext value seam and the
  delta-commit protocol

Entries are removed as they are fixed or spun into GitHub issues.
