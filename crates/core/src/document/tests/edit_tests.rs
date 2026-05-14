//! Unit tests for the document editor surface.

use crate::document::edit::{is_reserved_name, is_valid_field_name, EditError, RESERVED_NAMES};
use crate::document::sentinel::is_valid_tag_name;
use crate::document::{Document, Leaf};
use crate::value::QuillValue;
use crate::version::QuillReference;
use std::str::FromStr;

// ── Helper ───────────────────────────────────────────────────────────────────

fn make_doc() -> Document {
    Document::from_markdown("---\nQUILL: test_quill\ntitle: Hello\n---\n\nBody text.\n").unwrap()
}

fn make_doc_with_leaves() -> Document {
    Document::from_markdown(
        "---\nQUILL: test_quill\ntitle: Hello\n---\n\nBody.\n\n```leaf\nKIND: note\nfoo: bar\n```\n\nLeaf body.\n\n```leaf\nKIND: summary\n```\n",
    )
    .unwrap()
}

fn qv(s: &str) -> QuillValue {
    QuillValue::from_json(serde_json::json!(s))
}

fn qv_int(n: i64) -> QuillValue {
    QuillValue::from_json(serde_json::json!(n))
}

// ── is_valid_field_name ──────────────────────────────────────────────────────

#[test]
fn test_valid_field_names() {
    assert!(is_valid_field_name("title"));
    assert!(is_valid_field_name("my_field"));
    assert!(is_valid_field_name("_private"));
    assert!(is_valid_field_name("abc123"));
    assert!(is_valid_field_name("a1b2c3"));
    assert!(is_valid_field_name("x"));
    assert!(is_valid_field_name("_"));
}

#[test]
fn test_invalid_field_names() {
    assert!(!is_valid_field_name(""));
    assert!(!is_valid_field_name("Title")); // uppercase
    assert!(!is_valid_field_name("123abc")); // starts with digit
    assert!(!is_valid_field_name("my-field")); // hyphen not allowed
    assert!(!is_valid_field_name("my field")); // space not allowed
    assert!(!is_valid_field_name("BODY")); // uppercase (reserved)
    assert!(!is_valid_field_name("LEAVES")); // uppercase (reserved)
}

// ── is_reserved_name ────────────────────────────────────────────────────────

#[test]
fn test_reserved_names_constant() {
    // All four names are present
    assert!(RESERVED_NAMES.contains(&"BODY"));
    assert!(RESERVED_NAMES.contains(&"LEAVES"));
    assert!(RESERVED_NAMES.contains(&"QUILL"));
    assert!(RESERVED_NAMES.contains(&"KIND"));
    assert_eq!(RESERVED_NAMES.len(), 4);
}

#[test]
fn test_is_reserved_name() {
    assert!(is_reserved_name("BODY"));
    assert!(is_reserved_name("LEAVES"));
    assert!(is_reserved_name("QUILL"));
    assert!(is_reserved_name("KIND"));
    assert!(!is_reserved_name("body")); // case-sensitive
    assert!(!is_reserved_name("title"));
    assert!(!is_reserved_name(""));
}

// ── EditError variants ───────────────────────────────────────────────────────

#[test]
fn test_edit_error_reserved_name() {
    let mut doc = make_doc();
    let result = doc.main_mut().set_field("BODY", qv("value"));
    assert_eq!(result, Err(EditError::ReservedName("BODY".to_string())));
}

#[test]
fn test_edit_error_invalid_field_name() {
    let mut doc = make_doc();
    let result = doc.main_mut().set_field("My-Field", qv("value"));
    assert_eq!(
        result,
        Err(EditError::InvalidFieldName("My-Field".to_string()))
    );
}

#[test]
fn test_edit_error_invalid_tag_name() {
    let result = Leaf::new("Invalid-Tag");
    assert_eq!(
        result,
        Err(EditError::InvalidTagName("Invalid-Tag".to_string()))
    );
}

#[test]
fn test_edit_error_index_out_of_range() {
    let mut doc = make_doc(); // no leaves
    let leaf = Leaf::new("note").unwrap();
    let result = doc.insert_leaf(5, leaf);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: 5, len: 0 }));
}

// ── EditError Display ────────────────────────────────────────────────────────

#[test]
fn test_edit_error_display() {
    assert!(EditError::ReservedName("BODY".to_string())
        .to_string()
        .contains("BODY"));
    assert!(EditError::InvalidFieldName("Bad-Name".to_string())
        .to_string()
        .contains("Bad-Name"));
    assert!(EditError::InvalidTagName("Bad-Tag".to_string())
        .to_string()
        .contains("Bad-Tag"));
    assert!(EditError::IndexOutOfRange { index: 3, len: 2 }
        .to_string()
        .contains("3"));
}

// ── Reserved-name matrix: Document::set_field ────────────────────────────────

#[test]
fn test_document_set_field_rejects_all_reserved_names() {
    for &name in RESERVED_NAMES {
        let mut doc = make_doc();
        let result = doc.main_mut().set_field(name, qv("value"));
        assert_eq!(
            result,
            Err(EditError::ReservedName(name.to_string())),
            "expected ReservedName for '{}'",
            name
        );
    }
}

// ── Reserved-name matrix: Leaf::set_field ────────────────────────────────────

#[test]
fn test_leaf_set_field_rejects_all_reserved_names() {
    for &name in RESERVED_NAMES {
        let mut leaf = Leaf::new("note").unwrap();
        let result = leaf.set_field(name, qv("value"));
        assert_eq!(
            result,
            Err(EditError::ReservedName(name.to_string())),
            "expected ReservedName for '{}'",
            name
        );
    }
}

// ── Document::set_field (happy path) ─────────────────────────────────────────

#[test]
fn test_document_set_field_inserts() {
    let mut doc = make_doc();
    doc.main_mut().set_field("author", qv("Alice")).unwrap();
    assert_eq!(
        doc.main().frontmatter().get("author").unwrap().as_str(),
        Some("Alice")
    );
}

#[test]
fn test_document_set_field_updates_existing() {
    let mut doc = make_doc();
    doc.main_mut().set_field("title", qv("New Title")).unwrap();
    assert_eq!(
        doc.main().frontmatter().get("title").unwrap().as_str(),
        Some("New Title")
    );
}

// ── Document::remove_field ───────────────────────────────────────────────────

#[test]
fn test_document_remove_field_existing() {
    let mut doc = make_doc();
    let removed = doc.main_mut().remove_field("title").unwrap();
    assert_eq!(removed.unwrap().as_str(), Some("Hello"));
    assert!(doc.main().frontmatter().get("title").is_none());
}

#[test]
fn test_document_remove_field_absent() {
    let mut doc = make_doc();
    let removed = doc.main_mut().remove_field("nonexistent").unwrap();
    assert!(removed.is_none());
}

#[test]
fn test_document_remove_field_reserved_throws() {
    // Symmetric with set_field: reserved names are programmer errors and
    // throw, rather than silently returning None.
    let mut doc = make_doc();
    for reserved in ["BODY", "LEAVES", "QUILL", "KIND"] {
        match doc.main_mut().remove_field(reserved) {
            Err(EditError::ReservedName(name)) => assert_eq!(name, reserved),
            other => panic!("expected ReservedName for {reserved}, got {other:?}"),
        }
    }
}

#[test]
fn test_document_remove_field_invalid_name_throws() {
    let mut doc = make_doc();
    match doc.main_mut().remove_field("Bad-Name") {
        Err(EditError::InvalidFieldName(name)) => assert_eq!(name, "Bad-Name"),
        other => panic!("expected InvalidFieldName, got {other:?}"),
    }
}

// ── Document::set_quill_ref ──────────────────────────────────────────────────

#[test]
fn test_document_set_quill_ref() {
    let mut doc = make_doc();
    let new_ref = QuillReference::from_str("new_quill").unwrap();
    doc.set_quill_ref(new_ref);
    assert_eq!(doc.quill_reference().name, "new_quill");
}

// ── Document::replace_body ───────────────────────────────────────────────────

#[test]
fn test_document_replace_body() {
    let mut doc = make_doc();
    doc.main_mut().replace_body("New body content.");
    assert_eq!(doc.main().body(), "New body content.");
}

// ── Document::push_leaf ──────────────────────────────────────────────────────

#[test]
fn test_document_push_leaf() {
    let mut doc = make_doc();
    let leaf = Leaf::new("note").unwrap();
    doc.push_leaf(leaf);
    assert_eq!(doc.leaves().len(), 1);
    assert_eq!(doc.leaves()[0].tag(), "note");
}

// ── Document::insert_leaf ────────────────────────────────────────────────────

#[test]
fn test_document_insert_leaf_at_zero() {
    let mut doc = make_doc_with_leaves(); // 2 leaves: note, summary
    let leaf = Leaf::new("intro").unwrap();
    doc.insert_leaf(0, leaf).unwrap();
    assert_eq!(doc.leaves().len(), 3);
    assert_eq!(doc.leaves()[0].tag(), "intro");
    assert_eq!(doc.leaves()[1].tag(), "note");
}

#[test]
fn test_document_insert_leaf_at_end() {
    let mut doc = make_doc_with_leaves(); // 2 leaves
    let len = doc.leaves().len();
    let leaf = Leaf::new("footer").unwrap();
    doc.insert_leaf(len, leaf).unwrap();
    assert_eq!(doc.leaves()[len].tag(), "footer");
}

#[test]
fn test_document_insert_leaf_out_of_range() {
    let mut doc = make_doc(); // 0 leaves
    let leaf = Leaf::new("note").unwrap();
    let result = doc.insert_leaf(1, leaf);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: 1, len: 0 }));
}

// ── Document::remove_leaf ────────────────────────────────────────────────────

#[test]
fn test_document_remove_leaf() {
    let mut doc = make_doc_with_leaves(); // 2 leaves: note, summary
    let removed = doc.remove_leaf(0);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().tag(), "note");
    assert_eq!(doc.leaves().len(), 1);
    assert_eq!(doc.leaves()[0].tag(), "summary");
}

#[test]
fn test_document_remove_leaf_out_of_range() {
    let mut doc = make_doc();
    let removed = doc.remove_leaf(0);
    assert!(removed.is_none());
}

// ── Document::leaf_mut ───────────────────────────────────────────────────────

#[test]
fn test_document_leaf_mut() {
    let mut doc = make_doc_with_leaves();
    {
        let leaf = doc.leaf_mut(0).unwrap();
        leaf.replace_body("Updated leaf body.");
    }
    assert_eq!(doc.leaves()[0].body(), "Updated leaf body.");
}

#[test]
fn test_document_leaf_mut_out_of_range() {
    let mut doc = make_doc();
    assert!(doc.leaf_mut(0).is_none());
}

// ── Document::move_leaf ──────────────────────────────────────────────────────

#[test]
fn test_move_leaf_no_op_same_index() {
    let mut doc = make_doc_with_leaves(); // note(0), summary(1)
    let result = doc.move_leaf(0, 0);
    assert_eq!(result, Ok(()));
    assert_eq!(doc.leaves()[0].tag(), "note");
    assert_eq!(doc.leaves()[1].tag(), "summary");
}

#[test]
fn test_move_leaf_last_to_first() {
    let mut doc = make_doc_with_leaves(); // note(0), summary(1)
    doc.move_leaf(1, 0).unwrap();
    assert_eq!(doc.leaves()[0].tag(), "summary");
    assert_eq!(doc.leaves()[1].tag(), "note");
}

#[test]
fn test_move_leaf_first_to_last() {
    let mut doc = make_doc_with_leaves(); // note(0), summary(1)
    let last = doc.leaves().len() - 1;
    doc.move_leaf(0, last).unwrap();
    assert_eq!(doc.leaves()[0].tag(), "summary");
    assert_eq!(doc.leaves()[last].tag(), "note");
}

#[test]
fn test_move_leaf_from_out_of_range() {
    let mut doc = make_doc_with_leaves(); // 2 leaves
    let len = doc.leaves().len();
    let result = doc.move_leaf(len, 0);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: len, len }));
}

#[test]
fn test_move_leaf_to_out_of_range() {
    let mut doc = make_doc_with_leaves(); // 2 leaves
    let len = doc.leaves().len();
    let result = doc.move_leaf(0, len);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: len, len }));
}

// ── Document::set_leaf_tag ───────────────────────────────────────────────────

#[test]
fn test_set_leaf_tag_renames_in_place() {
    let mut doc = make_doc_with_leaves(); // note(0) with field foo=bar, summary(1)
    doc.set_leaf_tag(0, "annotation").unwrap();
    // Sentinel changed.
    assert_eq!(doc.leaves()[0].tag(), "annotation");
    // Frontmatter and body untouched.
    assert_eq!(
        doc.leaves()[0].frontmatter().get("foo").unwrap().as_str(),
        Some("bar")
    );
    // Other leaves untouched.
    assert_eq!(doc.leaves()[1].tag(), "summary");
}

#[test]
fn test_set_leaf_tag_rejects_invalid_tag() {
    let mut doc = make_doc_with_leaves();
    for bad in ["", "Bad", "with-dash", "1leading_digit"] {
        match doc.set_leaf_tag(0, bad) {
            Err(EditError::InvalidTagName(t)) => assert_eq!(t, bad),
            other => panic!("expected InvalidTagName for {bad:?}, got {other:?}"),
        }
    }
    // Original tag preserved on failure.
    assert_eq!(doc.leaves()[0].tag(), "note");
}

#[test]
fn test_set_leaf_tag_index_out_of_range() {
    let mut doc = make_doc_with_leaves();
    let len = doc.leaves().len();
    let result = doc.set_leaf_tag(len, "annotation");
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: len, len }));
}

#[test]
fn test_set_leaf_tag_round_trips_via_markdown() {
    // Verify that renaming a leaf and re-emitting markdown produces a doc
    // that re-parses with the new tag.
    let mut doc = make_doc_with_leaves();
    doc.set_leaf_tag(0, "annotation").unwrap();
    let md = doc.to_markdown();
    let reparsed = crate::Document::from_markdown(&md).unwrap();
    assert_eq!(reparsed.leaves()[0].tag(), "annotation");
}

// ── Leaf::new ────────────────────────────────────────────────────────────────

#[test]
fn test_leaf_new_valid() {
    let leaf = Leaf::new("note").unwrap();
    assert_eq!(leaf.tag(), "note");
    assert!(leaf.frontmatter().is_empty());
    assert_eq!(leaf.body(), "");
}

#[test]
fn test_leaf_new_invalid_tag_rejected() {
    for tag in ["Note", "", "my-leaf"] {
        assert_eq!(
            Leaf::new(tag),
            Err(EditError::InvalidTagName(tag.to_string()))
        );
    }
}

// ── Leaf::set_field ──────────────────────────────────────────────────────────

#[test]
fn test_leaf_set_field_valid() {
    let mut leaf = Leaf::new("note").unwrap();
    leaf.set_field("content", qv("Some text")).unwrap();
    assert_eq!(
        leaf.frontmatter().get("content").unwrap().as_str(),
        Some("Some text")
    );
}

#[test]
fn test_leaf_set_field_invalid_name() {
    let mut leaf = Leaf::new("note").unwrap();
    let result = leaf.set_field("Content", qv("text"));
    assert_eq!(
        result,
        Err(EditError::InvalidFieldName("Content".to_string()))
    );
}

// ── Leaf::remove_field ───────────────────────────────────────────────────────

#[test]
fn test_leaf_remove_field_existing() {
    let mut doc = make_doc_with_leaves();
    // doc.leaves()[0] is "note" with field "foo" = "bar"
    let leaf = doc.leaf_mut(0).unwrap();
    let removed = leaf.remove_field("foo").unwrap();
    assert_eq!(removed.unwrap().as_str(), Some("bar"));
    assert!(leaf.frontmatter().get("foo").is_none());
}

#[test]
fn test_leaf_remove_field_absent() {
    let mut leaf = Leaf::new("note").unwrap();
    assert!(leaf.remove_field("nonexistent").unwrap().is_none());
}

#[test]
fn test_leaf_remove_field_reserved_throws() {
    let mut leaf = Leaf::new("note").unwrap();
    for reserved in ["BODY", "LEAVES", "QUILL", "KIND"] {
        match leaf.remove_field(reserved) {
            Err(EditError::ReservedName(name)) => assert_eq!(name, reserved),
            other => panic!("expected ReservedName for {reserved}, got {other:?}"),
        }
    }
}

#[test]
fn test_leaf_remove_field_invalid_name_throws() {
    let mut leaf = Leaf::new("note").unwrap();
    match leaf.remove_field("Bad-Name") {
        Err(EditError::InvalidFieldName(name)) => assert_eq!(name, "Bad-Name"),
        other => panic!("expected InvalidFieldName, got {other:?}"),
    }
}

// ── Leaf::set_body ───────────────────────────────────────────────────────────

#[test]
fn test_leaf_set_body() {
    let mut leaf = Leaf::new("note").unwrap();
    leaf.replace_body("Leaf body text.");
    assert_eq!(leaf.body(), "Leaf body text.");
}

// ── Invariant check: sequence of mutations ───────────────────────────────────

/// After a deterministic sequence of mutations, the document must satisfy:
/// - No reserved key in frontmatter
/// - Every leaf tag passes is_valid_tag_name
/// - The plate JSON can be produced without panicking
#[test]
fn test_invariants_after_mutation_sequence() {
    let mut doc = make_doc();

    // 1. Add some frontmatter fields
    doc.main_mut().set_field("author", qv("Alice")).unwrap();
    doc.main_mut().set_field("version", qv_int(3)).unwrap();

    // 2. Add leaves
    let c1 = Leaf::new("note").unwrap();
    let c2 = Leaf::new("summary").unwrap();
    let c3 = Leaf::new("appendix").unwrap();
    doc.push_leaf(c1);
    doc.push_leaf(c2);
    doc.insert_leaf(1, c3).unwrap(); // now: note, appendix, summary

    // 3. Mutate a leaf field
    doc.leaf_mut(0)
        .unwrap()
        .set_field("text", qv("Hello"))
        .unwrap();

    // 4. Move leaves around
    doc.move_leaf(2, 0).unwrap(); // summary, note, appendix

    // 5. Remove a leaf
    doc.remove_leaf(1); // summary, appendix

    // 6. Replace body
    doc.main_mut().replace_body("Updated body.");

    // 7. Remove a frontmatter field
    doc.main_mut().remove_field("version").unwrap();

    // --- Assertions ---

    // No reserved key in frontmatter
    for key in doc.main().frontmatter().keys() {
        assert!(
            !is_reserved_name(key),
            "reserved key '{}' found in frontmatter",
            key
        );
    }

    // Every leaf tag is valid
    for leaf in doc.leaves() {
        let tag = leaf.tag();
        assert!(is_valid_tag_name(&tag), "invalid tag '{}' found", tag);
    }

    // Can produce plate JSON without panicking
    let json = doc.to_plate_json();
    assert!(json.is_object());
    assert_eq!(json["QUILL"].as_str(), Some("test_quill"));
    assert!(json["LEAVES"].is_array());
    assert_eq!(json["BODY"].as_str(), Some("Updated body."));

    // Frontmatter still has expected keys
    assert_eq!(
        doc.main().frontmatter().get("author").unwrap().as_str(),
        Some("Alice")
    );
    assert!(doc.main().frontmatter().get("version").is_none());
}

// ── Warnings never touched ───────────────────────────────────────────────────

#[test]
fn test_mutators_do_not_touch_warnings() {
    let doc = make_doc();
    let initial_warnings = doc.warnings().to_vec();

    let mut doc = doc;
    doc.main_mut().set_field("extra", qv("value")).unwrap();
    doc.main_mut().replace_body("New body.");
    let leaf = Leaf::new("new_leaf").unwrap();
    doc.push_leaf(leaf);

    assert_eq!(doc.warnings(), initial_warnings.as_slice());
}
