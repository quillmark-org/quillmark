//! Tests for [`Quill::seed_document`], [`Quill::seed_main`], and
//! [`Quill::seed_card`].

use std::collections::HashMap;

use quillmark_core::quill::FileTreeNode;

use crate::{Quill, Quillmark};

/// Build a minimal [`Quill`] from inline YAML with no filesystem dependencies.
fn quill_from_yaml(yaml: &str) -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    let root = FileTreeNode::Directory { files };
    Quillmark::new()
        .quill(root)
        .expect("quill_from_yaml: engine.quill failed")
}

const QUILL: &str = r#"
quill:
  name: seed_test
  version: "1.0"
  backend: typst
  description: Seed test
main:
  body:
    example: "Main body text."
  fields:
    title:
      type: string
      example: FIRSTNAME LASTNAME
    status:
      type: string
      default: draft
    notes:
      type: string
card_kinds:
  note:
    fields:
      author:
        type: string
        example: A. Author
      tag:
        type: string
"#;

#[test]
fn seed_main_commits_only_example_fields() {
    let quill = quill_from_yaml(QUILL);
    let card = quill.seed_main();
    let payload = card.payload();

    // A field with an `example` is committed verbatim.
    assert_eq!(
        payload.get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    // A field with only a `default` (no example) is left absent.
    assert!(
        payload.get("status").is_none(),
        "default-only field must be absent (interpolated at render)"
    );
    // A field with neither is left absent.
    assert!(payload.get("notes").is_none());

    // Main card carries `$quill: name@version` and `$kind: main`.
    let reference = card.quill().expect("main card must carry $quill");
    assert_eq!(reference.name, "seed_test");
    assert_eq!(card.kind(), Some("main"), "main card must carry $kind: main");

    // Body region carries `body.example`.
    assert_eq!(card.body(), "Main body text.");
}

/// A seeded document re-parses from its own markdown with `$quill` / `$kind`
/// metadata and seeded values intact. (Body whitespace is normalized by the
/// markdown layer, so this asserts structural fidelity, not byte-equality.)
#[test]
fn seeded_document_round_trips_through_markdown() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    let markdown = doc.to_markdown();
    let reparsed = crate::Document::from_markdown(&markdown)
        .expect("seeded document must re-parse from its own markdown");

    // Main metadata survives — this is what the `$kind: main` fix guarantees.
    assert_eq!(reparsed.main().quill().map(|r| r.name.as_str()), Some("seed_test"));
    assert_eq!(reparsed.main().kind(), Some("main"));
    assert_eq!(
        reparsed.main().payload().get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    // The markdown layer normalizes a body to a single trailing newline;
    // assert the exact normalized form rather than hiding it behind a trim.
    assert_eq!(reparsed.main().body(), "Main body text.\n");

    // The composable card survives with its kind and seeded value.
    assert_eq!(reparsed.cards().len(), 1);
    assert_eq!(reparsed.cards()[0].kind(), Some("note"));
    assert_eq!(
        reparsed.cards()[0].payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );
}

#[test]
fn seed_document_emits_one_seeded_card_per_kind() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    // Main seeded.
    assert_eq!(
        doc.main().payload().get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );

    // Exactly one instance of the single declared kind.
    assert_eq!(doc.cards().len(), 1);
    let note = &doc.cards()[0];
    assert_eq!(note.kind(), Some("note"));
    assert!(
        note.quill().is_none(),
        "composable card must not carry $quill"
    );
    // Example field committed; non-example field absent.
    assert_eq!(
        note.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );
    assert!(note.payload().get("tag").is_none());
}

/// The whole point of `example → absent`: a seeded document needs no
/// provenance to render, and absent fields pick up `default` then type-empty
/// zero at the render layer — never persisted, so editor and preview agree.
#[test]
fn seeded_document_compiles_with_default_then_zero_for_absent_fields() {
    let quill = quill_from_yaml(QUILL);
    let doc = quill.seed_document();

    let data = quill
        .compile_data(&doc)
        .expect("seeded document must compile");

    // Committed example survives.
    assert_eq!(
        data.get("title").and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    // Absent default-field resolves to its schema default.
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("draft"));
    // Absent no-default field resolves to the type-empty zero value.
    assert_eq!(data.get("notes").and_then(|v| v.as_str()), Some(""));
}

#[test]
fn seed_card_for_known_and_unknown_kind() {
    let quill = quill_from_yaml(QUILL);

    let note = quill.seed_card("note").expect("known kind");
    assert_eq!(note.kind(), Some("note"));
    assert_eq!(
        note.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );

    assert!(
        quill.seed_card("missing").is_none(),
        "unknown kind must return None"
    );
}

/// `body.example` is only seeded when bodies are enabled for the kind.
#[test]
fn seed_omits_body_when_body_disabled() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: bodyless
  version: "1.0"
  backend: typst
  description: Bodyless card test
main:
  fields:
    title:
      type: string
      example: T
card_kinds:
  data:
    body:
      enabled: false
    fields:
      value:
        type: string
        example: V
"#,
    );

    let card = quill.seed_card("data").expect("known kind");
    assert_eq!(
        card.body(),
        "",
        "body must be empty when body.enabled is false"
    );
    assert_eq!(
        card.payload().get("value").and_then(|v| v.as_str()),
        Some("V"),
    );
}
