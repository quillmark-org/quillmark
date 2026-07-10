//! Regression corpus for YAML-ambiguous string values.
//!
//! Every string in this module is "dangerous" to a YAML parser that lacks
//! type-fidelity guarantees: bare `on`, `01234`, `2024-01-15`, etc. would be
//! silently coerced to booleans, integers, or dates by a YAML 1.1 parser, or
//! misread as anchors/aliases/tags by any YAML parser.
//!
//! The canonical emitter (§9) double-quotes every string scalar with
//! JSON-style escaping, which is what buys the round-trip guarantee tested
//! here.
//!

use crate::document::Document;

// ── Fixture path ──────────────────────────────────────────────────────────────

fn ambiguous_strings_fixture() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(manifest_dir)
        .join("..")
        .join("fixtures")
        .join("resources")
        .join("ambiguous_strings.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Cannot read ambiguous_strings.md at {}: {}",
            path.display(),
            e
        )
    })
}

/// Parse the fixture and return the document.
fn parse_fixture() -> Document {
    let src = ambiguous_strings_fixture();
    Document::from_markdown(&src)
        .unwrap_or_else(|e| panic!("ambiguous_strings.md failed to parse: {}", e))
}

/// Assert that a payload field is `QuillValue::String` with exactly the
/// expected bytes, then perform a round-trip and assert byte-identical.
fn assert_string_field_round_trips(doc: &Document, key: &str, expected: &str) {
    let value = doc
        .main()
        .payload()
        .get(key)
        .unwrap_or_else(|| panic!("field '{}' not found in payload", key));

    // Must be a string, not a bool / number / null.
    assert!(
        value.as_str().is_some(),
        "field '{}': expected QuillValue::String, got {:?}",
        key,
        value
    );
    assert_eq!(
        value.as_str().unwrap(),
        expected,
        "field '{}': string value mismatch",
        key
    );
}

/// Full round-trip: parse → emit → re-parse → assert each field still string.
fn assert_round_trip_strings(keys_and_values: &[(&str, &str)]) {
    let doc = parse_fixture();
    let emitted = doc.to_markdown();
    let doc2 = Document::from_markdown(&emitted)
        .unwrap_or_else(|e| panic!("re-parse after emit failed: {}\nEmitted:\n{}", e, emitted));

    for (key, expected) in keys_and_values {
        // First parse: string type.
        assert_string_field_round_trips(&doc, key, expected);
        // Re-parsed: byte-identical.
        let v2 = doc2
            .main()
            .payload()
            .get(key)
            .unwrap_or_else(|| panic!("field '{}' missing after round-trip", key));
        assert!(
            v2.as_str().is_some(),
            "field '{}' is not a string after round-trip; value = {:?}",
            key,
            v2
        );
        assert_eq!(
            v2.as_str().unwrap(),
            *expected,
            "field '{}': value changed on round-trip",
            key
        );
    }
}

// ── Category: Word booleans ───────────────────────────────────────────────────

/// `on`, `off`, `yes`, `no`, `true`, `false` are YAML 1.1 booleans.
/// Quillmark always emits them double-quoted so they re-parse as strings.
#[test]
fn ambiguous_word_booleans_round_trip() {
    assert_round_trip_strings(&[
        ("on_word", "on"),
        ("off_word", "off"),
        ("yes_word", "yes"),
        ("no_word", "no"),
        ("true_word", "true"),
        ("false_word", "false"),
    ]);
}

// ── Category: Null-like strings ───────────────────────────────────────────────

/// `null` and `~` parse as YAML null in many parsers.
#[test]
fn ambiguous_null_like_round_trip() {
    assert_round_trip_strings(&[("null_word", "null"), ("tilde", "~")]);
}

// ── Category: Numeric-like strings ───────────────────────────────────────────

/// `01234` (octal-like), `1e10` (scientific notation), `0x1F` (hex-like).
/// A YAML 1.1 parser would silently coerce these to integers or floats.
#[test]
fn ambiguous_numeric_like_round_trip() {
    assert_round_trip_strings(&[
        ("leading_zeros", "01234"),
        ("exponential", "1e10"),
        ("hex_like", "0x1F"),
    ]);
}

// ── Category: Date-like string ────────────────────────────────────────────────

/// ISO 8601 date strings look like YAML dates in YAML 1.1.
#[test]
fn ambiguous_iso_date_round_trip() {
    assert_round_trip_strings(&[("iso_date", "2024-01-15")]);
}

// ── Category: Special characters ─────────────────────────────────────────────

/// Empty string, single space, embedded newline, embedded quote, backslash.
#[test]
fn ambiguous_special_characters_round_trip() {
    assert_round_trip_strings(&[
        ("empty_string", ""),
        ("single_space", " "),
        ("embedded_newline", "line1\nline2"),
        ("embedded_quote", "he said \"hi\""),
        ("embedded_backslash", "a\\b"),
    ]);
}

// ── Category: YAML syntax strings ────────────────────────────────────────────

/// Strings that look like YAML structural tokens: map entries, sequence
/// markers, comments, anchors, aliases, tags.
#[test]
fn ambiguous_yaml_syntax_round_trip() {
    assert_round_trip_strings(&[
        ("looks_like_map", "key: value"),
        ("looks_like_seq", "- item"),
        ("hash_comment", "#comment"),
        ("yaml_anchor", "&anchor"),
        ("yaml_alias", "*alias"),
        ("yaml_tag", "!tag"),
    ]);
}

