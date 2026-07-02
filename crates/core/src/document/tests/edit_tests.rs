//! Unit tests for the document editor surface.

use crate::document::edit::{is_valid_field_name, EditError};
use crate::document::meta::is_valid_kind_name;
use crate::document::{Card, Document};
use crate::value::QuillValue;
use crate::version::QuillReference;
use std::str::FromStr;

// ── Helper ───────────────────────────────────────────────────────────────────

fn make_doc() -> Document {
    Document::from_markdown(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hello\n~~~\n\nBody text.\n",
    )
    .unwrap()
}

fn make_doc_with_cards() -> Document {
    Document::from_markdown(
        "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hello\n~~~\n\nBody.\n\n~~~card-yaml\n$kind: note\nfoo: bar\n~~~\n\nCard body.\n\n~~~card-yaml\n$kind: summary\n~~~\n",
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
    // Uppercase is accepted (lowercase is canonical but not enforced); case
    // is significant. Uppercase names like `Title`/`BODY` are ordinary fields.
    assert!(is_valid_field_name("Title"));
    assert!(is_valid_field_name("BODY"));
    assert!(is_valid_field_name("MixedCase_1"));
}

#[test]
fn test_invalid_field_names() {
    assert!(!is_valid_field_name(""));
    assert!(!is_valid_field_name("123abc")); // starts with digit
    assert!(!is_valid_field_name("my-field")); // hyphen not allowed
    assert!(!is_valid_field_name("my field")); // space not allowed
    assert!(!is_valid_field_name("$body")); // $-prefix reserved for metadata
}

// ── EditError variants ───────────────────────────────────────────────────────

#[test]
fn test_edit_error_invalid_kind_name() {
    let result = Card::new("Invalid-Kind");
    assert_eq!(
        result,
        Err(EditError::InvalidKindName("Invalid-Kind".to_string()))
    );
}

// ── EditError Display ────────────────────────────────────────────────────────

#[test]
fn test_edit_error_display() {
    assert!(EditError::InvalidFieldName("Bad-Name".to_string())
        .to_string()
        .contains("Bad-Name"));
    assert!(EditError::InvalidKindName("Bad-Kind".to_string())
        .to_string()
        .contains("Bad-Kind"));
    assert!(EditError::IndexOutOfRange { index: 3, len: 2 }
        .to_string()
        .contains("3"));
}

// ── `$`-prefixed names: Document::set_field ──────────────────────────────────

#[test]
fn test_document_set_field_rejects_dollar_prefixed_names() {
    // `$`-prefixed keys are reserved for system metadata — the only
    // field-name reservation (uppercase is accepted).
    for name in ["$body", "$cards", "$quill", "$kind"] {
        let mut doc = make_doc();
        let result = doc.main_mut().set_field(name, qv("value"));
        assert_eq!(
            result,
            Err(EditError::InvalidFieldName(name.to_string())),
            "expected InvalidFieldName for '{}'",
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
        doc.main().payload().get("author").unwrap().as_str(),
        Some("Alice")
    );
}

#[test]
fn test_document_set_field_updates_existing() {
    let mut doc = make_doc();
    doc.main_mut().set_field("title", qv("New Title")).unwrap();
    assert_eq!(
        doc.main().payload().get("title").unwrap().as_str(),
        Some("New Title")
    );
}

// ── Document::remove_field ───────────────────────────────────────────────────

#[test]
fn test_document_remove_field_existing() {
    let mut doc = make_doc();
    let removed = doc.main_mut().remove_field("title").unwrap();
    assert_eq!(removed.unwrap().as_str(), Some("Hello"));
    assert!(doc.main().payload().get("title").is_none());
}

#[test]
fn test_document_remove_field_absent() {
    let mut doc = make_doc();
    let removed = doc.main_mut().remove_field("nonexistent").unwrap();
    assert!(removed.is_none());
}

#[test]
fn test_document_field_legacy_uppercase_accepted() {
    // Uppercase names like `BODY`/`CARDS`/`QUILL`/`CARD` are ordinary valid
    // field names: set them, read them back verbatim, and remove them. Only
    // `$`-prefixed keys are reserved.
    let mut doc = make_doc();
    for name in ["BODY", "CARDS", "QUILL", "CARD"] {
        doc.main_mut()
            .set_field(name, qv("v"))
            .unwrap_or_else(|e| panic!("expected {name} to be accepted, got {e:?}"));
        assert_eq!(
            doc.main().payload().get(name).unwrap().as_str().unwrap(),
            "v"
        );
        let removed = doc.main_mut().remove_field(name).unwrap();
        assert_eq!(removed.unwrap().as_str().unwrap(), "v");
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

// ── Document::push_card ──────────────────────────────────────────────────────

#[test]
fn test_document_push_card() {
    let mut doc = make_doc();
    let card = Card::new("note").unwrap();
    doc.push_card(card).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

// ── Document::insert_card ────────────────────────────────────────────────────

#[test]
fn test_document_insert_card_at_zero() {
    let mut doc = make_doc_with_cards(); // 2 cards: note, summary
    let card = Card::new("intro").unwrap();
    doc.insert_card(0, card).unwrap();
    assert_eq!(doc.cards().len(), 3);
    assert_eq!(doc.cards()[0].kind(), Some("intro"));
    assert_eq!(doc.cards()[1].kind(), Some("note"));
}

#[test]
fn test_document_insert_card_at_end() {
    let mut doc = make_doc_with_cards(); // 2 cards
    let len = doc.cards().len();
    let card = Card::new("footer").unwrap();
    doc.insert_card(len, card).unwrap();
    assert_eq!(doc.cards()[len].kind(), Some("footer"));
}

#[test]
fn test_document_insert_card_out_of_range() {
    let mut doc = make_doc(); // 0 cards
    let card = Card::new("note").unwrap();
    let result = doc.insert_card(1, card);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: 1, len: 0 }));
}

// ── Document::remove_card ────────────────────────────────────────────────────

#[test]
fn test_document_remove_card() {
    let mut doc = make_doc_with_cards(); // 2 cards: note, summary
    let removed = doc.remove_card(0);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().kind(), Some("note"));
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.cards()[0].kind(), Some("summary"));
}

#[test]
fn test_document_remove_card_out_of_range() {
    let mut doc = make_doc();
    let removed = doc.remove_card(0);
    assert!(removed.is_none());
}

// ── Document::card_mut ───────────────────────────────────────────────────────

#[test]
fn test_document_card_mut() {
    let mut doc = make_doc_with_cards();
    {
        let card = doc.card_mut(0).unwrap();
        card.replace_body("Updated card body.");
    }
    assert_eq!(doc.cards()[0].body(), "Updated card body.");
}

#[test]
fn test_document_card_mut_out_of_range() {
    let mut doc = make_doc();
    assert!(doc.card_mut(0).is_none());
}

// ── Document::move_card ──────────────────────────────────────────────────────

#[test]
fn test_move_card_no_op_same_index() {
    let mut doc = make_doc_with_cards(); // note(0), summary(1)
    let result = doc.move_card(0, 0);
    assert_eq!(result, Ok(()));
    assert_eq!(doc.cards()[0].kind(), Some("note"));
    assert_eq!(doc.cards()[1].kind(), Some("summary"));
}

#[test]
fn test_move_card_last_to_first() {
    let mut doc = make_doc_with_cards(); // note(0), summary(1)
    doc.move_card(1, 0).unwrap();
    assert_eq!(doc.cards()[0].kind(), Some("summary"));
    assert_eq!(doc.cards()[1].kind(), Some("note"));
}

#[test]
fn test_move_card_first_to_last() {
    let mut doc = make_doc_with_cards(); // note(0), summary(1)
    let last = doc.cards().len() - 1;
    doc.move_card(0, last).unwrap();
    assert_eq!(doc.cards()[0].kind(), Some("summary"));
    assert_eq!(doc.cards()[last].kind(), Some("note"));
}

#[test]
fn test_move_card_from_out_of_range() {
    let mut doc = make_doc_with_cards(); // 2 cards
    let len = doc.cards().len();
    let result = doc.move_card(len, 0);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: len, len }));
}

#[test]
fn test_move_card_to_out_of_range() {
    let mut doc = make_doc_with_cards(); // 2 cards
    let len = doc.cards().len();
    let result = doc.move_card(0, len);
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: len, len }));
}

// ── Document::set_card_kind ───────────────────────────────────────────────────

#[test]
fn test_set_card_kind_renames_in_place() {
    let mut doc = make_doc_with_cards(); // note(0) with field foo=bar, summary(1)
    doc.set_card_kind(0, "annotation").unwrap();
    // `$kind` changed.
    assert_eq!(doc.cards()[0].kind(), Some("annotation"));
    // Payload and body untouched.
    assert_eq!(
        doc.cards()[0].payload().get("foo").unwrap().as_str(),
        Some("bar")
    );
    // Other cards untouched.
    assert_eq!(doc.cards()[1].kind(), Some("summary"));
}

#[test]
fn test_set_card_kind_rejects_invalid_kind() {
    let mut doc = make_doc_with_cards();
    for bad in ["", "Bad", "with-dash", "1leading_digit"] {
        match doc.set_card_kind(0, bad) {
            Err(EditError::InvalidKindName(t)) => assert_eq!(t, bad),
            other => panic!("expected InvalidKindName for {bad:?}, got {other:?}"),
        }
    }
    // Original kind preserved on failure.
    assert_eq!(doc.cards()[0].kind(), Some("note"));
}

#[test]
fn test_set_card_kind_index_out_of_range() {
    let mut doc = make_doc_with_cards();
    let len = doc.cards().len();
    let result = doc.set_card_kind(len, "annotation");
    assert_eq!(result, Err(EditError::IndexOutOfRange { index: len, len }));
}

#[test]
fn test_set_card_kind_round_trips_via_markdown() {
    // Verify that renaming a card and re-emitting markdown produces a doc
    // that re-parses with the new kind.
    let mut doc = make_doc_with_cards();
    doc.set_card_kind(0, "annotation").unwrap();
    let md = doc.to_markdown();
    let reparsed = crate::Document::from_markdown(&md).unwrap();
    assert_eq!(reparsed.cards()[0].kind(), Some("annotation"));
}

// ── Card::new ────────────────────────────────────────────────────────────────

#[test]
fn test_card_new_invalid_kind_rejected() {
    for kind in ["Note", "", "my-card"] {
        assert_eq!(
            Card::new(kind),
            Err(EditError::InvalidKindName(kind.to_string()))
        );
    }
}

// ── Card::set_field ──────────────────────────────────────────────────────────

#[test]
fn test_card_set_field_valid() {
    let mut card = Card::new("note").unwrap();
    card.set_field("content", qv("Some text")).unwrap();
    assert_eq!(
        card.payload().get("content").unwrap().as_str(),
        Some("Some text")
    );
}

#[test]
fn test_card_set_field_invalid_name() {
    let mut card = Card::new("note").unwrap();
    let result = card.set_field("bad-name", qv("text"));
    assert_eq!(
        result,
        Err(EditError::InvalidFieldName("bad-name".to_string()))
    );
}

// ── Document::new (blank canvas) ─────────────────────────────────────────────

#[test]
fn test_document_new_blank_canvas() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    assert_eq!(doc.quill_reference().to_string(), "test_quill");
    assert!(doc.cards().is_empty());
    assert_eq!(doc.main().body(), "");
    assert!(doc.warnings().is_empty());

    doc.main_mut().set_fields([("title", "Hello")]).unwrap();
    let mut card = Card::new("note").unwrap();
    card.set_field("qty", 3).unwrap();
    doc.push_card(card).unwrap();

    // A built-from-blank document round-trips the canonical emitter.
    let reparsed = Document::from_markdown(&doc.to_markdown()).unwrap();
    assert_eq!(doc, reparsed);
}

// ── Card::set_fields ─────────────────────────────────────────────────────────

#[test]
fn test_card_set_fields_inserts_in_iterator_order() {
    let mut card = Card::new("note").unwrap();
    card.set_fields([
        ("b".to_string(), qv("two")),
        ("a".to_string(), qv("one")),
    ])
    .unwrap();
    let keys: Vec<&String> = card.payload().iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["b", "a"]);
    assert_eq!(card.payload().get("a").unwrap().as_str(), Some("one"));
}

#[test]
fn test_card_set_fields_collects_every_violation() {
    let mut card = Card::new("note").unwrap();
    let errors = card
        .set_fields([
            ("ok".to_string(), qv("fine")),
            ("bad-name".to_string(), qv("v")),
            ("also bad".to_string(), qv("v")),
        ])
        .unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors[0],
        (
            "bad-name".to_string(),
            EditError::InvalidFieldName("bad-name".to_string())
        )
    );
    assert_eq!(
        errors[1],
        (
            "also bad".to_string(),
            EditError::InvalidFieldName("also bad".to_string())
        )
    );
}

#[test]
fn test_card_set_fields_atomic_on_error() {
    let mut card = Card::new("note").unwrap();
    card.set_field("existing", qv("old")).unwrap();
    let result = card.set_fields([
        ("existing".to_string(), qv("new")),
        ("bad-name".to_string(), qv("v")),
    ]);
    assert!(result.is_err());
    // Nothing from the failed batch is applied — not even the valid entries.
    assert_eq!(card.payload().get("existing").unwrap().as_str(), Some("old"));
    assert!(card.payload().get("bad-name").is_none());
}

#[test]
fn test_card_set_fields_clears_fill_and_repeated_name_last_wins() {
    let mut card = Card::new("note").unwrap();
    card.set_fill("title", qv("draft")).unwrap();
    card.set_fields([
        ("title".to_string(), qv("first")),
        ("title".to_string(), qv("final")),
    ])
    .unwrap();
    let value = card.payload().get("title").unwrap();
    assert!(!value.fill());
    assert_eq!(value.as_str(), Some("final"));
}

#[test]
fn test_set_field_scalar_conversions() {
    // The `From` impls let scalars pass straight through `impl Into<QuillValue>`.
    let mut card = Card::new("note").unwrap();
    card.set_field("name", "Alice").unwrap();
    card.set_field("qty", 3).unwrap();
    card.set_field("price", 2.5).unwrap();
    card.set_field("active", true).unwrap();
    card.set_field("tags", serde_json::json!(["a", "b"])).unwrap();
    card.set_fields([("count", 1), ("total", 2)]).unwrap();
    assert_eq!(card.payload().get("name").unwrap().as_str(), Some("Alice"));
    assert_eq!(card.payload().get("qty").unwrap().as_i64(), Some(3));
    assert_eq!(card.payload().get("price").unwrap().as_f64(), Some(2.5));
    assert_eq!(card.payload().get("active").unwrap().as_bool(), Some(true));
    assert_eq!(card.payload().get("total").unwrap().as_i64(), Some(2));
}

// ── Card::remove_field ───────────────────────────────────────────────────────

#[test]
fn test_card_remove_field_existing() {
    let mut doc = make_doc_with_cards();
    // doc.cards()[0] is "note" with field "foo" = "bar"
    let card = doc.card_mut(0).unwrap();
    let removed = card.remove_field("foo").unwrap();
    assert_eq!(removed.unwrap().as_str(), Some("bar"));
    assert!(card.payload().get("foo").is_none());
}

#[test]
fn test_card_remove_field_absent() {
    let mut card = Card::new("note").unwrap();
    assert!(card.remove_field("nonexistent").unwrap().is_none());
}

#[test]
fn test_card_remove_field_invalid_name_throws() {
    let mut card = Card::new("note").unwrap();
    match card.remove_field("Bad-Name") {
        Err(EditError::InvalidFieldName(name)) => assert_eq!(name, "Bad-Name"),
        other => panic!("expected InvalidFieldName, got {other:?}"),
    }
}

// ── Card::set_body ───────────────────────────────────────────────────────────

#[test]
fn test_card_set_body() {
    let mut card = Card::new("note").unwrap();
    card.replace_body("Card body text.");
    assert_eq!(card.body(), "Card body text.");
}

// ── Invariant check: sequence of mutations ───────────────────────────────────

/// After a deterministic sequence of mutations, the document must satisfy:
/// - Every payload key passes is_valid_field_name
/// - Every card kind passes is_valid_kind_name
/// - The plate JSON can be produced without panicking
#[test]
fn test_invariants_after_mutation_sequence() {
    let mut doc = make_doc();

    // 1. Add some payload fields
    doc.main_mut().set_field("author", qv("Alice")).unwrap();
    doc.main_mut().set_field("version", qv_int(3)).unwrap();

    // 2. Add cards
    let c1 = Card::new("note").unwrap();
    let c2 = Card::new("summary").unwrap();
    let c3 = Card::new("appendix").unwrap();
    doc.push_card(c1).unwrap();
    doc.push_card(c2).unwrap();
    doc.insert_card(1, c3).unwrap(); // now: note, appendix, summary

    // 3. Mutate a card field
    doc.card_mut(0)
        .unwrap()
        .set_field("text", qv("Hello"))
        .unwrap();

    // 4. Move cards around
    doc.move_card(2, 0).unwrap(); // summary, note, appendix

    // 5. Remove a card
    doc.remove_card(1); // summary, appendix

    // 6. Replace body
    doc.main_mut().replace_body("Updated body.");

    // 7. Remove a payload field
    doc.main_mut().remove_field("version").unwrap();

    // --- Assertions ---

    // Every payload key is valid
    for key in doc.main().payload().keys() {
        assert!(
            is_valid_field_name(key),
            "invalid key '{}' found in payload",
            key
        );
    }

    // Every card kind is valid
    for card in doc.cards() {
        if let Some(kind) = card.kind() {
            assert!(is_valid_kind_name(kind), "invalid kind '{}' found", kind);
        }
    }

    // Can produce plate JSON without panicking
    let json = doc.to_plate_json();
    assert!(json.is_object());
    assert_eq!(json["$quill"].as_str(), Some("test_quill"));
    assert!(json["$cards"].is_array());
    assert_eq!(json["$body"].as_str(), Some("Updated body."));

    // Payload still has expected keys
    assert_eq!(
        doc.main().payload().get("author").unwrap().as_str(),
        Some("Alice")
    );
    assert!(doc.main().payload().get("version").is_none());
}

// ── $ext mutators ──────────────────────────────────────────────────────────────

#[test]
fn test_set_ext_adds_map_and_strips_from_plate() {
    let mut doc = make_doc();
    let mut ext = serde_json::Map::new();
    ext.insert(
        "presentation".to_string(),
        serde_json::json!({ "title": "Greeting" }),
    );
    doc.main_mut().set_ext(ext);

    // Surfaced through the typed accessor.
    assert_eq!(
        doc.main().ext().unwrap()["presentation"]["title"].as_str(),
        Some("Greeting")
    );

    // Never reaches the plate JSON backends consume.
    let json = doc.to_plate_json();
    assert!(json.get("$ext").is_none());
    assert!(!json.as_object().unwrap().contains_key("$ext"));
}

#[test]
fn test_set_ext_round_trips_through_markdown() {
    let mut doc = make_doc();
    let mut ext = serde_json::Map::new();
    ext.insert("agent".to_string(), serde_json::json!({ "pinned": true }));
    doc.main_mut().set_ext(ext);

    let md = doc.to_markdown();
    let reparsed = Document::from_markdown(&md).unwrap();
    assert_eq!(
        reparsed.main().ext().unwrap()["agent"]["pinned"].as_bool(),
        Some(true)
    );
}

#[test]
fn test_remove_ext_returns_previous_and_clears() {
    let mut doc = make_doc();
    let mut ext = serde_json::Map::new();
    ext.insert("agent".to_string(), serde_json::json!(1));
    doc.main_mut().set_ext(ext);

    let removed = doc.main_mut().remove_ext().unwrap();
    assert_eq!(removed["agent"].as_i64(), Some(1));
    assert!(doc.main().ext().is_none());
    // Removing again is a no-op.
    assert!(doc.main_mut().remove_ext().is_none());
}

#[test]
fn test_set_ext_namespace_preserves_siblings() {
    let mut doc = make_doc();
    doc.main_mut()
        .set_ext_namespace("presentation", serde_json::json!({ "title": "A" }));
    doc.main_mut()
        .set_ext_namespace("agent", serde_json::json!({ "pinned": true }));

    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("A"));
    assert_eq!(ext["agent"]["pinned"].as_bool(), Some(true));

    // Replacing one namespace leaves the other intact.
    doc.main_mut()
        .set_ext_namespace("presentation", serde_json::json!({ "title": "B" }));
    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("B"));
    assert_eq!(ext["agent"]["pinned"].as_bool(), Some(true));
}

#[test]
fn test_remove_ext_namespace_preserves_siblings() {
    let mut doc = make_doc();
    doc.main_mut()
        .set_ext_namespace("presentation", serde_json::json!({ "title": "A" }));
    doc.main_mut()
        .set_ext_namespace("tutorial", serde_json::json!(["step-1", "step-2"]));

    // Dropping one namespace returns its value and leaves the rest intact.
    let removed = doc.main_mut().remove_ext_namespace("tutorial").unwrap();
    assert_eq!(removed, serde_json::json!(["step-1", "step-2"]));
    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("A"));
    assert!(!ext.contains_key("tutorial"));
}

#[test]
fn test_remove_ext_namespace_drops_ext_when_empty() {
    let mut doc = make_doc();
    doc.main_mut()
        .set_ext_namespace("tutorial", serde_json::json!(["step-1"]));

    // Removing the last namespace clears `$ext` entirely — set/remove of a
    // single namespace is a clean inverse for a card that had no `$ext`.
    let removed = doc.main_mut().remove_ext_namespace("tutorial").unwrap();
    assert_eq!(removed, serde_json::json!(["step-1"]));
    assert!(doc.main().ext().is_none());
}

#[test]
fn test_remove_ext_namespace_is_noop_when_absent() {
    let mut doc = make_doc();
    // No `$ext` at all.
    assert!(doc.main_mut().remove_ext_namespace("tutorial").is_none());

    // `$ext` present but without the requested key.
    doc.main_mut()
        .set_ext_namespace("presentation", serde_json::json!({ "title": "A" }));
    assert!(doc.main_mut().remove_ext_namespace("tutorial").is_none());
    // The unrelated namespace is untouched.
    assert_eq!(
        doc.main().ext().unwrap()["presentation"]["title"].as_str(),
        Some("A")
    );
}

#[test]
fn test_set_empty_ext_is_preserved() {
    let mut doc = make_doc();
    doc.main_mut().set_ext(serde_json::Map::new());
    assert!(doc.main().ext().is_some());
    let md = doc.to_markdown();
    assert!(md.contains("$ext: {}"), "got: {md}");
}

#[test]
fn test_ext_mutators_work_on_composable_cards() {
    let mut doc = make_doc_with_cards();
    doc.card_mut(0)
        .unwrap()
        .set_ext_namespace("agent", serde_json::json!({ "note": "x" }));
    assert_eq!(
        doc.cards()[0].ext().unwrap()["agent"]["note"].as_str(),
        Some("x")
    );

    // Namespace removal targets the same card and clears `$ext` once empty.
    let removed = doc.card_mut(0).unwrap().remove_ext_namespace("agent");
    assert_eq!(removed, Some(serde_json::json!({ "note": "x" })));
    assert!(doc.cards()[0].ext().is_none());
}

// ── §8 value-depth bound at every ingestion boundary (CORE-2) ────────────────

/// Build `{"a":{"a":…}}` nested `depth` levels (iteratively, so the test
/// itself stays stack-safe).
fn deep_value(depth: usize) -> serde_json::Value {
    let mut v = serde_json::json!(1);
    for _ in 0..depth {
        let mut m = serde_json::Map::new();
        m.insert("a".to_string(), v);
        v = serde_json::Value::Object(m);
    }
    v
}

#[test]
fn set_field_rejects_value_past_depth_limit() {
    let mut doc =
        crate::document::Document::from_markdown("~~~\n$quill: q@1.0\n$kind: main\n~~~\n").unwrap();
    let ok = crate::value::QuillValue::from_json(deep_value(50));
    assert!(doc.main_mut().set_field("x", ok).is_ok());

    let too_deep = crate::value::QuillValue::from_json(deep_value(150));
    let err = doc.main_mut().set_field("y", too_deep).unwrap_err();
    assert!(
        matches!(err, crate::document::EditError::ValueTooDeep { max: 100 }),
        "expected ValueTooDeep, got {err:?}"
    );
    // set_fill and set_ext carry the same bound.
    let too_deep = crate::value::QuillValue::from_json(deep_value(150));
    assert!(doc.main_mut().set_fill("y", too_deep).is_err());
    let serde_json::Value::Object(map) = deep_value(150) else {
        unreachable!()
    };
    assert!(doc.main_mut().set_ext(map).is_err());
    assert!(doc
        .main_mut()
        .set_ext_namespace("ns", deep_value(150))
        .is_err());
}

#[test]
fn storage_dto_rejects_value_past_depth_limit() {
    // A hand-crafted storage DTO with an over-deep field value must be
    // rejected with a clean error, not abort the process — the §8 bound
    // holds on the storage path, not just the markdown parse path.
    let stored = serde_json::json!({
        "schema": "quillmark/document@0.82.0",
        "main": {
            "payload": {"items": [
                {"type": "quill", "value": "q@1.0"},
                {"type": "kind", "value": "main"},
                {"type": "field", "key": "x", "value": deep_value(150)}
            ]},
            "body": ""
        },
        "cards": []
    });
    let err = serde_json::from_value::<crate::document::Document>(stored).unwrap_err();
    assert!(
        err.to_string().contains("deeper than the maximum"),
        "expected depth error, got {err}"
    );

    // $ext carries the same bound.
    let serde_json::Value::Object(deep_map) = deep_value(150) else {
        unreachable!()
    };
    let stored = serde_json::json!({
        "schema": "quillmark/document@0.82.0",
        "main": {
            "payload": {"items": [
                {"type": "quill", "value": "q@1.0"},
                {"type": "kind", "value": "main"},
                {"type": "ext", "value": deep_map}
            ]},
            "body": ""
        },
        "cards": []
    });
    let err = serde_json::from_value::<crate::document::Document>(stored).unwrap_err();
    assert!(
        err.to_string().contains("deeper than the maximum"),
        "expected $ext depth error, got {err}"
    );
}

#[test]
fn wire_card_rejects_value_past_depth_limit_and_bad_names() {
    let wire: crate::document::CardWire = serde_json::from_value(serde_json::json!({
        "kind": "note",
        "payloadItems": [
            {"type": "field", "key": "x", "value": deep_value(150)}
        ],
        "body": ""
    }))
    .unwrap();
    let err = crate::document::Card::try_from(wire).unwrap_err();
    assert!(
        err.to_string().contains("deeper than the maximum"),
        "expected depth error, got {err}"
    );

    let wire: crate::document::CardWire = serde_json::from_value(serde_json::json!({
        "kind": "note",
        "payloadItems": [
            {"type": "field", "key": "Bad Name", "value": 1}
        ],
        "body": ""
    }))
    .unwrap();
    let err = crate::document::Card::try_from(wire).unwrap_err();
    assert!(
        err.to_string().contains("[A-Za-z_]"),
        "expected name error, got {err}"
    );
}
