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

/// Commit `value` to a richtext field named `name` via `commit_field` and a
/// synthesized richtext `FieldSchema` — the typed richtext write.
fn commit_richtext(
    card: &mut Card,
    name: &str,
    value: &serde_json::Value,
    inline: bool,
) -> Result<(), EditError> {
    use crate::quill::{FieldSchema, FieldType};
    let schema = FieldSchema::new(name.to_string(), FieldType::RichText { inline }, None);
    card.commit_field(name, QuillValue::from_json(value.clone()), &schema)
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
    doc.main_mut().revise_body("New body content.").unwrap();
    assert_eq!(doc.main().body_markdown(), "New body content.\n");
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
        card.revise_body("Updated card body.").unwrap();
    }
    assert_eq!(doc.cards()[0].body_markdown(), "Updated card body.\n");
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
    assert_eq!(doc.main().body_markdown(), "");

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
    card.set_fields([("b".to_string(), qv("two")), ("a".to_string(), qv("one"))])
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
    assert_eq!(
        card.payload().get("existing").unwrap().as_str(),
        Some("old")
    );
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
    card.set_field("tags", serde_json::json!(["a", "b"]))
        .unwrap();
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

// ── Card::replace_body ───────────────────────────────────────────────────────

#[test]
fn test_card_set_body() {
    let mut card = Card::new("note").unwrap();
    card.revise_body("Card body text.").unwrap();
    assert_eq!(card.body_markdown(), "Card body text.\n");
}

// ── Card richtext body writers ───────────────────────────────────────────────

/// A body markdown import past the container-nesting limit now returns
/// `EditError::BodyImport` instead of silently degrading to the empty corpus.
#[test]
fn test_replace_body_reports_import_error() {
    let mut card = Card::new("note").unwrap();
    let deep = ">".repeat(crate::error::MAX_NESTING_DEPTH + 5);
    match card.revise_body(&deep) {
        Err(EditError::BodyImport(_)) => {}
        other => panic!("expected BodyImport, got {other:?}"),
    }
}

/// `install_body` installs a pre-built corpus verbatim — value semantics, no
/// markdown import, lossless for corpus-only marks a markdown projection drops.
#[test]
fn test_install_body_sets_directly() {
    use quillmark_richtext::model::{Mark, MarkKind};

    let mut corpus = quillmark_richtext::import::from_markdown("underlined body").unwrap();
    // An `underline` mark has no markdown projection — the corpus path must keep it.
    corpus.marks.push(Mark {
        start: 0,
        end: 10,
        kind: MarkKind::Underline,
    });
    corpus.normalize();

    let mut card = Card::new("note").unwrap();
    card.install_body(corpus.clone());
    assert_eq!(card.body(), &corpus);
    assert!(card
        .body()
        .marks
        .iter()
        .any(|m| matches!(m.kind, MarkKind::Underline)));
}

/// `install_field` installs a pre-built corpus into a richtext field verbatim —
/// the field-level twin of `install_body`, lossless for corpus-only marks, and
/// reads back through `field_richtext`. A malformed name is rejected.
#[test]
fn test_install_field_sets_directly() {
    use quillmark_richtext::model::{Mark, MarkKind};

    let mut corpus = quillmark_richtext::import::from_markdown("underlined intro").unwrap();
    corpus.marks.push(Mark {
        start: 0,
        end: 10,
        kind: MarkKind::Underline,
    });
    corpus.normalize();

    let mut card = Card::new("note").unwrap();
    card.install_field("intro", corpus.clone()).unwrap();
    let read = card.field_richtext("intro").unwrap().unwrap();
    assert_eq!(read, corpus);
    assert!(read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)));

    assert_eq!(
        card.install_field("$bad", quillmark_richtext::RichText::empty())
            .unwrap_err()
            .variant_name(),
        "InvalidFieldName"
    );
}

/// `revise_field` imports markdown into a richtext field with edit semantics,
/// rebasing surviving anchors and returning the text delta. An absent field
/// cold-imports from empty; a present non-corpus value is a decode error.
#[test]
fn test_revise_field_diff_imports_and_returns_delta() {
    use quillmark_richtext::model::{Mark, MarkKind};

    let mut card = Card::new("note").unwrap();
    // Cold start against an absent field.
    let delta = card.revise_field("intro", "hello target world").unwrap();
    assert!(!delta.ops.is_empty());

    // Anchor the field, then revise — the anchor rebases onto surviving text.
    let mut base = card.field_richtext("intro").unwrap().unwrap();
    base.marks.push(Mark {
        start: 6,
        end: 12, // "target"
        kind: MarkKind::Anchor { id: "c1".into() },
    });
    base.normalize();
    card.install_field("intro", base).unwrap();
    card.revise_field("intro", "why keep the target here").unwrap();
    let read = card.field_richtext("intro").unwrap().unwrap();
    assert!(read
        .marks
        .iter()
        .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")));

    // A present non-corpus scalar is a decode error.
    card.set_field("count", crate::QuillValue::from_json(serde_json::json!(3)))
        .unwrap();
    assert_eq!(
        card.revise_field("count", "x").unwrap_err().variant_name(),
        "FieldRichtextDecode"
    );
}

/// `commit_field` on a richtext field accepts a **canonical corpus object**,
/// stores it as the field's canonical value (lossless for corpus-only marks),
/// and reads back through `field_richtext` — the typed door beside the
/// value-semantics `install_field` / `body`.
#[test]
fn test_commit_field_richtext_corpus_object_reads_back() {
    use quillmark_richtext::model::{Mark, MarkKind};

    let mut corpus = quillmark_richtext::import::from_markdown("underlined intro").unwrap();
    // `underline` has no markdown projection — the corpus store must keep it.
    corpus.marks.push(Mark {
        start: 0,
        end: 10,
        kind: MarkKind::Underline,
    });
    corpus.normalize();
    let json = quillmark_richtext::serial::to_canonical_value(&corpus);

    let mut card = Card::new("note").unwrap();
    commit_richtext(&mut card, "intro", &json, false).unwrap();

    // Stored structurally as the corpus object, not the authored string.
    assert!(card.payload().get("intro").unwrap().as_json().is_object());
    // Reads back losslessly, underline intact.
    let read = card.field_richtext("intro").unwrap().unwrap();
    assert_eq!(read, corpus);
    assert!(read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)));
}

/// A richtext `commit_field` accepts a **markdown string** (imported) and passes
/// `null` through (reading back as the empty corpus); `field_markdown` is the
/// projection twin of `body_markdown`. A non-corpus object / other shape is
/// `EditError::FieldRichtextDecode`.
#[test]
fn test_commit_field_richtext_markdown_null_and_rejects_bad() {
    let mut card = Card::new("note").unwrap();

    commit_richtext(&mut card, "intro", &serde_json::json!("**bold** intro"), false).unwrap();
    assert_eq!(card.field_markdown("intro").unwrap(), "**bold** intro\n");

    // null passes through (stored as null) and reads back as the empty corpus.
    commit_richtext(&mut card, "intro", &serde_json::Value::Null, false).unwrap();
    assert!(card.payload().get("intro").unwrap().as_json().is_null());
    assert!(card.field_richtext("intro").unwrap().unwrap().is_blank());

    assert_eq!(
        commit_richtext(&mut card, "intro", &serde_json::json!({ "not": "a corpus" }), false)
            .unwrap_err()
            .variant_name(),
        "FieldRichtextDecode"
    );
    assert_eq!(
        commit_richtext(&mut card, "intro", &serde_json::json!(42), false)
            .unwrap_err()
            .variant_name(),
        "FieldRichtextDecode"
    );
}

/// A richtext(inline) `commit_field` commits the `richtext(inline)` check at
/// write: a single-`Para` corpus stores, a multi-block corpus is
/// `EditError::FieldRichtextNotInline` — the write is the strict commit, not a
/// deferred render failure.
#[test]
fn test_commit_field_richtext_inline_enforced_at_write() {
    let mut card = Card::new("note").unwrap();

    // One paragraph line: inline, accepted.
    commit_richtext(&mut card, "title", &serde_json::json!("A single line"), true).unwrap();
    assert_eq!(card.field_markdown("title").unwrap(), "A single line\n");

    // Two blocks: rejected at write, and nothing is stored over the good value.
    let err = commit_richtext(
        &mut card,
        "title",
        &serde_json::json!("line one\n\nline two"),
        true,
    )
    .unwrap_err();
    assert_eq!(err.variant_name(), "FieldRichtextNotInline");
    assert_eq!(card.field_markdown("title").unwrap(), "A single line\n");
}

/// A multi-block element committed to an `array` of `richtext(inline)` items
/// classifies as `FieldRichtextNotInline`, not the generic `FieldConform`: the
/// mapper keys on the coercion target (`richtext(inline)`), so the richtext
/// constraint is honored even when it is nested under an array. Issue #906.
#[test]
fn test_commit_field_array_of_inline_richtext_reports_not_inline() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let mut schema = FieldSchema::new("refs".to_string(), FieldType::Array, None);
    schema.items = Some(Box::new(FieldSchema::new(
        "refs".to_string(),
        FieldType::RichText { inline: true },
        None,
    )));

    // A single-line element coerces to a corpus and the array commits.
    card.commit_field(
        "refs",
        QuillValue::from_json(serde_json::json!(["one line"])),
        &schema,
    )
    .unwrap();

    // A two-block element fails the inline check, surfaced as the richtext
    // variant (not FieldConform) despite the field's own type being Array.
    let err = card
        .commit_field(
            "refs",
            QuillValue::from_json(serde_json::json!(["line one\n\nline two"])),
            &schema,
        )
        .unwrap_err();
    assert_eq!(err.variant_name(), "FieldRichtextNotInline");
}

// ── commit_field (typed write) ───────────────────────────────────────────────

/// `commit_field` on a scalar schema stores the coerced canonical (`"3"` → `3`);
/// a strict write drops the render floor's cross-type coercions (a `bool` for an
/// `integer` field) and fails a shape mismatch with `EditError::FieldConform`.
#[test]
fn test_commit_field_scalar_strict() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let int_schema = FieldSchema::new("qty".to_string(), FieldType::Integer, None);

    // Value-parsing normalization survives: "3" → 3.
    card.commit_field("qty", QuillValue::from_json(serde_json::json!("3")), &int_schema)
        .unwrap();
    assert_eq!(
        card.payload().get("qty").unwrap().as_json(),
        &serde_json::json!(3)
    );

    // Cross-type boolean→integer is a render-floor leniency, dropped on write.
    let err = card
        .commit_field("qty", QuillValue::from_json(serde_json::json!(true)), &int_schema)
        .unwrap_err();
    assert_eq!(err.variant_name(), "FieldConform");

    // A non-numeric string is a mismatch and fails now, not at render.
    assert_eq!(
        card.commit_field("qty", QuillValue::from_json(serde_json::json!("x")), &int_schema)
            .unwrap_err()
            .variant_name(),
        "FieldConform"
    );
    // The good value is untouched by the failed writes.
    assert_eq!(
        card.payload().get("qty").unwrap().as_json(),
        &serde_json::json!(3)
    );
}

/// `commit_field` on an `object` schema fails a non-object value with
/// `FieldConform`, where the render floor would defer to validation.
#[test]
fn test_commit_field_object_rejects_non_object() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let schema = FieldSchema::new("meta".to_string(), FieldType::Object, None);
    assert_eq!(
        card.commit_field("meta", QuillValue::from_json(serde_json::json!(42)), &schema)
            .unwrap_err()
            .variant_name(),
        "FieldConform"
    );
}

/// A malformed field name fails with `InvalidFieldName` before any coercion.
#[test]
fn test_commit_field_rejects_bad_name() {
    use crate::quill::{FieldSchema, FieldType};

    let mut card = Card::new("note").unwrap();
    let schema = FieldSchema::new("$bad".to_string(), FieldType::Integer, None);
    assert_eq!(
        card.commit_field("$bad", QuillValue::from_json(serde_json::json!(1)), &schema)
            .unwrap_err()
            .variant_name(),
        "InvalidFieldName"
    );
}

/// `field_richtext` on an absent field is `None`; on a plain non-richtext field
/// value it is `Some(Err(_))` — the read mirrors the write in needing the caller
/// to name a field it knows is richtext.
#[test]
fn test_field_richtext_absent_and_non_richtext() {
    let mut card = Card::new("note").unwrap();
    assert!(card.field_richtext("missing").is_none());
    assert!(card.field_markdown("missing").is_none());

    card.set_field("count", 3).unwrap();
    assert!(card.field_richtext("count").unwrap().is_err());
    assert!(card.field_markdown("count").is_none());
}

/// A corpus-valued field emits to card-yaml as its **markdown projection**, not
/// a nested `{text, lines, marks, islands}` object — card-yaml stays
/// markdown-clean. Re-parsing yields a string field (schema-less parse), the
/// documented on-disk identity boundary; the corpus survives only via the DTO.
#[test]
fn test_corpus_field_emits_as_markdown_projection() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    commit_richtext(
        doc.main_mut(),
        "intro",
        &serde_json::json!("**bold** intro"),
        false,
    )
    .unwrap();

    let md = doc.to_markdown();
    // Projected to a quoted markdown scalar (the `body_markdown` projection,
    // trailing newline and all), not a block mapping.
    assert!(
        md.contains("intro: \"**bold** intro\\n\""),
        "expected markdown projection, got:\n{md}"
    );
    assert!(!md.contains("lines:"), "corpus object leaked into card-yaml:\n{md}");

    // Re-parse: schema-less, so the field is now a plain string (identity lost
    // on disk — the intended boundary), but the markdown content survives.
    let reparsed = Document::from_markdown(&md).unwrap();
    assert_eq!(
        reparsed.main().payload().get("intro").unwrap().as_str(),
        Some("**bold** intro\n")
    );
}

/// A plain object field that is *not* a canonical corpus emits structurally
/// (unchanged) — projection fires only on genuine corpus objects.
#[test]
fn test_non_corpus_object_field_emits_structurally() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    doc.main_mut()
        .set_field(
            "addr",
            QuillValue::from_json(serde_json::json!({ "city": "Paris" })),
        )
        .unwrap();
    let md = doc.to_markdown();
    assert!(md.contains("addr:"), "{md}");
    assert!(md.contains("city: Paris"), "{md}");
}

/// A content-canonical corpus whose top-level keys are in NON-canonical order
/// stays structural — the projection guard is byte-exact (canonical-string
/// equality), not the order-independent `Value` compare it used to be. Under
/// the old guard this projected to a flattened markdown scalar, breaking the
/// field's round-trip. Issue #905.
#[test]
fn test_noncanonical_order_corpus_field_stays_structural() {
    // A real corpus, then its top-level keys rebuilt in reverse (same content,
    // non-canonical bytes).
    let rt = quillmark_richtext::import::from_markdown("**bold**").unwrap();
    let canonical = quillmark_richtext::serial::to_canonical_value(&rt);
    let obj = canonical.as_object().unwrap();
    let mut scrambled = serde_json::Map::new();
    for k in obj.keys().rev() {
        scrambled.insert(k.clone(), obj[k].clone());
    }

    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());
    doc.main_mut()
        .set_field(
            "intro",
            QuillValue::from_json(serde_json::Value::Object(scrambled)),
        )
        .unwrap();
    let md = doc.to_markdown();
    // Structural: the corpus keys survive; it did not collapse to `intro: "..."`.
    assert!(
        md.contains("marks:") && md.contains("lines:"),
        "non-canonical-order corpus should stay structural, got:\n{md}"
    );
}

/// `revise_body` updates the body and returns the text delta from the old body
/// to the new — the recordable whole-document (stale-text) writer.
#[test]
fn test_revise_body_returns_delta_and_updates_body() {
    use crate::{Assoc, Delta};

    let mut card = Card::new("note").unwrap();
    card.revise_body("hello world").unwrap();
    let delta: Delta = card.revise_body("hello brave world").unwrap();
    assert_eq!(card.body().text, "hello brave world");
    // The delta maps a stale position at the end of "hello " forward across
    // the inserted "brave ".
    assert_eq!(delta.map_pos(6, Assoc::Before), 6);
    assert_eq!(delta.map_pos(11, Assoc::After), 17);
}

/// A whole-document markdown revise rebases a surviving identity anchor onto
/// the new text via `diff_import`, where the old fresh-import path dropped it.
#[test]
fn test_revise_body_rebases_anchor() {
    use quillmark_richtext::model::{Mark, MarkKind};

    let mut base = quillmark_richtext::import::from_markdown("keep the target word").unwrap();
    // Anchor over "target" (chars 9..15).
    base.marks.push(Mark {
        start: 9,
        end: 15,
        kind: MarkKind::Anchor { id: "c1".into() },
    });
    base.normalize();
    let mut card = Card::new("note").unwrap();
    card.install_body(base);

    card.revise_body("why keep the target word").unwrap();
    let anchor = card
        .body()
        .marks
        .iter()
        .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
        .expect("identity anchor survives the whole-document replace");
    let text = &card.body().text;
    let s = quillmark_richtext::usv::char_to_byte(text, anchor.start);
    let e = quillmark_richtext::usv::char_to_byte(text, anchor.end);
    assert_eq!(&text[s..e], "target");
}

/// `apply_body_change` applies a native field-change bundle (text delta, then
/// line ops, then mark ops) to the body corpus.
#[test]
fn test_apply_body_change_applies_bundle() {
    use crate::MarkOp;
    use quillmark_richtext::delta::diff;
    use quillmark_richtext::model::MarkKind;

    let mut card = Card::new("note").unwrap();
    card.revise_body("abc").unwrap();
    let d = diff("abc", "abXc");
    card.apply_body_change(
        &d,
        &[],
        &[MarkOp::Add {
            start: 3,
            end: 4,
            kind: MarkKind::Strong,
        }],
    )
    .unwrap();
    assert_eq!(card.body().text, "abXc");
    let strong = card
        .body()
        .marks
        .iter()
        .find(|m| matches!(m.kind, MarkKind::Strong))
        .expect("strong mark applied post-delta");
    assert_eq!((strong.start, strong.end), (3, 4));
}

/// An out-of-range mark op surfaces as `EditError::CorpusApply` rather than a
/// panic or a silent no-op.
#[test]
fn test_apply_body_change_reports_out_of_range() {
    use crate::MarkOp;
    use quillmark_richtext::delta::diff;
    use quillmark_richtext::model::MarkKind;

    let mut card = Card::new("note").unwrap();
    card.revise_body("abc").unwrap();
    let identity = diff("abc", "abc");
    let result = card.apply_body_change(
        &identity,
        &[],
        &[MarkOp::Add {
            start: 0,
            end: 99,
            kind: MarkKind::Strong,
        }],
    );
    match result {
        Err(EditError::CorpusApply(_)) => {}
        other => panic!("expected CorpusApply, got {other:?}"),
    }
}

/// `apply_field_richtext_change` splices a bundle into a richtext field's stored
/// corpus and re-stores it — the field-path twin of `apply_body_change`.
/// Identity marks (a strong span applied post-delta) survive on the re-stored
/// corpus, which is what makes anchors persist on field content across edits.
#[test]
fn test_apply_field_richtext_change_splices_and_persists() {
    use crate::MarkOp;
    use quillmark_richtext::delta::diff;
    use quillmark_richtext::model::MarkKind;

    let mut card = Card::new("note").unwrap();
    commit_richtext(&mut card, "intro", &serde_json::json!("abc"), false).unwrap();
    let d = diff("abc", "abXc");
    card.apply_field_richtext_change(
        "intro",
        &d,
        &[],
        &[MarkOp::Add {
            start: 3,
            end: 4,
            kind: MarkKind::Strong,
        }],
    )
    .unwrap();

    // The stored value is still a canonical corpus object carrying the edit.
    assert!(card.payload().get("intro").unwrap().as_json().is_object());
    let rt = card.field_richtext("intro").unwrap().unwrap();
    assert_eq!(rt.text, "abXc");
    assert!(rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Strong)));
}

/// `apply_field_richtext_change` on an absent or non-richtext field is a
/// `FieldRichtextDecode` error, not a panic — the caller must address a field it
/// knows is richtext.
#[test]
fn test_apply_field_richtext_change_rejects_non_richtext() {
    use quillmark_richtext::delta::diff;

    let mut card = Card::new("note").unwrap();
    let identity = diff("", "");
    assert_eq!(
        card.apply_field_richtext_change("missing", &identity, &[], &[])
            .unwrap_err()
            .variant_name(),
        "FieldRichtextDecode"
    );

    card.set_field("count", 3).unwrap();
    assert_eq!(
        card.apply_field_richtext_change("count", &identity, &[], &[])
            .unwrap_err()
            .variant_name(),
        "FieldRichtextDecode"
    );
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
    doc.main_mut().revise_body("Updated body.").unwrap();

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
    // `$body` is canonical corpus JSON; its `text` is the content-only string.
    assert_eq!(json["$body"]["text"].as_str(), Some("Updated body."));

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
    doc.main_mut().set_ext(ext).expect("set_ext");

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
    doc.main_mut().set_ext(ext).expect("set_ext");

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
    doc.main_mut().set_ext(ext).expect("set_ext");

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
        .set_ext_namespace("presentation", serde_json::json!({ "title": "A" }))
        .expect("set_ext_namespace");
    doc.main_mut()
        .set_ext_namespace("agent", serde_json::json!({ "pinned": true }))
        .expect("set_ext_namespace");

    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("A"));
    assert_eq!(ext["agent"]["pinned"].as_bool(), Some(true));

    // Replacing one namespace leaves the other intact.
    doc.main_mut()
        .set_ext_namespace("presentation", serde_json::json!({ "title": "B" }))
        .expect("set_ext_namespace");
    let ext = doc.main().ext().unwrap();
    assert_eq!(ext["presentation"]["title"].as_str(), Some("B"));
    assert_eq!(ext["agent"]["pinned"].as_bool(), Some(true));
}

#[test]
fn test_remove_ext_namespace_preserves_siblings() {
    let mut doc = make_doc();
    doc.main_mut()
        .set_ext_namespace("presentation", serde_json::json!({ "title": "A" }))
        .expect("set_ext_namespace");
    doc.main_mut()
        .set_ext_namespace("tutorial", serde_json::json!(["step-1", "step-2"]))
        .expect("set_ext_namespace");

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
        .set_ext_namespace("tutorial", serde_json::json!(["step-1"]))
        .expect("set_ext_namespace");

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
        .set_ext_namespace("presentation", serde_json::json!({ "title": "A" }))
        .expect("set_ext_namespace");
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
    doc.main_mut()
        .set_ext(serde_json::Map::new())
        .expect("set_ext");
    assert!(doc.main().ext().is_some());
    let md = doc.to_markdown();
    assert!(md.contains("$ext: {}"), "got: {md}");
}

#[test]
fn test_ext_mutators_work_on_composable_cards() {
    let mut doc = make_doc_with_cards();
    doc.card_mut(0)
        .unwrap()
        .set_ext_namespace("agent", serde_json::json!({ "note": "x" }))
        .expect("set_ext_namespace");
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

// ── Single-card / $id reads (#956) ───────────────────────────────────────────

#[test]
fn find_card_returns_first_match_and_card_indexes() {
    let mut doc = Document::new(QuillReference::from_str("test_quill").unwrap());

    let mut c0 = Card::new("item").unwrap();
    c0.payload_mut().set_id("dup");
    let mut c1 = Card::new("item").unwrap();
    c1.payload_mut().set_id("other");
    let mut c2 = Card::new("item").unwrap();
    c2.payload_mut().set_id("dup");
    doc.push_card(c0).unwrap();
    doc.push_card(c1).unwrap();
    doc.push_card(c2).unwrap();

    // `$id` is non-unique by design — find_card returns the first match.
    let (idx, card) = doc.find_card("dup").unwrap();
    assert_eq!(idx, 0);
    assert_eq!(card.id(), Some("dup"));
    assert_eq!(doc.find_card("other").map(|(i, _)| i), Some(1));
    assert!(doc.find_card("missing").is_none());

    // card(i) is the immutable single-card read; None out of range.
    assert_eq!(doc.card(1).unwrap().id(), Some("other"));
    assert!(doc.card(3).is_none());
}
