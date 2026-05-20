//! Tests for `Document::to_markdown`.
//!
//! Coverage:
//! - Type-fidelity round-trip over the full fixture corpus.
//! - Stability (emit-twice byte-equal) smoke test.
//! - Unit tests for targeted value types and edge cases.

use crate::document::Document;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse → emit → re-parse and assert the two Documents are equal.
fn assert_round_trip(label: &str, src: &str) {
    let a = Document::from_markdown(src)
        .unwrap_or_else(|e| panic!("{}: from_markdown failed on original: {}", label, e));
    let emitted = a.to_markdown();
    let b = Document::from_markdown(&emitted).unwrap_or_else(|e| {
        panic!(
            "{}: from_markdown failed on emitted document.\nError: {}\nEmitted:\n{}",
            label, e, emitted
        )
    });
    assert_eq!(
        a, b,
        "{}: round-trip produced different Documents.\nEmitted:\n{}",
        label, emitted
    );
}

// ── Fixture corpus round-trip ─────────────────────────────────────────────────

/// Every top-level `.md` file under `crates/fixtures/resources` — files
/// without a root `~~~card-yaml` block are skipped at parse time.
#[test]
fn fixture_corpus_round_trip() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    let resources_dir = std::path::Path::new(manifest_dir)
        .join("..") // crates/core → crates
        .join("fixtures")
        .join("resources");

    let mut fixture_paths: Vec<std::path::PathBuf> = Vec::new();

    // Top-level resource .md files that have a root card-yaml block.
    for entry in std::fs::read_dir(&resources_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            fixture_paths.push(path);
        }
    }

    assert!(
        !fixture_paths.is_empty(),
        "no fixture files found — check paths"
    );

    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for path in &fixture_paths {
        let label = path.to_string_lossy();
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP {}: cannot read: {}", label, e);
                skipped += 1;
                continue;
            }
        };

        // Skip files that are not parseable Quillmark documents (no card-yaml block).
        match Document::from_markdown(&src) {
            Err(_) => {
                skipped += 1;
                continue;
            }
            Ok(a) => {
                let emitted = a.to_markdown();
                match Document::from_markdown(&emitted) {
                    Err(e) => {
                        failed += 1;
                        failures.push(format!(
                            "FAIL {}: re-parse failed: {}\nEmitted:\n{}",
                            label, e, emitted
                        ));
                    }
                    Ok(b) => {
                        if a == b {
                            passed += 1;
                        } else {
                            failed += 1;
                            failures.push(format!(
                                "FAIL {}: documents differ after round-trip.\nEmitted:\n{}",
                                label, emitted
                            ));
                        }
                    }
                }
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "Fixture round-trip failures ({} failed, {} passed, {} skipped):\n{}",
            failed,
            passed,
            skipped,
            failures.join("\n\n")
        );
    }

    assert!(
        passed > 0,
        "No fixtures passed round-trip — did all files get skipped?"
    );

    eprintln!(
        "fixture_corpus_round_trip: {} passed, {} skipped",
        passed, skipped
    );
}

// ── Stability smoke test ──────────────────────────────────────────────────────

#[test]
fn emit_twice_is_byte_equal() {
    let src = "\
~~~card-yaml
#@quill: test@1.0.0
#@kind: main
title: Stability Test
flags:
  - on
  - 'yes'
  - null
count: 42
nested:
  key: value
~~~

Body content here.
";
    let doc = Document::from_markdown(src).unwrap();
    let first = doc.to_markdown();
    let second = doc.to_markdown();
    assert_eq!(
        first, second,
        "to_markdown must be deterministic (byte-equal on repeated calls)"
    );
}

// ── Value type unit tests ─────────────────────────────────────────────────────

#[test]
fn round_trip_booleans() {
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\nflag_true: true\nflag_false: false\n~~~\n";
    assert_round_trip("booleans", src);
}

#[test]
fn round_trip_null() {
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\nnull_field: null\n~~~\n";
    assert_round_trip("null", src);
}

#[test]
fn round_trip_numbers() {
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\ncount: 42\nfloat: 3.14\n~~~\n";
    assert_round_trip("numbers", src);
}

#[test]
fn round_trip_string_ambiguous() {
    // These are the strings most likely to be mis-parsed as booleans/numbers.
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\nfield_on: \"on\"\nfield_yes: \"yes\"\nfield_01234: \"01234\"\n~~~\n";
    assert_round_trip("ambiguous strings", src);
}

#[test]
fn round_trip_nested_map() {
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\nsender:\n  name: Alice\n  city: Springfield\n~~~\n";
    assert_round_trip("nested map", src);
}

#[test]
fn round_trip_sequence() {
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\ntags:\n  - demo\n  - test\n~~~\n";
    assert_round_trip("sequence", src);
}

#[test]
fn round_trip_empty_sequence() {
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\nempty: []\n~~~\n";
    assert_round_trip("empty sequence", src);
}

#[test]
fn round_trip_cards() {
    let src = "\
~~~card-yaml
#@quill: q
#@kind: main
title: Test
~~~

Body text.

~~~card-yaml
#@kind: section
heading: Chapter 1
~~~

Card body here.
";
    assert_round_trip("cards", src);
}

#[test]
fn round_trip_card_empty_body() {
    let src = "\
~~~card-yaml
#@quill: q
#@kind: main
title: Test
~~~

~~~card-yaml
#@kind: empty_body_card
title: No body
~~~
";
    assert_round_trip("card with empty body", src);
}

#[test]
fn round_trip_string_with_escapes() {
    // String containing backslash and quotes — must survive as a string.
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\npath: \"C:\\\\Users\\\\test\"\n~~~\n";
    assert_round_trip("string with backslash", src);
}

#[test]
fn round_trip_multiline_string() {
    // A string containing a literal newline.
    let src = "~~~card-yaml\n#@quill: q\n#@kind: main\nbio: \"Line one\\nLine two\"\n~~~\n";
    assert_round_trip("multiline string", src);
}

#[test]
fn round_trip_quill_version_selectors() {
    for qref in &["q", "q@1", "q@1.2", "q@1.2.3", "q@latest"] {
        let src = format!("~~~card-yaml\n#@quill: {}\n#@kind: main\ntitle: t\n~~~\n", qref);
        assert_round_trip(&format!("quill ref {}", qref), &src);
    }
}

#[test]
fn empty_map_omitted_from_emit() {
    // After parsing a document where a field is an empty object,
    // the emitter should omit that field.
    use crate::value::QuillValue;
    use indexmap::IndexMap;

    let mut payload: IndexMap<String, QuillValue> = IndexMap::new();
    payload.insert(
        "empty_obj".to_string(),
        QuillValue::from_json(serde_json::json!({})),
    );
    payload.insert(
        "real_field".to_string(),
        QuillValue::from_json(serde_json::json!("hello")),
    );

    use crate::document::{Card, CardMetadata, Payload};
    let meta = CardMetadata {
        quill: Some("test".parse().unwrap()),
        ..CardMetadata::default()
    };
    let main = Card::from_parts(meta, Payload::from_index_map(payload), String::new());
    let doc = crate::document::Document::from_main_and_cards(main, vec![], vec![]);

    let md = doc.to_markdown();
    assert!(
        !md.contains("empty_obj"),
        "empty object should be omitted from emit, got:\n{}",
        md
    );
    assert!(
        md.contains("\"hello\""),
        "real field should appear double-quoted, got:\n{}",
        md
    );
}
