# Legacy / redundant / low-value audit — tests, fixtures, logic

Second-pass residue audit of the `integration/richtext` line (HEAD `91d53ec`)
against `origin/main` (`28b29ea`). The two share **no common ancestor** — this
branch is an orphan-history reboot, so a literal `main..HEAD` diff is the whole
richtext rework (151 files, +18k/−6k) and is not a useful cleanup lens. The
useful question is: within the *current* tree, what is legacy, redundant, or
low-value?

**Prior context.** This branch already carries three cleanup passes:
`86f15fe` ("remove redundant and low-value tests", ~60 tests), `#868`
("dead-redundant-logic"), and `7574c5b` ("Trim unused test-only public
methods"). So the residue below is small and deliberately conservative — the
suite is in good health (1079 test fns, `cargo test --workspace --no-run`
exits 0, no `#[ignore]`, no `assert!(true)`). **No LEGACY tests survive** —
every audited test exercises a live symbol; the `applyFieldDelta` removal
(#886) left no orphaned tests.

---

## Tier 1 — high confidence (dead fixtures + assert-nothing tests)

These are unambiguous: orphaned files with zero references, or tests that
assert nothing.

| # | Path | Kind | Why | Action |
|---|------|------|-----|--------|
| 1 | `crates/fixtures/resources/taro.png` | fixture | 186 KB image, **0 references** anywhere (git index only). The `taro` quill ships its own assets; this root-level PNG is unused. | delete |
| 2 | `crates/fixtures/resources/versioned_letter_latest.md` | fixture | references `business_letter@latest`, a quill that does not exist in fixtures — leftover from a removed version-resolution test | delete |
| 3 | `crates/fixtures/resources/versioned_resume_exact.md` | fixture | references non-existent `resume` quill; same removed test | delete |
| 4 | `crates/fixtures/resources/versioned_resume_major.md` | fixture | same | delete |
| 5 | `crates/bindings/wasm/tests/metadata.rs:6` | test | `test_quill_from_tree_with_ui_metadata` builds a quill declaring `ui.group`, then `let _ = quill;` — never asserts the UI metadata it names. The real check is `wasm_bindings.rs:333`. | delete file |
| 6 | `crates/bindings/wasm/tests/resolve_quill.rs:8` | test | `test_quill_from_tree_versioned` builds two same-name/different-version quills and discards both — asserts nothing about version resolution | delete or make it assert |
| 7 | `crates/core/src/document/tests/assemble_tests.rs:1708` | test | `test_yaml_syntax_error_bad_indentation` ends `let _ = result;` ("may or may not be an error") — pure no-panic smoke, covered by `parse_fuzz::fuzz_decompose_no_panic` | delete |
| 8 | `crates/fuzz/src/convert_fuzz.rs:100`/`:113` | fuzz | `fuzz_escape_string_no_raw_quotes` and `fuzz_escape_string_valid_escapes` — **byte-identical** generator (`\PC*`) and body | delete one |

## Tier 2 — solid redundancy (subsumed by a stronger sibling)

Each is fully covered by a named stronger test; deletion loses no coverage.

| Path | Test | Subsumed by | Note |
|------|------|-------------|------|
| `richtext/src/serial.rs:642` | `byte_deterministic_regardless_of_input_order` | `tests/properties.rs:285 canonical_json_order_insensitive` | proptest does the same `marks.reverse()` over fuzzed corpora |
| `richtext/src/serial.rs:631` | `round_trips_and_is_fixed_point` | `properties.rs:274 canonical_json_fixed_point` + the stricter golden pin `serial.rs:669` | |
| `richtext/src/delta.rs:518` | `anchor_survives_edit_elsewhere` | `properties.rs:296 diff_import_preserves_surviving_anchor` | proptest generalizes the exact scenario |
| `core/.../emit_tests.rs:176` | `round_trip_string_ambiguous` | `ambiguous_strings_tests.rs::ambiguous_word_booleans_round_trip` + `::ambiguous_numeric_like_round_trip` | subsuming tests also assert value stays `String` (stronger) |
| `core/.../emit_tests.rs:170` | `round_trip_numbers` | `number_edge_tests.rs:99 emitted_number_representation_matches_parse` | case set includes 42 / 3.14 with per-key `v1==v2` |
| `core/.../emit_tests.rs:129` | `emit_twice_is_byte_equal` | `emit_stability_tests.rs` (fixture content) + fuzz idempotence | hardcoded determinism smoke |
| `core/.../number_edge_tests.rs:43` | `string_that_looks_like_scientific_notation_round_trip` | `ambiguous_strings_tests.rs:123 ambiguous_numeric_like_round_trip` | string-side only; keep the numeric-side tests in this file |
| `core/.../number_edge_tests.rs:62` | `string_hex_like_round_trip` | same | |
| `core/.../dto.rs:1101` | `v0_82_0_payload_migrates_forward` | `dto.rs:1278 v0_82_0_payload_loads_via_migration` (adds schema-version re-check) | strictly stronger sibling |

## Tier 3 — the richtext single-construct round-trip cluster (consolidate)

`export.rs` has ~12 deterministic single-construct round-trip tests, each of
which is one arm of the `document()` generator that `properties.rs:163
corpus_round_trip_and_invariants` fuzzes:

`paragraph:882`, `two_paragraphs:887`, `marks:892`, `heading:902`,
`inline_code:907`, `bullet_list:917`, `ordered_list:922`,
`multi_paragraph_item:927`, `blockquote:932`, `link:937`, `table:942`,
`image:962`.

Recommendation: **collapse to one or two seed-free smoke tests** rather than
delete outright — they give fast, deterministic failure localization the
proptest doesn't. Do **not** touch the neighbours that are *not* generator
arms and have zero property coverage: `thematic_break:946/:951`,
`hard_break_in_list_item:977`, `nested_marks:897` (source-level nesting the
space-joined token generator never emits).

## Tier 4 — fuzz targets that are unit tests in a proptest costume

- `convert_fuzz.rs:255–509` — ~20 formatting-combination proptests
  (`fuzz_bold_single`, nested/adjacent combos, 3-way, intraword). Content is
  random `[a-zA-Z0-9]` wrapped in **fixed** markers; the entropy never
  influences the marker→`#strong[` mapping, and several pairs assert identical
  `contains()` checks (`fuzz_bold_then_italic` ≡ `fuzz_bold_containing_italic`).
  → collapse to a handful of table-driven example tests (the `regression`
  cases at `:473–509` already do this better).
- `parse_fuzz.rs` — `fuzz_decompose_{valid_payload:29, malformed_yaml:63,
  nested_structures:88, special_characters:107, unicode:121}` are no-op
  "wrap-in-card, `from_markdown`, discard" variants subsumed by
  `fuzz_decompose_no_panic:6` (its arbitrary input space already includes
  card-fenced strings). Keep only the two with real invariants
  (`large_payload:71`, `multiple_cards:133`). `emit_roundtrip_fuzz` further
  subsumes the parse-no-panic coverage.

## Tier 5 — low-confidence / judgment (flagged, not recommended)

- `core/.../edit_tests.rs:378` `test_card_set_field_valid` — the string case is
  a strict subset of `test_set_field_scalar_conversions:487` (which does
  string+int+float+bool+array). Mild REDUNDANT; deletable but cheap.
- `core/.../assemble_tests.rs:810` `test_quill_with_card_blocks` — subsumed by
  `test_basic_card_block:287`.
- `core/.../assemble_tests.rs:1150` `test_lone_triple_dash_in_body_is_delegated`
  — near-dup of `:1128`; keep one (the two constructs parse differently in
  CommonMark).
- `core/.../lossiness_tests.rs:679` `mixed_inline_comments_round_trip` —
  integration of three cases each covered singly; residual value is
  cross-interference.
- `core/src/quill/tests.rs:1056 test_quill_with_all_ui_properties` — strict
  subset of `test_field_order_preservation:1007`.
- `core/src/quill/tests.rs:1081 test_field_schema_with_description` — halves
  duplicate `test_field_schema_struct:562` and the ordering tests.
- `core/src/quill/schema_yaml.rs:64 omits_ref` — near-tautological (no code path
  emits `ref:`); weak regression guard.
- `richtext/src/delta.rs:478/:484`, `normalize.rs:150/206/214`,
  `wasm/tests/wasm_bindings.rs:39 test_quill_from_tree` — assorted thin smokes;
  keep-or-fold, low stakes.
- `quillmark/tests/quill_engine_test.rs:63
  test_quill_render_succeeds_with_engine_loaded_quill` — bare `is_ok()` render
  over a synthetic quill, subsumed by `quiver_test::every_quill_in_quiver_renders`.

## Logic (not tests) — surface, don't auto-cut

- `core/src/document/dto.rs` — legacy wire formats `V0_81_0` / `V0_82_0` /
  `V0_92_0`, all "read + migrate forward only." Intentional back-compat with a
  migration-test spine. **Question worth raising:** this branch is a
  pre-release reboot with no shared history to `main`; if `0.81`/`0.82`
  documents were never released from *this* lineage, the read-only migration
  chain (and its tests) may be droppable. Needs a product call, not a
  mechanical cut.
- `core/src/quill/types.rs:170 RICHTEXT_INLINE_TOKEN_MSG` and
  `markdown_field_test.rs::test_markdown_type_is_a_load_error` — migration
  shims rejecting retired `type: richtext(inline)` / `type: markdown` tokens.
  Keep for the transition window; candidates to retire once the window closes.
- `core/src/quill/validation.rs:716` — stale comment referencing a
  `validates_array_of_objects` test that does not exist. Fix the comment.

## Cross-cutting redundancy that is BY DESIGN (do not "fix")

- **Binding parity.** The DTO triad (`round_trip` / `rejects_invalid` /
  `drops_warnings`) and name-mismatch rejection are asserted in Rust core,
  WASM (`wasm_bindings.rs`), and Python (`test_parse.py`/`test_render.py`).
  This is intentional per-boundary coverage; the Rust core test is
  authoritative. `wasm_bindings.rs`'s unique value is the serialization-shape
  tests (`test_artifact_bytes_is_uint8array:87`) — keep those if ever trimming.
- **`spec_conformance_probe.rs`** deliberately duplicates unit tests as an
  independent external-crate probe against pulldown-cmark. Keep intact.
- **`quiver_test.rs`** enumerates *all* quills via `read_dir`, so no quill is
  truly dead — `classic_resume` (1 doc-comment ref) and `cmu_letter` (0 refs)
  are still load/render/blueprint-swept. `cmu_letter` (640 KB, 5 TTF fonts) is
  the only letter-with-SVG-asset shape; removing it shrinks quiver breadth.
  Treat as demo templates, not dead fixtures — drop only if breadth isn't
  valued.

---

### Suggested execution order

1. Tier 1 (#1–8) — mechanical, zero coverage loss.
2. Tier 2 — deletions with named stronger siblings.
3. Tier 3 + Tier 4 — consolidations (rewrite, not raw delete).
4. Fix the `validation.rs:716` stale comment.
5. Raise the `dto.rs` legacy-wire-format question as a separate issue.

Tiers 1–2 are ~19 tests + 4 fixture files; with the Tier 3/4 consolidations
this is a focused, low-risk cleanup that keeps every real invariant covered.
