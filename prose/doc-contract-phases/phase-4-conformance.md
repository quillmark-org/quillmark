# Phase 4 — conformance suite, frozen at 1.0

> **Gate**: phases 1–3 settled. Fixtures written against a moving surface
> get written twice.

## Goal

The general cure for "green upstream tests, broken pinned consumer": a
versioned fixture set both repos run in CI. Freezing it is what 1.0
*means* — post-1.0 a fixture diff is a breaking change by definition;
pre-1.0 the diffs are the pivot log.

## Fixture format — the actual design work

- **Operation scripts, not just snapshots.** "Expected mutator errors"
  means fixtures encode operations: apply `removeField x` → expect
  `edit::unknown_field` at path *P*. The format is a small operation-script
  DSL over `(document + schema)`, with expectations on the resolved view
  (`fieldStates()` values + sources), validation diagnostics, and mutator
  errors.
- **Assert codes, paths, sources, values — never message text.** A message
  copyedit must not be a formal break, or fixture diffs get rubber-stamped
  and the freeze signal dies. The Typst `typst::<message-prefix>`
  convention — identity derived from prose — is exactly what the frozen
  set excludes.
- Every grammar edge ruled in phase 2 (card bodies, `$ext`, nested indices)
  and both branches of every phase-3 constraint appear as fixtures.

## Delivery

- A lockstep package (e.g. `@quillmark/conformance`) published from the
  same commit as `@quillmark/wasm` — pinning works identically, without
  taxing runtime consumers with fixture bytes. The editor pins both and
  runs the suite against its own integration layer.
- Engine-side, the suite runs in the workspace tests; a fixture change in a
  PR is reviewable as a contract change, not a test update.

## Rider: `contractVersion`

A constant on the WASM surface, semver'd over
`{diagnostic taxonomy, path grammar, fieldStates shape}` independently of
crate semver, so the editor asserts compatibility at load time rather than
at bug-report time. Its value is mostly pre-1.0 — a contract-stability
signal while crate versions break freely; post-freeze, package semver plus
the fixtures nearly subsume it. A rider here, not a centerpiece.

## Acceptance

- Both repos run the suite green from the same published fixture version.
- No fixture asserts message text.
- The 1.0 release notes name the fixture freeze as the release's meaning;
  the migration-guide policy for post-1.0 fixture diffs is written down.
