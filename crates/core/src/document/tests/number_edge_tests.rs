//! Targeted number-edge tests
//!
//! `QuillValue::Number` and the emitter agree on representation.
//!

use crate::document::Document;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn assert_round_trip(label: &str, src: &str) {
    let a = Document::from_markdown(src)
        .unwrap_or_else(|e| panic!("{}: from_markdown failed: {}", label, e));
    let emitted = a.to_markdown();
    let b = Document::from_markdown(&emitted)
        .unwrap_or_else(|e| panic!("{}: re-parse failed: {}\nEmitted:\n{}", label, e, emitted));
    assert_eq!(
        a, b,
        "{}: round-trip produced different Documents.\nEmitted:\n{}",
        label, emitted
    );
}

// ── 1e10 (scientific-notation float) ─────────────────────────────────────────

/// `1e10` bare in YAML parses as the float `10_000_000_000.0`.
/// After round-trip the number value must be preserved.
#[test]
fn number_scientific_notation_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nbig: 1e10\n~~~\n";
    assert_round_trip("1e10", src);

    // The parsed value must be a number (not a string).
    let doc = Document::from_markdown(src).unwrap();
    let v = doc.main().payload().get("big").unwrap();
    assert!(
        v.as_f64().is_some(),
        "1e10 must parse as a number, got {:?}",
        v
    );
}

// ── Large integer (beyond i32) ────────────────────────────────────────────────

/// An integer beyond `i32::MAX` but within `i64::MAX` must round-trip correctly.
#[test]
fn large_integer_round_trip() {
    let src = "~~~card-yaml\n$quill: q\n$kind: main\nbig_int: 9999999999999\n~~~\n";
    assert_round_trip("large integer", src);

    let doc = Document::from_markdown(src).unwrap();
    let v = doc.main().payload().get("big_int").unwrap();
    assert_eq!(
        v.as_i64(),
        Some(9_999_999_999_999_i64),
        "9999999999999 must parse as i64, got {:?}",
        v
    );
}

// ── Emitter representation agreement ─────────────────────────────────────────

/// After emit, the numeric representation in the YAML output must be parseable
/// back to the same `serde_json::Number`.  We test with a representative set.
#[test]
fn emitted_number_representation_matches_parse() {
    struct Case {
        src_value: &'static str,
        key: &'static str,
    }

    let cases = [
        Case {
            src_value: "42",
            key: "count",
        },
        Case {
            src_value: "3.14",
            key: "pi",
        },
        Case {
            src_value: "0",
            key: "zero",
        },
        Case {
            src_value: "-7",
            key: "neg",
        },
        Case {
            src_value: "9999999999999",
            key: "big",
        },
    ];

    for case in &cases {
        let src = format!(
            "~~~card-yaml\n$quill: q\n$kind: main\n{}: {}\n~~~\n",
            case.key, case.src_value
        );
        let doc = Document::from_markdown(&src).unwrap();
        let emitted = doc.to_markdown();
        let doc2 = Document::from_markdown(&emitted).unwrap_or_else(|e| {
            panic!(
                "re-parse failed for {}: {}\nEmitted:\n{}",
                case.src_value, e, emitted
            )
        });
        let v1 = doc.main().payload().get(case.key).unwrap();
        let v2 = doc2.main().payload().get(case.key).unwrap();
        assert_eq!(
            v1, v2,
            "number {} changed representation after emit/re-parse\nEmitted:\n{}",
            case.src_value, emitted
        );
    }
}
