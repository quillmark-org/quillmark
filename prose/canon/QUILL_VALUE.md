# QuillValue - Centralized Value Type

> **Implementation**: `crates/core/src/`

## TL;DR

`QuillValue` is a newtype over `serde_json::Value` that gives Quillmark one canonical representation for metadata and fields. YAML and JSON are the only input formats; conversion happens at system boundaries.

## Design

- **JSON foundation** — `serde_json::Value` backing for ecosystem support.
- **Boundary conversion** — YAML/JSON convert to `QuillValue` once, at entry points.
- **Newtype** — wraps JSON to add domain methods and control the surface API.

```rust
pub struct QuillValue(serde_json::Value);
```

## Methods

- **Constructors:** `string()`, `integer()`, `bool()`, `null()`
- **Conversion:** `from_yaml_str()`, `from_json()`, `as_json()`, `into_json()`
- **Delegating:** `is_null()`, `as_str()`, `as_bool()`, `as_i64()`, `as_u64()`, `as_f64()`, `as_array()`, `as_object()`, `get(key)` (wraps the result in `QuillValue`)

Implements `Deref<Target = serde_json::Value>` for transparent access to JSON methods.

## Usage

Quill metadata and schemas, parsed document fields, field default/example values, and FFI boundaries (Python, WASM).
