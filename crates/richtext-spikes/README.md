# quillmark-richtext-spikes

Throwaway Phase-0 probes for the richtext content model
([#831](https://github.com/quillmark-org/quillmark/issues/831)). **Not a
product** — a member of the workspace held *outside* `default-members`, so
`cargo test --workspace` runs it but a bare `cargo test` skips it. Delete this
crate when the phases it de-risks land.

Each spike leaves an executable finding: every claim in a finding doc maps to an
assertion in the matching test file.

| Spike | Question | Finding | Tests |
|-------|----------|---------|-------|
| A | mark semantics + annotation rebase | [`phase-0-finding-a-editor.md`](../../prose/plans/richtext/phase-0-finding-a-editor.md) | `tests/spike_a_editor.rs` |
| B | source-map inversion + navigation | [`phase-0-finding-b-sourcemap.md`](../../prose/plans/richtext/phase-0-finding-b-sourcemap.md) | `tests/spike_b_sourcemap.rs` |
| C | seam encoding + determinism | [`phase-0-finding-c-seam.md`](../../prose/plans/richtext/phase-0-finding-c-seam.md) | `tests/spike_c_seam.rs` |

```
cargo test -p quillmark-richtext-spikes
```

`src/` is the shared throwaway `RichText` prototype: `model` (corpus + lines +
marks + islands), `canonical` (byte-deterministic JSON), `codec` (markdown ⇄
corpus + pdfform `.text`), `sourcemap` (per-run escape inversion), `diff`
(cold-parse diff + rebase), `usv` (USV ↔ UTF-16/UTF-8).
