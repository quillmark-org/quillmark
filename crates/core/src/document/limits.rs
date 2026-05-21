//! Size and depth budget constants for document parsing.
//!
//! These constants govern the maximum sizes and counts accepted during parsing
//! to prevent denial-of-service via excessively large or deeply nested input.

/// Maximum YAML nesting depth (100 levels).
///
/// Prevents stack overflow from deeply nested YAML structures.
/// Enforced at the serde-saphyr parser level via [`serde_saphyr::Budget`].
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
