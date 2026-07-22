# Conformance Suite

> **Implementation**: `crates/conformance/` (engine runner) ·
> `conformance/` (fixtures + `@quillmark/conformance` package)

The engine↔consumer boundary carries contract surfaces — one diagnostic
envelope, one address grammar, a resolved-value view, a typed surface. The
conformance suite freezes them behind an executable, cross-repo check: one
versioned fixture file both `borb-sh/quillmark` and its pinned consumers run in
CI. It is the general cure for "green upstream tests, broken pinned consumer" —
the disease the document-contract rework diagnosed (`prose/doc-contract-phases/`).

Freezing the set is what the 1.0 release **means**. Post-1.0 a fixture diff is a
breaking change by definition; pre-1.0 the diffs are the pivot log.

## The fixture file

`conformance/conformance.json` is the single language-neutral source of truth —
the engine embeds it (`include_str!`), the npm package ships it verbatim. It
carries a `contractVersion` stamp, the quills fixtures build against (each a
`Quill.yaml`-only file tree — the suite never renders), and two fixture arrays.

**State fixtures** (`fixtures`) are an operation-script DSL over
`(document + schema)`: parse a document, replay `steps` (the WASM `Document`
verbs — `storeField`, `commitField`, `insertCard`, …), then assert on the
resolved view (`fieldStates()` values + sources, in declaration order),
validation diagnostics, and mutator errors. A step's `error` asserts an
`edit::*` failure with its `code` and `DocPath`; its absence asserts success.

**Grammar fixtures** (`paths`) pin the `DocPath` grammar: a path string, the
`DocPathSeg[]` it parses to, and — for a geometry address — the plate-space
form (`$cards.<kind>.<ordinal>`) it translates from. The parse + round-trip is
the universal check both repos run; the plate translation is the engine-side
seam (a consumer reads already-translated addresses, so it runs only the parse
half).

## Codes, paths, sources, values — never message text

A fixture asserts a diagnostic's `code`, `path`, and `severity`, a field's
`value` and `source`, a path's segments — never a `message`. A message copyedit
must not be a formal break, or fixture diffs get rubber-stamped and the freeze
signal dies. The Typst `typst::<message-prefix>` convention — identity derived
from prose ([ERROR.md](ERROR.md)) — is exactly what the frozen set excludes; the
fixture format carries no message field, so the discipline is structural, not a
review rule.

## Two runners, one set

- **Engine** — `crates/conformance` replays each fixture against
  `quillmark-core` (the same producers the WASM surface serializes) and runs in
  the workspace tests (`cargo test --workspace`). A fixture change in a PR is
  reviewable as a contract change, not a test update.
- **Consumer** — `@quillmark/conformance` ships the identical JSON with a runner
  over an adapter the consumer supplies (its `@quillmark/wasm` integration
  layer). Published lockstep with `@quillmark/wasm` from the same commit; a
  consumer pins both. This keeps fixture bytes off runtime consumers while the
  editor asserts the contract against its own layer.

Acceptance: both repos run the suite green from the same published fixture
version.

## `contractVersion`

`quillmark_core::CONTRACT_VERSION` (WASM `contractVersion()`) is semver'd over
`{diagnostic taxonomy, DocPath grammar, fieldStates shape}`, independently of
crate/package semver. A consumer asserts compatibility at load time rather than
at bug-report time, and the fixture set is stamped with the value it was frozen
against — the engine runner refuses a mismatch. Its value is mostly a pre-1.0
signal: while crate versions break freely, it moves only when a boundary surface
does. Post-freeze, package semver plus the fixtures nearly subsume it.

## Post-1.0 policy

Once frozen at 1.0, a change to `conformance.json` **is** a contract break: it
bumps the major (or `contractVersion`) and lands a `docs/migrations/` entry, the
same as any other breaking change. Adding a fixture that pins behavior the set
did not yet cover is additive; changing an existing assertion is not.
