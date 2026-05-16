# Proposal: simplifications opened by the example-removal cleanup

Surfaced from a post-merge review of `chore/integration` (PRs #582–#586 plus
the bundled-example-fixture removal). The example/`example_file` removal was
itself a clean simplification cascade — one insight ("the generated blueprint
is the universal default document") deleted a field, two getters, the
example-file loader, the path-traversal guard, the auto-pickup, two
`Quill.yaml` keys, and ~930 lines net. These are the small residual items it
left in its wake.

Capture only — to be implemented in a follow-up session, not in the
integration branch.

## 1. Collapse `QuillSource.name` / `backend_id` into config-backed getters

`crates/core/src/quill.rs:36-37` — `QuillSource` carries `name` and
`backend_id` as owned `String` fields, populated in
`crates/core/src/quill/load.rs:102-103` as verbatim `config.name.clone()` /
`config.backend.clone()`. Nothing ever mutates them. After the example field
was removed, `config` and `files` are the only genuine source data on the
struct; `name`/`backend_id` are pure caches of `config`.

Make `name()` / `backend_id()` (`quill.rs:45-52`) return `&self.config.name`
/ `&self.config.backend` and drop the two fields. Removes two struct fields
and two clones, with no public API change (`&str` in, `&str` out).

`metadata` is also a projection of `config`, but it is a `HashMap` exposed by
reference to the bindings — converting it to a computed getter is a larger,
separate question and is out of scope here.

## 2. Do not extract a file-loader helper in `load.rs`

`crates/core/src/quill/load.rs:81-98` — before the example removal,
`from_config` had two structurally identical blocks (`get_file` →
`from_utf8` → `diag`) for the plate and the example. That duplication would
once have justified a `load_text_file(root, name, code)` helper. Only the
plate block remains. This note exists so a future reviewer does not
re-introduce an abstraction for a single call site.

## 3. CLI command scaffolding duplication

The `specs` command added in #583 (`crates/bindings/cli/src/commands/specs.rs`)
is the fifth copy of the same opening: an `if !path.exists()` check followed
by `Quillmark::new().quill_from_path(...)`. The check now appears in
`render.rs:46`, `specs.rs:20`, `schema.rs:20`, `info.rs:23`, and
`validate.rs:91`.

`quill_from_path` (`crates/quillmark/src/orchestration/engine.rs:45`) already
errors on a missing path; the explicit check only upgrades the message from
`"Failed to load quill: No such file or directory"` to
`"Quill directory not found: <path>"`. `specs` and `schema` are the tightest
pair — identical but for one line (`blueprint()` vs `schema_yaml()`) — and
both also share the "write to file or stdout" tail.

A shared helper — load-quill-with-friendly-error plus emit-to-path-or-stdout
— would collapse `specs` and `schema` to their one differing line and let
`render` / `info` / `validate` reuse the load step. Worth doing now that the
pattern has five instances.
