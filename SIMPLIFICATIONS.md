# Simplification opportunities — for review

Findings from a workspace-wide hygiene pass (code + docs audit, all claims
grep-verified). Low-risk items were applied directly in the accompanying
commits; everything below changes public API, behavior, or policy and needs a
maintainer decision. Ordered by expected payoff.

## Cascades

### 1. Flatten `RenderError` to `kind` + `diags`
`crates/core/src/error.rs:308`

All eight `RenderError` variants have the identical shape
`{ diags: Vec<Diagnostic> }`; the variant only encodes the *kind* of failure.
Restructuring to `RenderError { kind: RenderErrorKind, diags: Vec<Diagnostic> }`
(fieldless `RenderErrorKind` enum) eliminates:

- the two 8-arm matches in `diagnostics()` / `into_diagnostics()` (become field
  accesses),
- the variant split in the `Display` impl,
- per-variant construction boilerplate at every error site.

**Cost**: breaking change to a public enum; the wasm and python bindings
pattern-match these variants to map typed exceptions, so all three crates move
together. Best bundled into the next breaking release.

### 2. Express blueprint emission as Document construction + `emit`
`crates/core/src/quill/blueprint.rs` vs `crates/core/src/document/emit.rs`

Both modules hand-roll block-style YAML container walkers (indentation,
mappings, sequences) on top of the shared scalar emitters. Blueprint output is
already a parseable Document (`blueprint_round_trips_idempotently` asserts
this), so blueprint generation could build a synthetic `Document`/`Payload`
(annotations riding as comments) and call the one emitter. Eliminates the
duplicated walker in `blueprint.rs` (`write_value`, `write_array_items`,
`write_typed_object_field`, `write_typed_table_field`) — several hundred lines.

**Cost**: the annotation grammar (`# e.g.` trailers, `<must-fill>` sentinels)
is intricate; needs careful test-by-test migration. High effort, high payoff.

### 3. Collapse the `compile_to_{pdf,svg,png}` trio onto `render_document_pages`
`crates/backends/typst/src/compile.rs:72-152`

Two parallel implementations of "compiled document → output bytes" exist.
`compile_to_svg` / `compile_to_png` have zero callers anywhere;
`compile_to_pdf` is called only by tests (`producer_meta.rs`, `sig_field.rs`),
which can use the `Backend::open` + `RenderSession::render` path instead (two
of the four tests already do). Deleting the trio removes ~80 lines including a
duplicated PNG-encode error block and a duplicated overlay extract/inject
sequence.

**Cost**: the three functions are `pub` in the published `quillmark-typst`
crate, so removal is a (minor) API break — though they are outside the
documented `Backend` flow.

### 4. Let `OutputFormat` own its string id and MIME type
`crates/core/src/types.rs` + three bindings

The format↔string and format↔MIME tables are hand-maintained four times:

- `crates/bindings/python/src/enums.rs:59` (format mapping) and
  `types.rs:779` (MIME),
- `crates/bindings/wasm/src/types.rs:22` (format) and `:175` (MIME),
- `crates/bindings/cli/src/commands/render.rs:98` (inline string parse).

Adding `mime_type()`, `FromStr`, and a variants slice to
`quillmark_core::OutputFormat` is additive; the four matches become forwarding
calls. Same pattern for the `EditError` variant-name prefix strings duplicated
in `wasm/src/engine.rs:950` and `python/src/errors.rs:14` — a `Display`/method
on core `EditError` collapses both.

**Cost**: low; additive core change plus mechanical binding rewires. Deferred
only because it spans four crates.

## Behavior-preserving refactors

### 5. Make `normalize_document` infallible
`crates/core/src/normalize.rs:174`

The function always returns `Ok` (its companion error enum was dead and has
been removed). Changing the signature to return `Document` drops the `?` at
`quill/compose.rs:24` and `unwrap()`s in tests — but `normalize` is a
`pub mod`, so this is a public-API signature change.

### 6. Unify the identifier-validation predicates
`crates/core/src/document/edit.rs:27`, `document/meta.rs:165`,
`quill/config.rs:870`

`is_valid_field_name`, `is_valid_kind_name`, and `is_snake_case_identifier`
(plus `is_valid_card_identifier` / `is_valid_quill_name`) all check the same
`[a-z_][a-z0-9_]*` charset family, differing only in leading-underscore policy
and NFC normalization. One parameterized helper covers all three; existing
tests pin the per-call-site flags.

### 7. Simplify `build_payload`'s `$`-item splice
`crates/core/src/document/assemble.rs:373-472`

The closed 4-key `$` set is routed through a `HashMap<&'static str, _>` plus
two separate "in source order" reconstruction passes. A direct linear scan of
the ≤4-element `meta_items` slice removes the map and the leftover-drain loop.
Parser core, well-covered by `assemble_tests`, but ordering is subtle — review
carefully.

### 8. Deduplicate `transform_markdown_fields` projections
`crates/backends/typst/src/lib.rs:249-264, 339-372`

The content-field and date-field collection blocks (top-level and per-card
`$defs`) are two copies of the same "filter properties by predicate → collect
names" shape. One `collect_field_names(props, predicate)` helper plus a single
`$defs` loop building both maps. Covered by existing `test_transform_*` tests.

## Public-API surface that is unused in-repo

Each is `pub` and reachable by downstream crates.io consumers, so removal is
an API decision, not dead-code cleanup:

- `FileTreeNode::print_tree` (`crates/core/src/quill/tree.rs:220`) — debug
  ASCII-tree renderer, zero callers.
- `Payload::take_quill` (`crates/core/src/document/payload.rs:359`) — zero
  callers, and using it produces a main card violating the "main must carry
  `$quill`" invariant. If kept, document the foot-gun; otherwise remove.
- `SCHEMA_V0_81_0` (`crates/core/src/document/dto.rs:46`) — exported constant
  with no reader (the serde rename uses a string literal). Arguably kept as
  the documented legacy-schema identifier; harmless either way.
- The `quillmark` facade re-exports `Artifact`, `Backend`, `Card`,
  `ParseError`, `ParseOutput`, `Severity` (`crates/quillmark/src/lib.rs:24`)
  that nothing in-repo imports via the facade. Decide the policy: "facade
  mirrors the full core surface" (keep, fine) or "facade is the minimal
  documented set" (prune six re-exports).

## Repo/packaging decisions

- **npm license mismatch**: `crates/bindings/wasm/package.template.json`
  declares `"MIT OR Apache-2.0"` but the workspace is Apache-2.0-only with a
  single `LICENSE` file; `scripts/build-wasm.sh:124-129` tries to copy
  `LICENSE-MIT` / `LICENSE-APACHE` files that don't exist (both guards are
  always false), and the package `files` list ships no license at all.
  Reconcile to `Apache-2.0`, copy the real `LICENSE`, and add it to the
  template `files` array — unless dual licensing is actually intended.
- **Python 3.13 wheels**: `.github/workflows/release.yml` builds wheels with
  `--interpreter 3.10 3.11 3.12` while `pyproject.toml` advertises a 3.13
  classifier. Add 3.13 to the matrix or drop the classifier. (abi3-py310 may
  make the extra interpreter moot — verify before changing.)
- **Unreferenced fixture resources**: `crates/fixtures/resources/`
  `card_yaml_demo.md`, `sample.md`, `taro.png`, `versioned_letter_latest.md`,
  `versioned_resume_exact.md`, `versioned_resume_major.md` have no by-name
  reference in any source file. Verify nothing loads them via constructed
  paths, then delete.
- **Landed proposal**: `prose/proposals/mcp-feedback.md` has no
  superseded/landed banner, but its substance (the two-cell Endorsed/Must-Fill
  model) is now canon (SCHEMAS.md, BLUEPRINT.md; migration 0.83-to-0.84).
  Per `prose/README.md` ("proposals removed once landed or abandoned"), delete
  it or add a "Landed in 0.84" banner.
