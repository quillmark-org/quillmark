# Minimize review — v0.92.1 → main

Findings from a five-reviewer sweep of `v0.92.1..main` under the theme
"minimize": unrealized simplification cascades, dead/stale code, redundant
tests, and complexity disproportionate to value. Every finding is verified
against callers at HEAD (grep evidence noted inline); speculative items were
discarded. Line numbers refer to HEAD (`5869d307`).

Estimated total: **~700 deletable lines** plus a public-API narrowing of
`quillmark-pdf` and one untested feature configuration removed.

## 1. Unrealized simplification cascades

### 1.1 The `__meta__` sentinel is write-only (typst) — ~70–90 lines
`transform_document` injects `__meta__` into document data
(`crates/backends/typst/src/lib.rs:831`) via `SchemaMeta::to_json`
(lib.rs:778, sole caller is that insert) — then every production consumer
strips it: `transformed_data` (lib.rs:214), `transform_cards_array`
(lib.rs:875), and the codegen skips any survivor (helper.rs:137). The
template consumes the generated `_qm-meta` literal, never document data. The
only readers are three unit tests asserting the shape of a value production
throws away (lib.rs:1048, 1200, 1226).

Delete `SchemaMeta::to_json`, the insert, both strips, and the helper.rs
skip; rewrite the three tests to assert `SchemaMeta` fields directly;
`test_transform_cards_array_strips_per_card_meta` (lib.rs:~1230) becomes
moot. Cascades to stale docs describing the retired eval path (lib.rs:693,
798–800, 810–811, 866–870). Follow-on: with `to_json` gone, `SchemaMeta`'s
card tables can be `BTreeMap<String, Vec<String>>` instead of
`serde_json::Map`, deleting `card_names()` and the `filter_map`
re-extraction in `validate_date_fields` (lib.rs:267–272).

### 1.2 Untested `pdfform` (no-preview) WASM feature variant with a false contract — collapse into one feature
`crates/bindings/wasm/Cargo.toml:47–59` defines both `pdfform` and
`pdfform-preview`. Only `pdfform-preview` is ever built
(`scripts/build-wasm.sh:112`; no CI job or test compiles bare `pdfform`).
The bare variant's documented contract ("this build reports
`supportsCanvas == false`") is false: `supportsCanvas` derives from the
session seam and the pdfform backend implements `page_size_pt`
unconditionally, so a bare build would report `true` while the
`pageSize`/`paint` methods don't exist (engine.rs impl blocks at ~1339 vs
~1450). Fold `web-sys` into `pdfform`, delete `pdfform-preview`, unify the
two split impl blocks and the 17 cfg predicates into one. ~20 lines plus a
whole untested configuration.

### 1.3 Dead `LiveSession::handle()` / `SessionHandle::as_any()` escape hatch — ~45 lines
`crates/core/src/session.rs:157–170` (`handle()`) and `:25` (`as_any`).
Zero callers workspace-wide; the doc itself says "No in-tree caller does."
Yet six `fn as_any` impls exist (typst lib.rs:308, pdfform lib.rs:210,
tests). Both are `#[doc(hidden)]` — no stable-API impact. Delete; keep
`'static` via an `Any` supertrait bound if needed. Re-add the day a typed
backend surface has a consumer.

### 1.4 Dead emptiness guard in `coerce_and_validate` — ~5 lines
`crates/core/src/quill/compose.rs:105–113`: `if !diags.is_empty()` inside
the `Err` arm is unreachable-as-false. The old code filtered
`validation::field_absent` out (so the vec could empty); `FieldAbsent` was
deleted with the filter, and `validate_typed_document` returns `Err` only
when `errors` is non-empty (validation.rs:278). Collapse to a `map_err`.

### 1.5 `RenderResult::with_warning` orphaned by the warnings rework — ~15 lines
`crates/core/src/error.rs:400–403`. After warnings became current-compile
session state, the single injection point is `LiveSession::render`
(session.rs:252) extending the pub field; every backend constructs via
`new`. Only caller is its own unit test. Delete builder + test.

### 1.6 `flatten()` takes a `StampOptions` every caller sets to `producer: None` — ~16 lines
`crates/backends/pdfform/src/flatten.rs:40–52`. Both production call sites
(pdfform lib.rs:106, :264) pass `producer: None`; the module doc says
flatten backs raster outputs only, so a flatten-path producer stamp is
unreachable by design. Drop the parameter; call
`PdfUpdate::begin(&pdf, None)`.

### 1.7 `pub TypstSession` is a leftover of the deleted `typst_session_of` downcast
`crates/backends/typst/src/lib.rs:47`. Every Typst-only operation moved onto
the `SessionHandle` trait; no external reference remains, all fields and the
sole inherent method are private. Demote to non-`pub` (drops a semver-public
commitment) and fix the stale "used by the WASM canvas painter" doc.

### 1.8 `quillmark-pdf` / `pdfform` public surface wider than its consumers
- 14 `pub` items in `quillmark-pdf/src/{reader,writer}.rs` have no caller
  outside the crate + pdfform's flatten.rs (which imports an explicit six +
  four). Mark `pub(crate)`; the docs.rs surface of a published crate shrinks
  to the intended `stamp`/`regions_of`/`page_media_boxes`/`PdfUpdate` seam.
- `pub use form::{FieldKind, FormField, FormParseError, FormSpec, Rect}`
  (pdfform lib.rs:22) has zero external callers (the only external use of
  the crate is `PdfformBackend` in orchestration/engine.rs:38). Delete.
- `pub use quillmark_core::RenderedRegion` (quillmark-pdf lib.rs:33–36) —
  every consumer imports from `quillmark_core` directly. Delete.

### 1.9 Clippy accommodations for a gate that no longer runs (informational)
The clippy CI gate was enabled (`a11caa45`), the tree made clippy-clean
(incl. an `#[allow(too_many_arguments)]` and a `manual_contains` rewrite),
then the gate was deleted (`68c3751a`) along with the old "formalize Clippy
later" breadcrumb. The accommodations are harmless; the gap is that nothing
marks clippy as intentionally off, inviting a repeat "make clippy clean" PR.
Restore the breadcrumb or decide the policy.

## 2. Dead / stale code

- **`FieldType::type_id()`** — `crates/quillmark-pdf/src/lib.rs:93–103`:
  zero callers workspace-wide, and its rustdoc is false (no type id exists
  in the `RenderedRegion` sidecar). Delete (~11 lines).
- **`field_key` / `ui_key` constant modules** —
  `crates/core/src/quill/types.rs:14–32` + re-export at quill.rs:25: 12
  string constants with zero consumers; the doc concedes parsing and schema
  generation both use literal strings. Delete (~28 lines).
- **"No fonts found" skip guards ×5** — the only producer of that string was
  deleted pre-0.7 (Figtree embedded fallback, `world.rs:63–72` cannot
  error); this diff rewrote all guards and added two new ones:
  `quiver_test.rs:65,109`, `usaf_memo_regions_test.rs:32`,
  `quill_engine_test.rs:118`, `usaf_memo_signature_test.rs:78`. Delete all
  five blocks (~40 lines).
- **`_qm-has-meta` constant-true guard** — `lib.typ.template:15,35`: the
  binding is always `true` (backend unconditionally generates `_qm-meta`);
  the "hand-built test helpers that set it false" its doc cites do not exist
  anywhere. Delete binding + branch + doc (~10 lines); empty-tables
  rejection stays covered by content_regions.rs:682.
- **`QuillWorld::set_source` return value never read** — world.rs:143;
  single caller discards it. Return `()`.
- **Redundant `Signature` early-return** — pdfform resolve.rs:78–80: the
  match at :87 already returns `None` for `Signature`; the intervening
  lookup is side-effect-free. Delete 3 lines.
- **`pdfform_preview.rs` example is half taro/Typst** —
  `crates/quillmark/examples/pdfform_preview.rs:26–36,85–110`: ~50 of 112
  lines drive the Typst backend on `taro`, unrelated to the example's stated
  purpose and covered by `examples/taro.rs`. Delete the taro section.
- **Stale `UnsupportedBackend` doc refs ×4** — the variant no longer exists
  (code is `engine::backend_not_found`):
  `quillmark/src/orchestration/engine.rs:55`, wasm engine.rs:275, python
  types.rs:35,72. One-word fixes.
- **`inject_helper_package` re-inserts a constant `typst.toml` per apply** —
  world.rs:172–175; move to construction. `set_binary` (:154) and
  `helper_spec` (:120) are single-caller inline candidates.
- **`read_bool` vs `read_value_bool`** — overlay/extract.rs:144,225:
  near-duplicates, one caller each; the permissive one serves both
  (`multiline: none` becomes false instead of an internal error). ~14 lines.
- **`write_zadb_char` glyph param** — flatten.rs:204–216: always `b'4'`; the
  escape check for the glyph byte is unreachable. Hardcode (~4 lines).
- **`PdfformSession.page_count` duplicates `page_boxes.len()`** —
  pdfform lib.rs:154. Derive it (~4 lines).
- **Debug artifact write in a test** — sig_field.rs:183
  `fs::write("/tmp/qm_sig_two_pages.pdf", ..)` (predates the diff; delete
  while touching the file).

## 3. Redundant / low-value tests (~450 lines, several full Typst/PDF compiles)

Rust:
- **`tests/eval_error_hint.rs` (whole file, 99 lines)** — kept for the
  retired eval-hint path; its own header says the contract moved to
  `error_mapping`'s unit test. The one remaining assertion (compile errors
  carry a location) fits in error_mapping.rs's existing `fixture_world()`
  module; the file is a third copy of the `host_tree()` scaffolding.
- **`regression_widget_dict_has_exactly_one_subtype`** —
  sig_field.rs:124–163: fences a `pdf-writer` bug now owned by the spine,
  where `quillmark-pdf/tests/stamp.rs:172–185` runs the identical byte-level
  check over all four field types. Delete the typst copy (~40 lines).
  Related: the `/MK /CA`, flag-bit, and `/DA` asserts inside the three
  sig_field form-field tests re-pin spine emission already covered by
  `stamps_all_four_field_types_into_valid_acroform`; trim to the adapter
  mapping (`/V` binding, truthiness, option matching).
- **Duplicated region-test pairs** — (a) content_regions.rs:112–165 and
  :714–783 share byte-identical plate+data; merge into one session (~35
  lines, one fewer compile). (b) content_regions.rs:443–490 duplicates
  sig_field.rs:560–633 (which covers all four widget types); fold the one
  extra assert into the sig_field test and delete the copy (~48 lines).
- **Blueprint duplicate tests** — blueprint.rs:482 vs :1115 (identical
  schema, identical two assertions); :557 subsumed by :507. Delete one of
  each pair (~39 lines).
- **`test_render_error_diagnostics_extraction`** — error.rs:443–452: field
  passthrough on the collapsed struct; the Display-aggregation test builds
  the same shape. Delete (~10 lines).
- **`test_severity_mapping` tautology** — typst error_mapping.rs:161–178:
  matches `typst::diag::Severity` literals against an inline copy of the
  two-arm mapping — never calls `map_single_diagnostic`. Delete (~18 lines).
- **`build_base_pdf` duplicates `build_base_pdf_origin`** —
  quillmark-pdf tests/stamp.rs:12–54 vs :375–410: line-for-line identical
  except the media box. Delegate (~38 lines).
- **sample_form e2e re-asserts spine bit-flags** — sample_form.rs:96–133:
  multiline/combo flags, `Opt` length, checkbox `/V`+`/AS` already pinned at
  the spine seam; the e2e's value is the binding layer. Trim (~20–25 lines;
  defensible to keep as layered coverage).
- **`flatten_has_fonts_and_text_operators`** — flatten.rs:443–468: `BT`/`Tj`
  substring checks subsumed by the exact byte-window test; only the
  `/WinAnsiEncoding` check adds signal. Fold (~15 lines).

Integration (crates/quillmark/tests):
- **`test_extract_defaults_from_quill`** — default_values_test.rs:186–223:
  triplicates core `test_config_defaults` + `test_config_defaults_method`
  (same accessor, same shape). Delete (~37 lines).
- **`test_quill_engine_end_to_end`** — quill_engine_test.rs:72–96: named
  end-to-end but only calls `dry_run` (which never reads the plate); its
  plate uses retired minijinja filter syntax. Duplicates dry_run_test happy
  paths. Delete (~24 lines). Same stale `{{ title }}` plate residue at
  dry_run_test.rs:26.
- **dry_run/default_values overlap** —
  `test_dry_run_missing_must_fill_field_is_tolerated` (dry_run_test.rs:45)
  is a strict subset of `test_absent_must_fill_is_zero_filled`
  (default_values_test.rs:134); `test_dry_run_success` subsumed by the
  dry_run assert in `test_defaults_applied_when_absent`. ~33 lines.
- **`validate_does_not_surface_field_absence`** — validate_test.rs:97–110:
  absence-raises-nothing is unit-tested twice in core validation.rs, and the
  empty-vec seam by `validate_clean_document_has_no_diagnostics`. Delete
  (~14 lines). Keep the must_fill/unknown_card tests — those code values
  have no core-level assertions.
- **Three full PDF renders for selector acceptance** —
  version_mismatch_test.rs:92–116: selector semantics are unit-tested in
  core version.rs; the check runs in `dry_run` too (per the file's own
  reject test). Switch to `dry_run`, dropping 3 Typst compiles.

Bindings:
- **Ghost-code negative tests ×3 surfaces** — python test_schema.py:139–181
  (parametrized ×2, each rendering a full PDF), test_validate.py:112–121,
  wasm basic.test.js:1171–1185 all assert the absence of diagnostic codes
  (`validation::field_absent` etc.) that exist nowhere in the workspace.
  Keep at most one absence assert per binding folded into the existing
  must_fill tests (~35 lines, one fewer PDF render).
- **runtime.test.js standalone `regions()` test** — :137–151: copy of
  basic.test.js:419–433 and subsumed by the exhaustive forwarding test in
  the same file (:338). Delete (~16 lines).
- **pdfform clamp test duplicates the typst clamp test** —
  canvas.test.js:307–322 re-runs :196 through the shared backend-independent
  `paint` binding, at ~16k px through the rasterizer. Delete (~16 lines).
- **`tests/common.rs` is loaded by zero tests** — only the two examples
  include it via `#[path]`; it is example scaffolding living in `tests/`.
  Move beside the examples (structural, lowest priority).

## 4. Low-value / high-complexity logic

- **Hand-enumerated wasm cache key** — ci.yml:101 hashes 7 globs mirroring
  build-wasm.sh's input closure; `86ef7fd8` exists because the previous list
  was incomplete, and release.yml:149 still carries the narrow key. Simpler:
  cache only `target/wasm32-*/` and always run the script (warm rebuild is
  seconds) — deletes the enumerated key, the `pkg` cache path, and both
  `cache-hit` conditionals (~12 lines) and kills the stale-pkg class
  structurally.
- **release.yml restores a previous release's `pkg/` and the script never
  cleans it** — release.yml:150–151 `restore-keys` + no `rm -rf pkg` in
  build-wasm.sh, and `npm publish` runs from `pkg/`: a file removed from the
  pkg layout between releases lingers and ships. One-line fix (`rm -rf pkg`
  in the script) or drop `pkg` from the restore path. Bug-flavored; do this
  one regardless.
- **`Quillmark::render` hand-threads `RenderOptions` fields** —
  orchestration/engine.rs:96–102: rebuilds the struct field-by-field to
  override one field — a hazard that already bit in this diff (`regions`
  had to be manually added). Use struct-update syntax:
  `RenderOptions { output_format: .., ..opts.clone() }`.
- **Python `regions` surface landed with zero tests/docs** —
  python types.rs `render(.., regions=)` kwarg + `PyRenderResult.regions`
  getter (~30 lines): no occurrence of "regions" in python tests or README,
  unlike the WASM twin. Add one smoke test (cheapest) or delete until a
  consumer exists.
- **`ChangeSet` is the one hand-written/generated TS type pair without a
  drift guard** — runtime.d.ts:197 vs the Tsify type; runtime.types.test-d.ts
  pins all six sibling pairs but not this one. Add the 4-line pair (inverse
  of a deletion, but closes the same drift class the file exists for).

## Verified clean (checked, no finding)

- `RenderError` collapse and `Severity::Note` removal propagated fully — no
  variant names, `From` impls, match arms, or Note handling survive
  anywhere; all three bindings route through `summary_message` +
  `into_diagnostics`.
- Validation collapse left no orphans (`MUST_FILL_SENTINEL`, `FieldAbsent`,
  `quotable_actual`, etc. deleted with their callers); `Quill.plate` removal
  complete; region/session seam fully consumed by real binding callers.
- dotnet purge complete (code, script, CI, release jobs); `filter_fuzz`
  deletion fully cascaded; every fixture quill is exercised (quiver
  enumerates the directory).
- `quillmark-pdf` reader (936 lines) is not over-built: every pub function
  is on a live stamp/flatten/media-box path; the bespoke-scanner-vs-lopdf
  justification holds. typography.rs fully consumed. All deps used.
- span_scan.rs state machine, `page_hashes` memoization, comemo eviction
  policy, `#withClones` pre-await snapshot (pins a real race), and the
  `KeysEqual` type asserts are each load-bearing and proportionate.
- usaf_memo_regions_test.rs vs content_regions.rs are complementary: the
  integration test uniquely covers the engine-level one-shot sidecar and
  regions-after-apply on the flagship plate.
