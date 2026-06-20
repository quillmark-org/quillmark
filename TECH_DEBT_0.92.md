# Technical-debt sweep — the 0.92.0 breaking-change window

A coordinated, workspace-wide audit conducted ahead of `v0.92.0` (the DTO /
`Document`-model breaking release that also ships the greenfield .NET binding).
Six parallel sweeps covered: Document & DTO model; Quill/schema/value/error;
the Typst backend; the .NET binding; Python/WASM/CLI; and docs/CI/packaging.

Every claim below is line-cited and was grep-verified. This document is the
companion to the two existing trackers — it does **not** restate them except to
confirm an item is still live:

- [`SIMPLIFICATIONS.md`](SIMPLIFICATIONS.md) — simplification opportunities
- [`VULNERABILITIES.md`](VULNERABILITIES.md) — security findings

**The breaking-change lens.** Items are tagged **[BREAK]** (the fix changes a
public API / wire format / trait and can *only* land cleanly in a major break —
do it now or wait a full release cycle) or **[FREE]** (non-breaking; can land
any time, but is cheapest to batch with the break). Effort S/M/L, Payoff H/M/L.

---

## Status — paid off in this branch

Applied here (correctness fixes, risk-free `$seed` hardening since the key is
unreleased, additive de-duplication, the dead-`pub`-API removals, the .NET
panic-guard sweep, and docs hygiene):

- **Correctness:** C1 (`__meta__` no longer leaks into card objects), C6
  (serialization failure → diagnostic, not silent `"{}"`), B2 (`schema()`
  `expect` over silent null).
- **`$seed` hardening:** A7 (drop reserved `$`-keys), A10 (validate seed kind),
  A8 (comment).
- **De-duplication (core now owns the tables):** E1 (`OutputFormat`
  `as_str`/`mime_type`/`ALL`/`Display`/`FromStr`), E2 (`EditError::variant_name`),
  E9 (`STANDARD_METADATA_KEYS`). F3 (`; delete-ok` doc).
- **Dead / foot-gun `pub` API removed (breaking):** C3 (dead `compile_to_*`
  trio; tests moved to the `Backend`/`RenderSession` path), C4 (`compile`/`helper`
  modules made private), A4 (`Payload::take_quill`), A2 (unused
  `SCHEMA_V0_81_0`/`V0_82_0` re-exports).
- **.NET FFI safety (breaking):** D3 — every `unsafe extern "C"` entry point
  (49 of them) now traps panics via `ffi_try!`, so a panic becomes a
  `QuillmarkException` instead of aborting the host process.
- **Docs / hygiene:** F1 (migration nav), F2 (changelog), F4/F5/F7 (stale refs,
  landed-proposal banner, dead workspace metadata).

Regression tests added (card `__meta__`, seed-overlay reserved keys, seed-kind
validation, `OutputFormat` round-trip). `cargo test` green for
core/typst/cli/quillmark; `cargo build -p quillmark-dotnet`, `cargo check
--workspace`, and `cargo doc -Dwarnings` clean; bindings (wasm/python/.NET)
rely on CI for their runtime suites.

**Deliberately deferred** (need focused, separately-reviewed changes — larger
breaks or behavior changes that can't be locally test-verified): the
`RenderError` flatten (SIMPL #1), `Quill::from_tree` warnings channel (B1), the
remaining binding error-contract fixes (E3 Python `ValueError` → `QuillmarkError`,
D2 .NET `Diagnostic.ToString`, D4 .NET `set_quill_ref` code/hint, E4 Python
card-dict casing), `convert`/world warning plumbing (C2/C5), and the
`quill_reference()` field promotion (A6).

---

## Executive summary — five cross-cutting themes

The individual findings cluster into five systemic patterns. The patterns, not
the line items, are the reason to act in this window.

### Theme 1 — Silent failure: the diagnostic channel is leaky end-to-end
The most pervasive defect class (8 findings across 4 lanes). Quillmark's whole
value proposition is structured diagnostics, yet warnings and errors are dropped
at nearly every layer:
- `Quill::from_tree` throws away **all** Quill.yaml authoring warnings — its own
  comment claims it propagates them (B1). The `from_yaml_with_warnings` API is
  effectively dead.
- The Typst backend prints compile + asset/package-load warnings to **stderr**
  (invisible in WASM/library embeddings) (C5), silently substitutes `"{}"` for
  the entire document payload on a serialization error (C6), and silently keeps
  **unconverted Markdown** in a field when `mark_to_typst` fails (C2).
- `QuillConfig::schema()` returns `null` on a serialization failure on the hot
  render path (B2); `check_quill_reference` swallows an unparseable `$quill`
  version (B5); `SeedOverlay` silently drops a non-string `$body` (A7).

**These are individually small fixes but collectively a credibility issue for a
"diagnostics-first" tool.** Fixing the channel (B1, C5) is breaking; the rest
are free.

### Theme 2 — The "uniform errors" contract is only half-enforced
Canon ([`BINDINGS.md`](prose/canon/BINDINGS.md), [`ERROR.md`](prose/canon/ERROR.md))
mandates one error type per binding, always carrying a non-empty diagnostic list
rendered by the canonical `fmt_pretty`. Two of three audited bindings violate it:
- Python raises bare `PyValueError` (no `.diagnostics`, no code/hint) from 9+
  public methods incl. `set_quill_ref`, `make_card`, `push_card` (E3).
- The greenfield .NET `Diagnostic.ToString()` invents its own format instead of
  `fmt_pretty` (D2), and `set_quill_ref` omits the code+hint (D4 — same root as
  E3). Worse, ~40 .NET entry points lack the `ffi_try!` panic guard, so a panic
  **aborts the host process** instead of becoming an exception (D3).

Since .NET is greenfield *this release* and Python's DTO mapping is being
rewritten anyway, this is the moment to make the contract real.

### Theme 3 — Hand-maintained tables that core should own
The same lookup is copy-pasted across 2–4 crates; each copy is a future drift
bug. Adding one `OutputFormat` variant today means editing five sites.
- `OutputFormat ↔ string ↔ MIME`: 3 copies (E1, = SIMPL #4)
- `EditError` variant-name strings: 3 copies (E2, = SIMPL #4)
- "standard metadata keys": 4 copies (E9)
- identifier-validation predicates: 4 copies **with divergent acceptance sets**
  (A5 / B3, = SIMPL #6)
- `$ext`/`$seed` meta-depth check: 3 copies (A1); `make_card` assembly: 2 (E5);
  Typst field-name projection: 2 (C7, = SIMPL #8)

Most are additive (core gains a method/const; bindings forward) and therefore
**[FREE]** — but the predicate unification is **[BREAK]** and best done now.

### Theme 4 — `$seed` shipped asymmetric and unhardened
The new 0.92 feature has rough edges that should be filed down before authors
build on them: no blunt `removeSeed` in any binding though `$ext` has one (A3);
`set_seed_namespace` accepts invalid kind names that `$ext` correctly rejects
the analogue of (A10); `SeedOverlay` stores stray `$`-keys as user fields (A7);
the .NET seed methods are entirely untested (D7); and none of `$seed` /
`!must_fill` / `nested_fills` is fuzzed (F6). `$ext`/`$seed` symmetry is the
stated design goal (recent commit `8488740`) — these are the gaps.

### Theme 5 — Dead / foot-gun `pub` surface that only a break can remove
Published-crate API that is unused in-repo or actively dangerous, removable only
in a major break: `Payload::take_quill` (0 callers, produces an invalid main
card — A4); the `compile_to_{pdf,svg,png}` trio (dead/test-only — C3) plus the
`helper` module's `pub` constants and `compile_to_document`/`QuillWorld::new`
(C4); the unused `SCHEMA_V0_81_0`/`SCHEMA_V0_82_0` re-exports (A2); `Document::
quill_reference()`'s `expect`+clone, fixable to a stored `&QuillReference` field
(A6); and (from SIMPL) `FileTreeNode::print_tree` + six facade re-exports.

---

## Recommended roadmap for 0.92.0

Tiered by "fix now or pay later." Tiers 1–2 are the breaking-window payload;
3–5 are cheap to batch in.

### Tier 1 — Ship-blocking for the new/changed surfaces
| # | Finding | Tag | Effort | Payoff |
|---|---------|-----|--------|--------|
| D3 | Add `ffi_try!` to all ~40 .NET FFI entry points (panic = process abort) | BREAK | M | H |
| E3 | Python raises `PyValueError` (no diagnostics) from 9+ methods | BREAK | S | H |
| D2 | .NET `Diagnostic.ToString()` diverges from canonical `fmt_pretty` | BREAK | S | H |
| D4 | .NET `set_quill_ref` diagnostic missing `code`+`hint` (= E3 root) | BREAK | S | M |
| E4 | Python card dict camelCase/snake_case hybrid (`payload_items` vs `nestedFills`) | BREAK | S | M |
| A7 | `SeedOverlay::from_json_map` drops non-string `$body`; stores stray `$`-keys as fields | mixed | S | M |
| A10 | `set_seed_namespace` accepts invalid `card_kind` (asymmetry with `$ext`) | FREE | S | M |
| A3 | No `removeSeed` blunt-remove in any binding (`$ext`/`$seed` asymmetry) | FREE | S | M |
| F6 | Fuzz crate has zero coverage for `$seed` / `!must_fill` / `nested_fills` | — | M | H |
| F1 | `mkdocs.yml` nav omits the 0.92 migration guide (unreachable on ship day) | — | S | H |
| D7 | .NET `SeedCard`/`SeedMain`/`SeedDocument` untested | — | S | M |

### Tier 2 — High-value breaking cleanups (only doable in a break)
| # | Finding | Tag | Effort | Payoff |
|---|---------|-----|--------|--------|
| B1 | `Quill::from_tree` discards Quill.yaml warnings — revive the channel across all bindings | BREAK | M | H |
| A6 | `Document::quill_reference()`: store `quill_ref` field → kill `expect`+clone+scan; rename `quill_ref()` | BREAK | S | M |
| SIMPL#1 | Flatten `RenderError` to `{ kind, diags }` | BREAK | M | H |
| A5/B3 | Unify the 4 identifier predicates (document the leading-`_`/NFC policy split) | mixed | S | M |
| C3+C4 | Remove dead `compile_to_*` trio; demote `helper`/`compile_to_document`/`QuillWorld::new` to `pub(crate)` | BREAK | S | M |
| A4 | Remove `Payload::take_quill` foot-gun (0 callers, breaks main-card invariant) | BREAK | S | M |
| A2 | Drop unused `SCHEMA_V0_81_0`/`SCHEMA_V0_82_0` re-exports | BREAK | S | L |

### Tier 3 — Silent-failure / correctness (mostly free; do alongside)
| # | Finding | Tag | Effort | Payoff |
|---|---------|-----|--------|--------|
| C1 | `__meta__` sentinel leaks into **every** card object exposed to plate authors | FREE | S | H |
| C6 | `serde_json::to_string` → `"{}"` silent full-payload loss in `Backend::open` | FREE | S | H |
| C2 | `convert_content_value` keeps unconverted Markdown on `mark_to_typst` error | FREE | S | H |
| C5 | Typst compile + world warnings dumped to stderr (invisible in WASM) — extends VULN TYPST-2 | BREAK | M | H |
| B2 | `QuillConfig::schema()` `unwrap_or(Null)` on the render hot path | FREE | S | M |
| B5 | `check_quill_reference` swallows an unparseable `$quill` version | FREE | S | M |

### Tier 4 — Duplication core should own (mostly additive)
| # | Finding | Tag | Effort | Payoff |
|---|---------|-----|--------|--------|
| E1 | `OutputFormat::mime_type()`/`FromStr`/`Display`/`all()` → collapse 3 copies (= SIMPL #4) | FREE | S | H |
| E2 | `EditError::variant_name()`/`Display` → collapse 3 copies (= SIMPL #4) | FREE | S | M |
| A1 | One bounded-meta-map depth helper → collapse 3 copies | FREE | S | M |
| E5 | `CardWire::from_fields(...)` constructor → collapse WASM+Python `make_card` | FREE | S | M |
| C7 | `collect_field_names(props, predicate)` + single `$defs` pass (= SIMPL #8) | FREE | S | M |
| E9 | `quillmark_core::STANDARD_METADATA_KEYS` const → collapse 4 copies | FREE | S | L |
| D1 | .NET: declare count returns as `nint`/`int`, not `IntPtr` | FREE | S | M |

### Tier 5 — Docs / hygiene (free, cheap, finish before ship)
F2 (CHANGELOG v0.90 empty; content stranded in misplaced `## Unreleased`),
F3 (`; omit-ok` → `; delete-ok` stale doc comment), F5 (banner/delete landed
`mcp-feedback.md`), F4 (`build-wasm.sh` points at superseded proposal),
F7 (delete dead `[workspace.metadata.workspaces]` + misleading comment),
A8 (stale `$seed`-less ordering comment in `wire.rs`), A9 (`check_meta_depth`
needless clone), B4 (deprecate `QuillConfig::from_yaml`), B6 (`Version::Display`
3-segment round-trip note), E6 (CLI `validate` reinvents `Diagnostic`/`Severity`;
divergent output), E7 (Python `quill_ref` hardcoded `0.0.0` fallback), E8 (CLI
`validate` hides warnings by default — opposite of `render`), C8 (`MarkdownFixer`
unguarded `unwrap()`s), C9 (`error_mapping` derives codes by `split(':')` on a
human message), D5/D8/D9 (.NET minor polish).

Confirmed still-live SIMPLIFICATIONS packaging items: npm license mismatch
(`package.template.json` says `MIT OR Apache-2.0`; only Apache `LICENSE` exists;
`build-wasm.sh:124-129` copies non-existent files); Python 3.13 wheel/classifier
mismatch (abi3-py310 makes the extra interpreters redundant); 6 unreferenced
fixture resources.

---

## Appendix — full findings by lane

### Lane A — Document & DTO model
- **A1** `$ext`/`$seed` meta-depth check duplicated 3× with copy-paste `unreachable!`s — `wire.rs:257-290`, `dto.rs:579-594`, `edit.rs:92-100`. [FREE] S/M.
- **A2** `SCHEMA_V0_82_0` (and `_V0_81_0`) re-exported with zero readers — `dto.rs:56`, `mod.rs:38-39`; only `SCHEMA_V0_92_0` is used (all 4 bindings). [BREAK] S/L.
- **A3** No `removeSeed`/`remove_seed`/`qm_document_remove_seed` in any binding though `$ext` has the blunt-remove — `wasm/engine.rs:721`, `python/types.rs:456`, `dotnet/lib.rs:906`. [FREE-additive] S/M.
- **A4** `Payload::take_quill` — 0 callers; produces a main card violating the "main carries `$quill`" invariant — `payload.rs:445-454`. [BREAK] S/M.
- **A5** 3–4 identifier predicates re-implement `[a-z_][a-z0-9_]*` — `edit.rs:27`, `meta.rs:171`, `config.rs:870,880` (see B3 for the divergence). [FREE] S/M.
- **A6** `Document::quill_reference()` `expect`s + `clone`s on every call (`to_plate_json` hot path) — `mod.rs:239-244`. Store a `quill_ref: QuillReference` field; expose `&QuillReference`. [BREAK] S/M. **(verified)**
- **A7** `SeedOverlay::from_json_map` silently drops a non-string `$body`; non-`$body` `$`-prefixed keys are stored as user fields — `mod.rs:163-180`; callers `dotnet/lib.rs:468`, `wasm/engine.rs:472`, `python/types.rs:229`. [mixed] S/M.
- **A8** Stale comment lists `$quill < $kind < $id < $ext` omitting `$seed` — `wire.rs:244` (actual order `payload.rs:188-199`). [FREE] S/L.
- **A9** `check_meta_depth` clones the whole map only to borrow it — `edit.rs:92-100`. [FREE] S/L.
- **A10** `set_seed_namespace` inserts `card_kind` with no `is_valid_kind_name` check (advisory validator catches it only later) — `edit.rs:339-362`. [FREE] S/M.

### Lane B — Quill / schema / value / error
- **B1** `Quill::from_tree` discards `from_yaml_with_warnings` warnings; the comment claims it propagates them — `load.rs:45`. Affects every binding. [BREAK] M/H. **(verified)**
- **B2** `QuillConfig::schema()` `unwrap_or(Null)` ×3 on the Typst-context hot path — `config.rs:93,101,107`. [FREE] S/M.
- **B3** `is_snake_case_identifier` forbids leading `_` and skips NFC while the 3 peer predicates allow `_`/normalize — `config.rs:870-888` vs `edit.rs:27`/`meta.rs:171`. Unification is behavioral; document the policy split. [mixed] S/M.
- **B4** `pub fn from_yaml` bakes in the same warning-discard — `config.rs:895-907`. Deprecate. [FREE] S/M.
- **B5** `check_quill_reference` returns empty diags on an unparseable version — `compose.rs:101-120`. Push a warning. [FREE] S/M.
- **B6** `Version::Display` always emits 3 segments → a `Version` round-tripped into a `$quill` ref flips `Minor`→`Exact` (no current defect; document) — `version.rs`. [FREE] S/L.
- Confirms SIMPL #1 (`error.rs:370-395`, 8 identical variants), #5 (`normalize.rs:174-187` only ever `Ok`), #6, #4; and the live `!must_fill` sentinel (`validation.rs:13`).

### Lane C — Typst backend
- **C1** `__meta__` injected into every card by `transform_markdown_fields` and never stripped from `$cards` — `lib.rs:381-389,422-431`; template removes it only at top level (`lib.typ.template:62`). Plate authors enumerating a card see it. [FREE] S/H. **(verified)**
- **C2** `convert_content_value` maps `mark_to_typst` errors to `None`/clone → raw Markdown reaches Typst markup eval — `lib.rs:278-280,285-288`. [FREE] S/H.
- **C3** `compile_to_svg`/`compile_to_png` 0 callers; `compile_to_pdf` test-only — `compile.rs:82-161`. (= SIMPL #3, verified.) [BREAK] S/M.
- **C4** `generate_lib_typ`/`generate_typst_toml`/`HELPER_*` `pub` but used only in-crate — `helper.rs:14-31`. Demote with C3. [BREAK] S/M.
- **C5** Typst compile warnings + 4 world load warnings go to `eprintln!` — `compile.rs:34-46`, `world.rs:203,260,313,347`. Extends VULN TYPST-2. [BREAK shape] M/H.
- **C6** `serde_json::to_string(...).unwrap_or_else(|_| "{}")` → silent full data loss — `lib.rs:194-195`. [FREE] S/H.
- **C7** `content_field_names`/`date_field_names` duplication + double `$defs` traversal — `lib.rs:249-270,347-378`. (= SIMPL #8.) [FREE] S/M.
- **C8** `MarkdownFixer` `unwrap()`s on peeked invariants, uncommented — `convert.rs:721,723,770,773`. [FREE] S/L.
- **C9** Error code derived via `split(':')` on a human-readable Typst message — `error_mapping.rs:29-32`; unstable, contradicts ERROR.md's `typst::<type>`. [FREE] S/L.

### Lane D — .NET binding (greenfield)
- **D1** `isize` ABI returns mapped to `IntPtr` instead of `nint`/`int` — `NativeMethods.cs:70,107`. [FREE] S/M.
- **D2** `Diagnostic.ToString()` reimplements a different format (lowercase severity, no path/hint) vs core `fmt_pretty` — `Diagnostic.cs:37-48` vs `error.rs:161-188`. [BREAK] S/H.
- **D3** ~40 `extern "C"` entry points lack `ffi_try!` (mutators, RenderResult readers, `to_markdown`/`to_json`, `schema`, `blueprint`) — `dotnet/src/lib.rs`. Panic crosses FFI → abort. [BREAK] M/H.
- **D4** `set_quill_ref` error has no `parse::invalid_quill_reference` code/hint vs WASM — `lib.rs:826-840`. [BREAK] S/M.
- **D5** `EditError` surfaced via `set_error_message` (no `code`) — consistent across bindings, low priority — `lib.rs:117-126`. [FREE] S/L.
- **D7** No tests for `SeedCard`/`SeedMain`/`SeedDocument` or overlay flow — `Quillmark.Tests/BindingTests.cs`. — S/M.
- **D8/D9** Doc `RemoveExt` null semantics; `schema_version_of`/`blueprint_instruction` `clear_error` consistency. [FREE] S/L.
- **Parity:** full method-for-method parity with Python verified; WASM-only gaps (`open`/`RenderSession`, canvas, `fromTree`/`toTree`) are by design. Only substantive drift is D2.
- **ABI assessment:** sound after recent fixes (`QmBytes` boxed-slice, null-checked frees, correct `into_raw`/`from_raw` pairing, thread-local error parking, `NativeObject` dispose). Sole gap is D3.

### Lane E — Python / WASM / CLI
- **E1** `OutputFormat`↔MIME table ×3 — `wasm/types.rs:175-183`, `python/types.rs:769-773`, `cli/render.rs:98-108`. (= SIMPL #4.) [FREE] S/H.
- **E2** `EditError` variant strings ×3 — `wasm/engine.rs:1003-1011`, `python/errors.rs:14-23`, `dotnet/lib.rs:117-125`. (= SIMPL #4.) [FREE] S/M.
- **E3** Python raises `PyValueError` (no `.diagnostics`/code/hint) from `set_quill_ref`, `make_card`, `py_dict_to_card`, `validate` fallback — `python/types.rs:562-564,607,1099-1104,187`. [BREAK] S/H. **(verified)**
- **E4** Python card dict mixes `payload_items` (snake) with `nestedFills` (camel) — `python/types.rs:916` vs `:905`. [BREAK] S/M.
- **E5** `make_card` card assembly duplicated WASM+Python — `wasm/engine.rs:860-892`, `python/types.rs:586-610`. [FREE] S/M.
- **E6** CLI `validate` declares local `Severity`/`ValidationResult` (~70 lines) and prints `[ERROR]` bypassing `fmt_pretty` — `cli/validate.rs:19-85`. [FREE] M/M.
- **E7** Python `PyQuill.quill_ref` hand-rolls a `"0.0.0"` fallback instead of `config.version` — `python/types.rs:113-122`. [FREE] S/L.
- **E8** CLI `validate` hides warnings unless `--verbose`; `render` shows them unless `--quiet` — `cli/validate.rs:267-293` vs `render.rs:156`. [FREE] S/M.
- **E9** "standard metadata keys" list ×4 — `wasm/engine.rs:394`, `python/types.rs:144`, `cli/info.rs:8`, `dotnet/lib.rs:377`. [FREE] S/L.

### Lane F — Docs / tests / CI / packaging
- **F1** `mkdocs.yml:53-59` nav stops at 0.87; 0.88→0.92 guides (incl. the ship-day 0.92 guide) are unreachable; `mkdocs --strict` doesn't catch it. S/H.
- **F2** `CHANGELOG.md:10-12` v0.90.0 empty; its notes are stranded in a misplaced `## Unreleased` at `:39-136` (breaks the `release.yml` extract). S/M.
- **F3** `mod.rs:65` doc comment says `; omit-ok`; the live token everywhere else is `; delete-ok`. S/M.
- **F4** `build-wasm.sh:10` cites the superseded `wasm-bindings-split.md`; as-built design is `0.89-to-0.90.md`. S/L.
- **F5** `prose/proposals/mcp-feedback.md` landed in 0.84 but has no banner (policy: remove landed proposals). S/M.
- **F6** Fuzz crate has zero coverage for `!must_fill`/`nested_fills`/`$seed` (also extends open VULN FUZZ-A/FUZZ-B) — `crates/fuzz/src/`. M/H.
- **F7** `Cargo.toml:68-70` dead `[workspace.metadata.workspaces]` with a comment misattributing fixtures-publish-exclusion (real cause: `publish = false`). S/L.
- Confirms SIMPL packaging items (npm license, py3.13 wheels, 6 unreferenced fixtures).
