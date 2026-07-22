# Phase 1 — `edit::` diagnostic codes

> **Gate**: none. Mechanical via the existing `variant_name()` seam
> (`crates/core/src/document/edit.rs:128`); no design work.

## Goal

Every mutator failure is a `Diagnostic` with a stable, namespaced `code`,
in both bindings. The `[EditError::…]` message-prefix convention is deleted
outright — pre-1.0, no shadow contract left for anyone to parse.

## Code map

One code per `EditError` variant (`crates/core/src/document/edit.rs:56`):

| Variant | Code |
|---|---|
| `InvalidFieldName` | `edit::invalid_field_name` |
| `UnknownField` | `edit::unknown_field` |
| `InvalidKindName` | `edit::invalid_kind_name` |
| `ReservedKind` | `edit::reserved_kind` |
| `IndexOutOfRange` | `edit::index_out_of_range` |
| `ValueTooDeep` | `edit::value_too_deep` |
| `Import` | `edit::import` |
| `FieldRichtextDecode` | `edit::field_richtext_decode` |
| `FieldRichtextNotInline` | `edit::field_richtext_not_inline` |
| `FieldConform` | `edit::field_conform` |
| `ContentApply` | `edit::content_apply` |

## Work

- Core: an `EditError → code` mapping beside `variant_name()` (or derived
  from it), so both bindings share one producer.
- WASM: `edit_error_to_js` / `edit_errors_to_js`
  (`crates/bindings/wasm/src/engine.rs:1829,1834`) set `code`; the message
  drops the prefix and keeps the display text. Thrown JS `Error`s keep
  `.diagnostics[]`; each entry carries `{severity, code, message, path?}`.
- Python: `convert_edit_error` / `convert_edit_errors`
  (`crates/bindings/python/src/errors.rs:14,27`) — same treatment. Python is
  in scope here, not deferred: same seam, and the prefix is documented API
  in the Python README, so the migration guide covers it either way.
- Canon: add `edit::` to ERROR.md's namespace list.
- Migration: entry in the unreleased `docs/migrations/` guide — prefix
  removal, code table, route-on-`code` guidance.

**No `path` work in this phase.** The batched-mutator twins keep the bare
field-name `path` they already set; everything else waits for `DocPath`
(phase 2). Hand-assembled paths here would be re-serialized later —
the disease this rework exists to cure. Mutator paths also need call-site
plumbing the code doesn't carry yet: `IndexOutOfRange` knows an index but
not which card array, `FieldConform` a field but not which card.

## Rider: ratify null ≡ absent

Standalone doc edit, no dependencies. `SCHEMAS.md`'s null ≡ absent rule
makes "explicitly cleared" and "never touched" indistinguishable, so there
is no uniform "blank, not default" for non-string types and `removeField`
stays the sole unset verb. State it in canon as a chosen 1.0 commitment —
YAML round-trip sanity and the simpler model, tri-state foreclosed.

## Acceptance

- Every `EditError` surfaces with its `edit::` code in WASM and Python.
- No `[EditError::` substring anywhere in the workspace or its test
  expectations.
- ERROR.md lists `edit::`; the unreleased migration guide covers the break.
- Consumer routing (coercion vs. undeclared — `edit::field_conform` vs.
  `edit::unknown_field`) needs only `code`, never message text.

## Status

**Shipped** at `4162ec2`: `EditError::code()` in core beside `variant_name()`,
both binding converters set `code` and drop the prefix, `edit::*` in ERROR.md's
namespace list with its own section, the migration entry, and the null ≡ absent
ratification in `SCHEMAS.md`. All acceptance criteria hold; nothing remains.
