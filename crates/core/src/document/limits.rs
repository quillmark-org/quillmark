//! Nesting-depth budget for document parsing.
//!
//! [`MAX_YAML_DEPTH`] governs the maximum container nesting accepted during
//! parsing, preventing denial-of-service via deeply nested input. Sibling
//! size/count limits (input bytes, YAML bytes, card count, field count) live
//! in [`crate::error`].

/// Maximum nesting depth, counted in **container levels** (100).
///
/// The unit is container levels, not nodes: a value may nest up to this many
/// arrays/objects deep, and the scalar leaf at the bottom is *not* charged a
/// level. So `{"a":{"a":…{"a":1}}}` with exactly 100 objects is accepted, and
/// 101 objects is rejected — whether the deepest container is empty, holds a
/// scalar, or holds another container. Every ingestion boundary enforces this
/// identical shape: the YAML parser via [`serde_saphyr::Budget`], the payload
/// paths via [`crate::value::json_depth_exceeds`], and the bindings' own
/// converters (e.g. Python `py_to_json_at`).
///
/// Prevents stack overflow from deeply nested input.
pub const MAX_YAML_DEPTH: usize = 100;

/// serde-saphyr parse options carrying the depth budget.
///
/// Centralizes the [`serde_saphyr::Budget`] so every YAML entry point —
/// card-yaml payloads and `Quill.yaml` — enforces the same nesting limit.
pub(crate) fn yaml_parse_options() -> serde_saphyr::Options {
    serde_saphyr::Options {
        budget: Some(serde_saphyr::Budget {
            max_depth: MAX_YAML_DEPTH,
            ..Default::default()
        }),
        ..Default::default()
    }
}
