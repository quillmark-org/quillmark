# QuillValue - Centralized Value Type

> **Implementation**: `crates/core/src/`

## TL;DR

`QuillValue` is Quillmark's one canonical value representation: an annotated tree that pairs JSON-shaped data with a per-node `!must_fill` flag, plus a lazily materialized, fill-free `serde_json::Value` projection. YAML and JSON are the only input formats; conversion happens at system boundaries.

## Design

- **Annotated tree** — each node carries a `fill` bool (the in-memory form of the `!must_fill` YAML tag) alongside JSON-shaped data (null/bool/number/string/array/object).
- **JSON projection** — `as_json()` / `into_json()` / `Deref<Target = serde_json::Value>` expose a fill-free `serde_json::Value` view, materialized on first use and cached.
- **Boundary conversion** — YAML/JSON convert to `QuillValue` once, at entry points.
- **Opaque type** — wraps the tree to add domain methods (fill tracking) and control the surface API; the JSON projection is derived, never a second source of truth.

## Methods

- **Constructors:** `string()`, `integer()`, `bool()`, `null()`
- **Conversion:** `from_yaml_str()`, `from_json()`, `as_json()`, `into_json()`
- **Fill tracking:** `fill()`, `with_fill()`, `set_fill()`, `fill_paths()`, `nonroot_fill_paths()`, `set_fill_at()`, `is_object_at()`
- **Delegating:** `is_null()`, `as_str()`, `as_bool()`, `as_i64()`, `as_u64()`, `as_f64()`, `as_array()`, `as_object()`, `get(key)` (wraps the result in `QuillValue`, preserving the child's fill marker)

Implements `Deref<Target = serde_json::Value>` for transparent access to JSON methods.

## Usage

Quill metadata and schemas, parsed document fields (including `!must_fill` markers), field default/example values, and FFI boundaries (Python, WASM).
