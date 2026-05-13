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

/// All `.md` files in `crates/fixtures/resources/quills/**/example.md` and a
/// curated set of the top-level resource markdowns that have a QUILL field.
#[test]
fn fixture_corpus_round_trip() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    // Quill example.md files — enumerate dynamically.
    let quills_dir = std::path::Path::new(manifest_dir)
        .join("..") // crates/core → crates
        .join("fixtures")
        .join("resources")
        .join("quills");

    let mut fixture_paths: Vec<std::path::PathBuf> = Vec::new();

    // Walk the quills directory looking for *.md files (examples).
    if let Ok(entries) = std::fs::read_dir(&quills_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                // Each quill directory may have versioned subdirs.
                collect_md_files(&entry.path(), &mut fixture_paths);
            }
        }
    }

    // Top-level resource .md files that have a QUILL field.
    let resources_dir = quills_dir.parent().unwrap();
    for entry in std::fs::read_dir(resources_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            fixture_paths.push(path);
        }
    }

    // Also add the appreciated_letter subdirectory.
    let appreciated = resources_dir
        .join("appreciated_letter")
        .join("appreciated_letter.md");
    if appreciated.exists() {
        fixture_paths.push(appreciated);
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

        // Skip files that are not parseable Quillmark documents (no QUILL field).
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

/// Recursively collect all `.md` files under `dir`.
fn collect_md_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_md_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
}

// ── Stability smoke test ──────────────────────────────────────────────────────

#[test]
fn emit_twice_is_byte_equal() {
    let src = "\
---
QUILL: test@1.0.0
title: Stability Test
flags:
  - on
  - 'yes'
  - null
count: 42
nested:
  key: value
---

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
    let src = "---\nQUILL: q\nflag_true: true\nflag_false: false\n---\n";
    assert_round_trip("booleans", src);
}

#[test]
fn round_trip_null() {
    let src = "---\nQUILL: q\nnull_field: null\n---\n";
    assert_round_trip("null", src);
}

#[test]
fn round_trip_numbers() {
    let src = "---\nQUILL: q\ncount: 42\nfloat: 3.14\n---\n";
    assert_round_trip("numbers", src);
}

#[test]
fn round_trip_string_ambiguous() {
    // These are the strings most likely to be mis-parsed as booleans/numbers.
    let src = "---\nQUILL: q\nfield_on: \"on\"\nfield_yes: \"yes\"\nfield_01234: \"01234\"\n---\n";
    assert_round_trip("ambiguous strings", src);
}

#[test]
fn round_trip_nested_map() {
    let src = "---\nQUILL: q\nsender:\n  name: Alice\n  city: Springfield\n---\n";
    assert_round_trip("nested map", src);
}

#[test]
fn round_trip_sequence() {
    let src = "---\nQUILL: q\ntags:\n  - demo\n  - test\n---\n";
    assert_round_trip("sequence", src);
}

#[test]
fn round_trip_empty_sequence() {
    let src = "---\nQUILL: q\nempty: []\n---\n";
    assert_round_trip("empty sequence", src);
}

#[test]
fn round_trip_leaves() {
    let src = "\
---
QUILL: q
title: Test
---

Body text.

---
KIND: section
heading: Chapter 1
---

Leaf body here.
";
    assert_round_trip("leaves", src);
}

#[test]
fn round_trip_leaf_empty_body() {
    let src = "\
---
QUILL: q
title: Test
---

---
KIND: empty_body_leaf
title: No body
---
";
    assert_round_trip("leaf with empty body", src);
}

#[test]
fn round_trip_string_with_escapes() {
    // String containing backslash and quotes — must survive as a string.
    let src = "---\nQUILL: q\npath: \"C:\\\\Users\\\\test\"\n---\n";
    assert_round_trip("string with backslash", src);
}

#[test]
fn round_trip_multiline_string() {
    // A string containing a literal newline.
    let src = "---\nQUILL: q\nbio: \"Line one\\nLine two\"\n---\n";
    assert_round_trip("multiline string", src);
}

#[test]
fn round_trip_quill_version_selectors() {
    for qref in &["q", "q@1", "q@1.2", "q@1.2.3", "q@latest"] {
        let src = format!("---\nQUILL: {}\ntitle: t\n---\n", qref);
        assert_round_trip(&format!("quill ref {}", qref), &src);
    }
}

#[test]
fn empty_map_omitted_from_emit() {
    // After parsing a document where a field is an empty object,
    // the emitter should omit that field.
    use crate::value::QuillValue;
    use indexmap::IndexMap;

    let mut frontmatter: IndexMap<String, QuillValue> = IndexMap::new();
    frontmatter.insert(
        "empty_obj".to_string(),
        QuillValue::from_json(serde_json::json!({})),
    );
    frontmatter.insert(
        "real_field".to_string(),
        QuillValue::from_json(serde_json::json!("hello")),
    );

    use crate::document::{Leaf, Frontmatter, Sentinel};
    use crate::version::{QuillReference, VersionSelector};
    let main = Leaf::new_with_sentinel(
        Sentinel::Main(QuillReference::new(
            "test".to_string(),
            VersionSelector::Latest,
        )),
        Frontmatter::from_index_map(frontmatter),
        String::new(),
    );
    let doc = crate::document::Document::from_main_and_leaves(main, vec![], vec![]);

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
