# Quillmark Security & Parsing/Conversion Audit

**Date:** 2026-06-11
**Scope:** All production crates — `quillmark-core`, `quillmark` (orchestration),
`quillmark-typst` (backend), and the CLI / Python / WASM bindings. Plus a
dependency advisory scan and a review of fuzz coverage.
**Method:** Three parallel deep-read audits (core parsing/YAML/emit, Typst
backend injection/conversion, orchestration + bindings), each verifying
high-value findings against the real pipeline with throwaway harnesses; plus
`cargo audit` on the full dependency tree.

## How to read this document

- **~~Struck-through headings~~** are **resolved on this branch** — fix applied,
  regression-tested, and committed. Each carries a ✅ **Resolved** line.
- Headings in normal text are **open** and await your review. They were judged
  `NEEDS-REVIEW` (a behavior/contract change or an architecturally significant
  change) and were intentionally **not** auto-applied.
- Severity: Critical / High / Medium / Low / Info.

## Executive summary

The most security-critical surface — user Markdown/YAML/card/plate content
breaking out of escaping to inject **raw Typst code** — is **solid**. Adversarial
end-to-end testing through the real Typst evaluator produced **no injection**, and
the in-memory virtual filesystem makes **path traversal structurally impossible**
on the render path (no disk or network access; packages are vendored). The
dependency tree has **zero known-vulnerable crates**.

The real issues are concentrated in **core parsing/emit** and in
**defense-in-depth hardening** of the filesystem loader and language bindings.

| Area | Critical | High | Medium | Low | Info |
|------|:-:|:-:|:-:|:-:|:-:|
| Core (parse/YAML/emit) | 0 | 2 | 3 | 0 | 1 |
| Typst backend | 0 | 0 | 0 | 2 | 3 |
| Orchestration / bindings | 0 | 0 | 2 | 6 | 2 |

**16 findings resolved** on this branch — including every High-severity item.
**1 substantive finding** (TYPST-2, warnings dropped to stderr) plus several
info items remain open.

---

## Open findings (await your review)

### TYPST-2 — Markdown→Typst compile warnings and package-load problems are discarded to stderr

- **Severity:** Low (observability)
- **Files:** `crates/backends/typst/src/compile.rs:34-36`; `world.rs:237-242, 311-315`
- **Verified:** Yes.

**Description.** Typst compile warnings and quill package-load problems (bad
`typst.toml`, missing entrypoint) are written to stderr via `eprintln!` and
dropped rather than surfaced as `Diagnostic`s. In WASM/library embeddings stderr
is invisible, so authors get no signal. Not a security issue.

**Why not auto-fixed.** Changing the diagnostic surface / warnings channel is an
API expectation change. **Recommended:** collect warnings into the returned
`Diagnostic` set.

---

### Info-level / accepted notes (no change recommended now)

- **CORE-6** (`from_value` lacks serde's recursion guard) — folded into the
  CORE-2 resolution; see its residual-risk note in the resolved section.

- **TYPST-3** (`convert.rs:55-71`) — `escape_markup` omits line-start `=`/`-`/`+`.
  Unreachable today (pulldown soft-breaks become spaces; hard breaks emit inline
  `#linebreak()`, so escaped text is never at a true Typst line-start). Latent if
  the emitter is ever changed to emit raw newlines between text runs.
- **TYPST-4** (`lib.typ.template:36`, `lib.rs:325`) — date fields flow unescaped
  into `_parse-date`, which hard-errors on malformed values (graceful, not a
  panic). `Quill::compile_data()` validates `format: date-time` upstream, so this
  is defense-in-depth only.
- **TYPST-5** (`lib.typ.template:8-29`) — `signature-field` emits `#metadata(..)`
  that a plate using an unfiltered `query(metadata).last()` could read by
  accident. Already documented in the template; `extract.rs` filters correctly.
- **FUZZ-1** — concrete proptest coverage gaps, highest value first:
  - **FUZZ-A** `QuillConfig::from_yaml_with_warnings` with arbitrary YAML — the
    largest untested untrusted-input surface (CLI `validate`, WASM `fromTree`).
  - **FUZZ-B** `Document::from_json` with arbitrary JSON (storage DTO from a DB/network).
  - **FUZZ-C** `QuillReference::from_str`; **FUZZ-D** `FileTreeNode::insert` with
    adversarial paths; **FUZZ-E** `QuillIgnore` pattern/path matching;
    **FUZZ-F** mutation-model round-trip for `to_markdown`.
- **Dependency advisories** (`cargo audit`, 437 deps): **0 vulnerabilities**. Three
  *unmaintained* informational warnings on transitive deps pulled in by the Typst
  stack — `bincode 1.3.3` (RUSTSEC-2025-0141), `paste 1.0.15` (RUSTSEC-2024-0436),
  `yaml-rust 0.4.5` (RUSTSEC-2024-0320). No action required; track for upstream
  replacement.

---

## Resolved on this branch

### ~~CORE-1 — Indented `~~~` inside a YAML block scalar prematurely closes the card block (silent data corruption)~~

- **Severity:** High · **Files:** `crates/core/src/document/fences.rs` (closer
  scan), `prose/references/markdown-spec.md` §3.2 / §4 D2
- ✅ **Resolved — by spec amendment.** The spec previously allowed the closing
  `~~~` to carry 1–3 leading spaces (inherited from CommonMark's closing-fence
  rule). That leniency exists in CommonMark for indented openers and list
  contexts — neither applies to card-yaml blocks, whose openers are required to
  be column-zero — and the payload between the fences is YAML, where an
  indented `~~~` is structurally payload (a block-scalar line), not a closer.
  The spec now requires the closing fence at **column zero** (§3.2, D2), the
  scanner enforces it, and the corruption case parses intact: a tilde code
  fence inside a `|` block-scalar value stays in the field. A document whose
  only closer was indented now falls through to CommonMark as an unclosed code
  block with the existing `parse::unclosed_code_block` warning — a diagnostic
  instead of silent truncation. A column-zero `~~~` can never be block-scalar
  content (YAML requires scalar content indented past its key), so the closer
  is unambiguous and no block-scalar-aware scanner is needed.
- **Regression tests:**
  `card_fence_tests.rs::indented_tilde_inside_block_scalar_is_payload_not_closer`,
  `card_fence_tests.rs::indented_tilde_line_never_closes_a_card_fence`.
- **Migration note:** `docs/migrations/0.90-to-0.91.md`.

### ~~CORE-2 — Unbounded recursion → stack-overflow abort via deserialization / value conversion (DoS)~~

- **Severity:** High · **Files:** `crates/core/src/value.rs`, `document/edit.rs`,
  `document/dto.rs`, `document/wire.rs`, `document/assemble.rs`,
  `bindings/python/src/types.rs`
- ✅ **Resolved.** The spec §8 nesting limit (100) is now enforced at **every
  payload ingestion boundary**, not just the markdown parse path: an iterative
  (explicit-stack) `json_depth_exceeds` walker in `value.rs` backs a single
  shared validator (`edit::validate_field`), called by the parse path, the
  storage-DTO path, the live-wire path, and the typed mutators (`set_field` /
  `set_fill` / `set_ext` / `set_ext_namespace` — the `$ext` mutators now return
  `Result` and carry the same bound, since `$ext` flows through the recursive
  emit and DTO paths). The Python converter `py_to_json` bounds its own
  recursion at the same limit. Because no constructed `Document` can hold a
  value deeper than 100 levels, the recursive consumers (`to_markdown`,
  `to_plate_json`, DTO serialization) are bounded by construction.
- **Residual (documented):** `serde_wasm_bindgen::from_value`'s own recursion
  while converting a pathologically deep JS object can still exhaust the WASM
  stack before our checks run — a JS caller DoS-ing its own tab; and a native
  embedder that programmatically builds a >~5000-deep `serde_json::Value` and
  calls `serde_json::from_value::<Document>` overflows inside serde_json
  itself (the JSON *string* entry points are protected by serde_json's
  recursion limit, and both binding converters are now bounded, so no
  untrusted-input route reaches that state).
- **Regression tests:** `edit_tests.rs::set_field_rejects_value_past_depth_limit`,
  `storage_dto_rejects_value_past_depth_limit`,
  `wire_card_rejects_value_past_depth_limit_and_bad_names`.

### ~~CORE-4 — Top-level data-field names not validated against `[a-z_][a-z0-9_]*` at parse time~~

- **Severity:** Medium · **Files:** `crates/core/src/document/assemble.rs`,
  `edit.rs`, `dto.rs`, `wire.rs`
- ✅ **Resolved — via the same unification as CORE-2.** The shared
  `edit::validate_field` enforces the spec §3.4/§10 name rule on the markdown
  parse path (previously mutator-only), the storage-DTO path, and the wire
  path. This closes the half of the emit round-trip hole CORE-3 couldn't: a
  non-conforming *top-level* key (e.g. `"a: b": 1`) can no longer enter a
  `Document` and emit as broken YAML — it is a parse/storage/wire error naming
  the key. Two tests that encoded the old spec-violating behavior
  (`test_uppercase_payload_keys_pass_parser`, `test_unicode_in_yaml_keys`)
  were updated to assert the spec rule; Unicode remains fully supported in
  *values*.

### ~~CORE-5 — Trailing comment silently dropped when a plain scalar value contains `'` or `"`~~

- **Severity:** Medium · **Files:** `crates/core/src/document/prescan.rs`
- ✅ **Resolved — by conforming to YAML 1.2** rather than tuning the old
  heuristic. `split_trailing_comment` now determines scalar style from the
  *first* character of the value, per YAML: a quote opens a quoted scalar only
  at the start of the scalar (or inside a flow collection `[`/`{`, where the
  old anywhere-tracking remains correct and is retained); inside a plain
  scalar `'` and `"` are ordinary characters. `x: it's a test # note` now
  preserves the comment; `x: 'a # b'` still treats the `#` as content;
  `''`-escaped and `\"`-escaped quotes and unterminated (multi-line) quoted
  scalars are handled.
- **Regression tests:** seven YAML-conformance cases in
  `prescan.rs::tests` (apostrophe/double-quote in plain scalars, quoted-scalar
  comments, escapes, flow collections, unterminated quotes, `a#b`).

### ~~WASM-1 — Nine silent `.unwrap_or(JsValue::UNDEFINED)` on serialization failures~~

- **Severity:** Low · **Files:** `crates/bindings/wasm/src/engine.rs`
- ✅ **Resolved.** All nine sites route through one `serialize_or_throw`
  helper: serialization failure now throws a `WasmError` naming the surface
  (`schema`, `metadata`, `cards`, `warnings`, `removeField`, card/ext reads)
  instead of silently yielding `undefined`. Getters changed from `JsValue` to
  `Result<JsValue, JsValue>` — the success path is byte-identical for JS
  callers; only the (previously silent) failure mode changes.

### ~~TYPST-6 — Image alt text dropped in Markdown→Typst conversion (accessibility loss)~~

- **Severity:** Low (CommonMark fidelity / PDF accessibility) · **Files:**
  `crates/backends/typst/src/convert.rs`, `prose/canon/CONVERT.md`,
  `prose/references/markdown-spec.md` §6.3
- ✅ **Resolved.** `![alt](src)` now emits `#image("src", alt: "…")` — Typst's
  `alt:` parameter flows into the PDF as accessibility alternate text, which
  the old conversion discarded. Alt-text events are collected until
  `TagEnd::Image` and flattened to text (`alt:` is a string), which also fixes
  a latent leak where markup *inside* alt text (`![a *b*](x)`) emitted stray
  `#emph[]` into the output. The link-style title is still dropped for both
  links and images (Typst has no counterpart), now with the rationale recorded
  in `CONVERT.md`; the stale spec §6.3 claim that images are "not yet
  implemented" was corrected.
- **Regression tests:** four image tests in `convert.rs` (alt emission, empty
  alt, markup flattening + no leakage, quote/backslash escaping).

### ~~CORE-3 — Map keys emitted unescaped → invalid YAML / broken round-trip~~

- **Severity:** Medium-High · **Files:** `crates/core/src/document/emit.rs`
- ✅ **Resolved.** Nested mapping keys are now emitted through saphyr's scalar
  quoting (`emit_key`), so a nested key containing `: `, a leading YAML indicator
  (`*`, `&`, `?`, …), `#`, edge whitespace, or a type-ambiguous form (`n`, `true`,
  `123`) is correctly quoted and re-parses to the same key. Top-level field names
  (indent 0) are kept verbatim via `emit_key_at`, because the line-oriented
  prescan accepts only bare `[a-z_][a-z0-9_]*` names there and quoting one would
  make it unparseable (this is the CORE-4 boundary). Before the fix, a nested key
  like `a: b` emitted `a: b: 1`, which fails to re-parse.
- **Regression test:** `emit_tests.rs::nested_map_keys_with_structural_chars_emit_valid_yaml`.

### ~~TYPST-1 — Error locations from foreign sources mis-attributed to `main.typ`~~

- **Severity:** Low · **Files:** `crates/backends/typst/src/error_mapping.rs:46-62`
- ✅ **Resolved.** `resolve_span_to_location` now resolves a diagnostic against
  **its own source file** (`span.id()`), falling back to `world.main()` only for
  the detached span. Errors originating in an injected helper package or a
  vendored package now report the correct file path, line, and column instead of
  `main.typ` coordinates.

### ~~CLI-1 — `validate`: `plate_file` path-existence oracle (traversal + absolute)~~

- **Severity:** Medium · **Files:** `crates/bindings/cli/src/commands/validate.rs:181+`
- ✅ **Resolved.** `validate_file_references` now rejects any `plate_file` whose
  path contains non-`Normal` components (`..`, absolute roots) before touching the
  filesystem, closing the existence-probing oracle a crafted `Quill.yaml` could
  use (`../../../etc/shadow`, `/etc/passwd`). (The render path was already safe —
  `FileTreeNode::get_node` returns `None` for escaping paths.)

### ~~CLI-2 — `unreachable!` in `OutputWriter::write`~~

- **Severity:** Info · **Files:** `crates/bindings/cli/src/output.rs:34`
- ✅ **Resolved.** Replaced the `unreachable!` with a typed
  `CliError::InvalidArgument`, so a future caller constructing
  `OutputWriter::new(false, None, ..)` gets a clean error instead of a panic.

### ~~LOAD-1 — Quill directory loader follows symlinks into the in-memory tree~~

- **Severity:** Medium · **Files:** `crates/quillmark/src/load.rs:78+`
- ✅ **Resolved.** `load_dir` now stats entries with `symlink_metadata` and
  **skips symlinks** instead of dereferencing them, so a crafted quill bundle
  cannot pull a sensitive host file (`assets/x -> /etc/shadow`) into the asset
  tree the backend reads.
- **Regression test:** `load.rs::tests::load_dir_skips_symlinks` (unix).

### ~~LOAD-2 — No per-file size limit in the quill directory walker~~

- **Severity:** Low · **Files:** `crates/quillmark/src/load.rs`
- ✅ **Resolved.** Added a `MAX_QUILL_FILE_SIZE` (50 MiB) guard so a single
  oversized file in a quill directory returns a clean error instead of exhausting
  memory — mirroring the `MAX_INPUT_SIZE` guard on `Document::from_markdown`.

### ~~LOAD-3 — `FileTreeNode::get_node` silently drops `..`/`.`/root components~~

- **Severity:** Low · **Files:** `crates/core/src/quill/tree.rs:30-40`
- ✅ **Resolved.** `get_node` now **rejects** any non-`Normal` path component
  (returns `None`), matching `insert`'s behavior. Previously `get_file("a/../b")`
  navigated to `a/b`; an asymmetry that could mask path handling assuming
  normalization.
- **Regression test:** `tree.rs::tests::get_node_rejects_traversal_components`.

### ~~PY-1 — Python `float('nan')`/`float('inf')` silently becomes JSON `null`~~

- **Severity:** Low · **Files:** `crates/bindings/python/src/types.rs`
- ✅ **Resolved.** `py_to_json` now raises a `ValueError` for non-finite floats
  instead of silently storing `null` (the old behavior — `serde_json::json!(nan)`
  maps to `Value::Null` — corrupted data with no diagnostic).

### ~~PY-2 — Python `int` overflow leaked a raw `OverflowError` across FFI~~

- **Severity:** Low · **Files:** `crates/bindings/python/src/types.rs`
- ✅ **Resolved.** `py_to_json` now tries `i64`, then `u64`, then raises a clean
  `ValueError` for integers beyond 64-bit — so large positive ints convert
  losslessly and out-of-range values report a uniform binding error rather than
  PyO3's raw `OverflowError`.

### ~~WASM-2 — `paint()` did not validate the computed `render_scale` product~~

- **Severity:** Low · **Files:** `crates/bindings/wasm/src/engine.rs:1325`
- ✅ **Resolved.** `paint()` now validates `render_scale = layout_scale *
  effective_density` is finite, positive, and within `f32` range before handing
  it to `render_rgba`, closing the overflow-to-infinity path (reachable e.g. via a
  zero-dimension page that bypasses the `MAX_BACKING_DIMENSION` clamp).

---

## Verification

- `cargo test --workspace` — **green** (~945 tests; baseline and after all fixes).
- CI on PR #719 — **green**: `test`, `lint`, `wasm` (runs the binding tests where
  the `Result<JsValue>` getter changes land), and `build`.
- `cargo build -p quillmark-python` — compiles; clippy clean on changed crates.
- WASM/Python binding test suites run on CI (no local `wasm32`/maturin target).
- **Empirical depth-boundary probe:** documents that parse from markdown
  (serde_saphyr `Budget` rejects at nesting level 101) all survive
  `to_markdown`→`from_markdown` and JSON-storage round-trips — `json_depth_exceeds`
  shares the exact level-101 cutoff, so it never rejects a markdown-parsed
  document. No round-trip regression.
- Regression tests added across all fixes: CORE-1 (indented-tilde payload),
  CORE-2 (depth bound at mutator/DTO/wire boundaries), CORE-3 (nested-key
  quoting), CORE-5 (seven YAML-1.2 comment cases), TYPST-6 (image alt/escaping),
  LOAD-1 (symlink skip), LOAD-3 (traversal rejection).

### Independent diff review (Opus, read-only)

A second agent reviewed the full `crates/` diff. **Verdict: SHIP** — no
Critical/High issues; the three highest-risk areas (depth-semantics match,
`validate_field` never applied to nested keys, image-depth bookkeeping) all
verified clean empirically. Three Low/informational notes, all resolved or
accepted:
- The Python value converter's depth guard was off-by-one *looser* than core
  (no invariant escape, since core re-checks at the boundary). **Fixed** —
  aligned to the exact level-101 cutoff with a corrected docstring.
- Nested-image events bypass `MAX_NESTING_DEPTH` (iterative, O(events), input-size
  bounded — not exploitable). **Accepted**, now documented in a code comment.
- `split_trailing_comment` treats `#` immediately after a closing quote (`'a'#x`,
  itself invalid YAML) as a comment. Heuristic-only; serde_saphyr does the
  authoritative parse. **Accepted** (no correctness impact).

## Areas audited and found sound (no findings)

- **Typst escaping/eval boundary** — every breakout attempt (`#`-calls, bracket
  breakout from `#strong[..]`, `$math$`, `@label`, block/line comments, string
  breakout in `#link("..")`, control chars, code-fence content) was neutralized
  under adversarial end-to-end testing. No injection.
- **Typst world/file resolution** — pure in-memory map lookups; no disk/network;
  packages vendored; `../`/absolute/`/etc/passwd` all blocked.
- **`pdf_scan.rs` / `overlay`** — offset/pointer arithmetic is bounded; page-tree
  walk capped at `MAX_NODES`; encrypted/xref-stream PDFs rejected.
- **Multibyte/char-boundary slicing** across `prescan.rs`, `fences.rs`,
  `yaml_hints.rs`, `version.rs` — all slice at ASCII offsets or via `strip_prefix`.
- **YAML→JSON value conversion** (`value.rs`) — thin newtype over
  `serde_json::Value`; duplicate keys rejected upstream by saphyr.
