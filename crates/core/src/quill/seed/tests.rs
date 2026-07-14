//! Tests for [`Quill::seed_document`], [`Quill::seed_main`], and
//! [`Quill::seed_card`].

use std::collections::HashMap;

use serde_json::json;

use crate::quill::FileTreeNode;

use crate::{Document, Quill, SeedOverlay, Severity};

/// Build a [`SeedOverlay`] from a JSON object (the `$seed[<kind>]` shape).
fn overlay(value: serde_json::Value) -> SeedOverlay {
    SeedOverlay::from_json(&value).expect("overlay json must be an object")
}

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
    Quill::from_tree(root).expect("quill_from_yaml: Quill::from_tree failed")
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
    assert_eq!(
        card.kind(),
        Some("main"),
        "main card must carry $kind: main"
    );

    // Body region carries `body.example`.
    assert_eq!(card.body_markdown(), "Main body text.\n");
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

    // Main metadata, including `$kind: main`, survives the round trip.
    assert_eq!(
        reparsed.main().quill().map(|r| r.name.as_str()),
        Some("seed_test")
    );
    assert_eq!(reparsed.main().kind(), Some("main"));
    assert_eq!(
        reparsed
            .main()
            .payload()
            .get("title")
            .and_then(|v| v.as_str()),
        Some("FIRSTNAME LASTNAME"),
    );
    // The markdown layer normalizes a body to a single trailing newline;
    // assert the exact normalized form rather than hiding it behind a trim.
    assert_eq!(reparsed.main().body_markdown(), "Main body text.\n");

    // The composable card survives with its kind and seeded value.
    assert_eq!(reparsed.cards().len(), 1);
    assert_eq!(reparsed.cards()[0].kind(), Some("note"));
    assert_eq!(
        reparsed.cards()[0]
            .payload()
            .get("author")
            .and_then(|v| v.as_str()),
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

    let note = quill.seed_card("note", None).expect("known kind");
    assert_eq!(note.kind(), Some("note"));
    assert_eq!(
        note.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );

    assert!(
        quill.seed_card("missing", None).is_none(),
        "unknown kind must return None"
    );
}

// ── Overlay layering (overlay › example › absent) ───────────────────────────

#[test]
fn overlay_overrides_example_and_falls_through_for_untouched_fields() {
    let quill = quill_from_yaml(QUILL);
    // Override only `author`; leave the rest to the schema seed.
    let ov = overlay(json!({ "author": "Custom Author" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(
        card.payload().get("author").and_then(|v| v.as_str()),
        Some("Custom Author"),
        "overlay value wins over the example",
    );
}

#[test]
fn overlay_adds_a_field_the_base_omits() {
    let quill = quill_from_yaml(QUILL);
    // `tag` has no example, so the bare seed omits it; the overlay adds it.
    assert!(quill
        .seed_card("note", None)
        .unwrap()
        .payload()
        .get("tag")
        .is_none());
    let ov = overlay(json!({ "tag": "pinned" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(
        card.payload().get("tag").and_then(|v| v.as_str()),
        Some("pinned")
    );
    // `author` still flows from its example (sparse fall-through).
    assert_eq!(
        card.payload().get("author").and_then(|v| v.as_str()),
        Some("A. Author"),
    );
}

#[test]
fn overlay_fields_are_ordered_by_declaration() {
    let quill = quill_from_yaml(QUILL);
    // Overlay touches `tag` (declared 2nd) and `author` (declared 1st); the
    // result must still be in declaration order, not overlay insertion order.
    let ov = overlay(json!({ "tag": "pinned", "author": "Custom" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    let keys: Vec<&str> = card.payload().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["author", "tag"]);
}

#[test]
fn overlay_added_field_lands_in_declaration_position() {
    // The seed is driven by schema declaration order, so an overlay that adds a
    // base-omitted field lands at its *declared* position, not appended last.
    // Here `alpha` (no example, declared first) is base-omitted while `beta`
    // (declared second) flows from its example; overlaying `alpha` must place
    // it ahead of `beta`.
    let quill = quill_from_yaml(
        r#"
quill:
  name: order_seed
  version: "1.0"
  backend: typst
  description: Seed order test
card_kinds:
  note:
    fields:
      alpha:
        type: string
      beta:
        type: string
        example: B
"#,
    );
    let ov = overlay(json!({ "alpha": "A" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    let keys: Vec<&str> = card.payload().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["alpha", "beta"]);
}

#[test]
fn overlay_body_overrides_and_non_schema_keys_are_ignored() {
    let quill = quill_from_yaml(QUILL);
    // `note` has no body.example, so the bare seed body is empty.
    assert_eq!(quill.seed_card("note", None).unwrap().body_markdown(), "");
    // An overlay `$body` wins; a non-schema field is ignored.
    let ov = overlay(json!({ "author": "X", "$body": "Overlay body.", "bogus": "drop me" }));
    let card = quill.seed_card("note", Some(&ov)).expect("known kind");
    assert_eq!(card.body_markdown(), "Overlay body.\n");
    assert!(
        card.payload().get("bogus").is_none(),
        "a key naming no schema field must not land on the card",
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

    let card = quill.seed_card("data", None).expect("known kind");
    assert_eq!(
        card.body_markdown(),
        "",
        "body must be empty when body.enabled is false"
    );
    assert_eq!(
        card.payload().get("value").and_then(|v| v.as_str()),
        Some("V"),
    );
}

// ── Advisory `$seed` validation (editor surface only; never gates render) ────

/// A minimal `seed_test` document carrying a raw `$seed` YAML block.
fn doc_with_seed(seed_block: &str) -> Document {
    let md = format!("~~~card-yaml\n$quill: seed_test@1.0\n$kind: main\n{seed_block}~~~\n");
    Document::from_markdown(&md).expect("doc should parse")
}

#[test]
fn seed_overlay_type_mismatch_is_advisory_and_does_not_gate_render() {
    let quill = quill_from_yaml(QUILL);
    // `author` is a string; an integer overlay value is a type mismatch.
    let doc = doc_with_seed("$seed:\n  note:\n    author: 123\n");

    let diags = quill.validate(&doc);
    let seed_diag = diags
        .iter()
        .find(|d| d.path.as_deref() == Some("$seed.note.author"))
        .expect("a diagnostic rooted at the seed field");
    assert_eq!(
        seed_diag.severity,
        Severity::Warning,
        "seed diagnostics are advisory, not errors",
    );

    // The malformed overlay never blocks render — `$seed` is stripped.
    assert!(
        quill.compile_data(&doc).is_ok(),
        "compile_data must ignore $seed"
    );
    assert!(quill.dry_run(&doc).is_ok(), "dry_run must ignore $seed");
}

#[test]
fn seed_overlay_unknown_kind_is_flagged_but_renders() {
    let quill = quill_from_yaml(QUILL);
    let doc = doc_with_seed("$seed:\n  bogus_kind:\n    x: 1\n");
    let diags = quill.validate(&doc);
    let d = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("validation::seed_unknown_kind"))
        .expect("unknown-kind advisory");
    assert_eq!(d.path.as_deref(), Some("$seed.bogus_kind"));
    assert_eq!(d.severity, Severity::Warning);
    assert!(quill.compile_data(&doc).is_ok());
}

#[test]
fn well_formed_seed_overlay_yields_no_seed_diagnostics() {
    let quill = quill_from_yaml(QUILL);
    let doc = doc_with_seed("$seed:\n  note:\n    author: Custom\n");
    let diags = quill.validate(&doc);
    assert!(
        !diags
            .iter()
            .any(|d| d.path.as_deref().is_some_and(|p| p.starts_with("$seed"))),
        "a well-formed overlay should produce no seed diagnostics: {diags:?}",
    );
}
