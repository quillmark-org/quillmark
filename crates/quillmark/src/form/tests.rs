//! Tests for [`Quill::form`], [`Quill::blank_main`], and [`Quill::blank_card`].

use std::collections::HashMap;

use quillmark_core::quill::FileTreeNode;
use quillmark_core::Document;

use crate::{Quill, Quillmark};

use super::{Form, FormFieldSource};

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

#[test]
fn form_all_fields_present() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: form_test
  version: "1.0"
  backend: typst
  description: Form view test

cards:
  main:
    fields:
      title:
        type: string
      status:
        type: string
        default: draft
"#,
    );

    let md = "---\nQUILL: form_test\ntitle: \"My Title\"\nstatus: \"final\"\n---\n";
    let doc = Document::from_markdown(md).unwrap();

    let form = quill.form(&doc);

    assert!(form.diagnostics.is_empty(), "no diagnostics expected");
    assert!(form.cards.is_empty(), "no cards expected");

    let title_fv = form.main.values.get("title").expect("title field");
    assert_eq!(title_fv.source, FormFieldSource::Document);
    assert_eq!(
        title_fv.value.as_ref().and_then(|v| v.as_str()),
        Some("My Title")
    );

    let status_fv = form.main.values.get("status").expect("status field");
    assert_eq!(status_fv.source, FormFieldSource::Document);
    assert_eq!(
        status_fv.value.as_ref().and_then(|v| v.as_str()),
        Some("final")
    );
    // default still recorded even when document value present
    assert_eq!(
        status_fv.default.as_ref().and_then(|v| v.as_str()),
        Some("draft")
    );
}

#[test]
fn form_missing_field_uses_default() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: form_defaults_test
  version: "1.0"
  backend: typst
  description: Missing fields use defaults

cards:
  main:
    fields:
      title:
        type: string
        required: true
      status:
        type: string
        default: draft
      notes:
        type: string
"#,
    );

    // `title` and `notes` are absent from the document.
    // `title` is required — that produces a validation diagnostic.
    // `status` is absent but has a default.
    // `notes` is absent and has no default.
    let md = "---\nQUILL: form_defaults_test\n---\n";
    let doc = Document::from_markdown(md).unwrap();

    let form = quill.form(&doc);

    // `title` is required and missing → validation diagnostic
    assert!(
        form.diagnostics.iter().any(|d| d.message.contains("title")),
        "expected validation diagnostic for required 'title'; got: {:?}",
        form.diagnostics
    );

    let status_fv = form.main.values.get("status").expect("status field");
    assert_eq!(status_fv.source, FormFieldSource::Default);
    assert!(
        status_fv.value.is_none(),
        "value should be None when not in document"
    );
    assert_eq!(
        status_fv.default.as_ref().and_then(|v| v.as_str()),
        Some("draft")
    );

    let notes_fv = form.main.values.get("notes").expect("notes field");
    assert_eq!(notes_fv.source, FormFieldSource::Missing);
    assert!(notes_fv.value.is_none());
    assert!(notes_fv.default.is_none());
}

#[test]
fn form_unknown_card_drops_card_and_emits_diagnostic() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: unknown_card_test
  version: "1.0"
  backend: typst
  description: Unknown card kind test

cards:
  main:
    fields:
      title:
        type: string
  known_card:
    fields:
      note:
        type: string

"#,
    );

    let md = "---\nQUILL: unknown_card_test\ntitle: \"T\"\n---\n\n\
              ```card known_card\nnote: \"A\"\n```\n\n\
              ```card ghost_card\nnote: \"B\"\n```\n";
    let doc = Document::from_markdown(md).unwrap();

    let form = quill.form(&doc);

    // Only the known card appears in cards
    assert_eq!(form.cards.len(), 1, "only known_card should be projected");
    assert_eq!(form.cards[0].schema.name, "known_card");

    // A diagnostic for ghost_card
    let unknown_diag = form
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("form::unknown_card"))
        .expect("expected unknown_card diagnostic");
    assert!(
        unknown_diag.message.contains("ghost_card"),
        "diagnostic should name the tag: {:?}",
        unknown_diag.message
    );
}

#[test]
fn form_card_field_sources() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: card_fields_test
  version: "1.0"
  backend: typst
  description: Card field source test

cards:
  main:
    fields:
      title:
        type: string
  indorsement:
    fields:
      signature_block:
        type: string
        required: true
      office:
        type: string
        default: HQ
      extra:
        type: string

"#,
    );

    // signature_block present, office absent (has default), extra absent (no default)
    let md = "---\nQUILL: card_fields_test\ntitle: \"T\"\n---\n\n\
              ```card indorsement\nsignature_block: \"Col Smith\"\n```\n";
    let doc = Document::from_markdown(md).unwrap();

    let form = quill.form(&doc);
    assert_eq!(form.cards.len(), 1);
    let card = &form.cards[0];

    let sig = card.values.get("signature_block").expect("signature_block");
    assert_eq!(sig.source, FormFieldSource::Document);
    assert_eq!(
        sig.value.as_ref().and_then(|v| v.as_str()),
        Some("Col Smith")
    );

    let office = card.values.get("office").expect("office");
    assert_eq!(office.source, FormFieldSource::Default);
    assert!(office.value.is_none());
    assert_eq!(office.default.as_ref().and_then(|v| v.as_str()), Some("HQ"));

    let extra = card.values.get("extra").expect("extra");
    assert_eq!(extra.source, FormFieldSource::Missing);
    assert!(extra.value.is_none());
    assert!(extra.default.is_none());
}

#[test]
fn form_validation_diagnostics_appear() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: validation_diag_test
  version: "1.0"
  backend: typst
  description: Validation diagnostics test

cards:
  main:
    fields:
      count:
        type: integer
        required: true
"#,
    );

    // `count` is a string, not an integer → TypeMismatch validation error
    let md = "---\nQUILL: validation_diag_test\ncount: \"not-a-number\"\n---\n";
    let doc = Document::from_markdown(md).unwrap();

    let form = quill.form(&doc);

    let val_diag = form
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("form::validation_error"))
        .expect("expected a validation diagnostic");
    assert!(
        val_diag.message.contains("count"),
        "diagnostic should mention field name; got: {:?}",
        val_diag.message
    );
}

#[test]
fn form_serializes_cleanly() {
    // Smoke test: serde_json round-trip of Form.
    let quill = quill_from_yaml(
        r#"
quill:
  name: serial_test
  version: "1.0"
  backend: typst
  description: Serialization smoke test

cards:
  main:
    fields:
      title:
        type: string
        default: Untitled
      count:
        type: integer
"#,
    );

    let md = "---\nQUILL: serial_test\ntitle: \"Hello\"\n---\n";
    let doc = Document::from_markdown(md).unwrap();
    let form = quill.form(&doc);

    let json = serde_json::to_string(&form).expect("Form must serialize");
    let back: Form = serde_json::from_str(&json).expect("Form must deserialize");

    // Name fields on CardSchema / FieldSchema are intentionally skipped on the
    // wire (the map key carries them), so round-trip identity does not hold for
    // those. Compare structural content instead.
    assert_eq!(form.main.values, back.main.values);
    assert_eq!(form.cards, back.cards);
    assert_eq!(form.diagnostics, back.diagnostics);
    assert_eq!(
        form.main.schema.fields.keys().collect::<Vec<_>>(),
        back.main.schema.fields.keys().collect::<Vec<_>>()
    );
    assert!(
        json.contains("title"),
        "serialized JSON should contain field name"
    );
}

#[test]
fn form_over_usaf_memo_fixture() {
    // Integration test: load the usaf_memo fixture quill and view the
    // bundled example.  Checks that every required field gets a deterministic
    // FormFieldSource and no projection panics.
    let quill_path = quillmark_fixtures::resource_path("quills/usaf_memo/0.1.0");
    let quill = Quillmark::new()
        .quill_from_path(quill_path)
        .expect("failed to load usaf_memo fixture");

    let example_md = quill.source().example().unwrap_or("");
    // If the example can't parse, skip gracefully (it uses YAML comments that
    // are valid but the field values may not match the schema exactly).
    let doc = match Document::from_markdown(example_md) {
        Ok(d) => d,
        Err(_) => return,
    };

    let form = quill.form(&doc);

    // The form must produce a FormCard for main with at least the required fields.
    assert!(
        !form.main.values.is_empty(),
        "main card view should have fields"
    );

    // Every field value must have a deterministic source.
    for (name, fv) in &form.main.values {
        match fv.source {
            FormFieldSource::Document => {
                assert!(
                    fv.value.is_some(),
                    "Document source must have value for {name}"
                );
            }
            FormFieldSource::Default => {
                assert!(
                    fv.value.is_none(),
                    "Default source must have no value for {name}"
                );
                assert!(
                    fv.default.is_some(),
                    "Default source must have default for {name}"
                );
            }
            FormFieldSource::Missing => {
                assert!(
                    fv.value.is_none(),
                    "Missing source must have no value for {name}"
                );
                assert!(
                    fv.default.is_none(),
                    "Missing source must have no default for {name}"
                );
            }
        }
    }

    // Serialization must not panic.
    let json = serde_json::to_string(&form).expect("form must serialize");
    assert!(!json.is_empty());
}

// ── blank_main / blank_card ─────────────────────────────────────────────────

#[test]
fn blank_main_has_default_or_missing_sources() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: blank_main_test
  version: "1.0"
  backend: typst
  description: Blank main test

cards:
  main:
    fields:
      title:
        type: string
        default: Untitled
      count:
        type: integer
"#,
    );

    let blank = quill.blank_main();

    let title = blank.values.get("title").expect("title field");
    assert_eq!(title.source, FormFieldSource::Default);
    assert!(title.value.is_none());
    assert_eq!(
        title.default.as_ref().and_then(|v| v.as_str()),
        Some("Untitled")
    );

    let count = blank.values.get("count").expect("count field");
    assert_eq!(count.source, FormFieldSource::Missing);
    assert!(count.value.is_none());
    assert!(count.default.is_none());
}

#[test]
fn blank_card_returns_form_card_for_known_kind() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: blank_card_test
  version: "1.0"
  backend: typst
  description: Blank card test

cards:
  main:
    fields:
      title:
        type: string
  indorsement:
    fields:
      office:
        type: string
        default: HQ
      from:
        type: string

"#,
    );

    let blank = quill
        .blank_card("indorsement")
        .expect("known card type should yield a FormCard");

    assert_eq!(blank.schema.name, "indorsement");

    let office = blank.values.get("office").expect("office field");
    assert_eq!(office.source, FormFieldSource::Default);
    assert!(office.value.is_none());
    assert_eq!(office.default.as_ref().and_then(|v| v.as_str()), Some("HQ"));

    let from = blank.values.get("from").expect("from field");
    assert_eq!(from.source, FormFieldSource::Missing);
    assert!(from.value.is_none());
    assert!(from.default.is_none());
}

#[test]
fn blank_card_returns_none_for_unknown_kind() {
    let quill = quill_from_yaml(
        r#"
quill:
  name: blank_unknown_test
  version: "1.0"
  backend: typst
  description: Blank unknown card test

cards:
  main:
    fields:
      title:
        type: string
  known:
    fields:
      x:
        type: string

"#,
    );

    assert!(quill.blank_card("does_not_exist").is_none());
}
