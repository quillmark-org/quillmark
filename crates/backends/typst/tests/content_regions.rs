//! Auto-tagged content fields produce schema-path-keyed regions read from the
//! laid-out frame tree: a top-level markdown field, the multi-rect-per-field
//! shape when content breaks across pages, and the canonical card address
//! `$cards.<kind>.<n>.<field>` (per-kind 0-based ordinal, surviving interleaved
//! kinds). Plus the `form-field` `field:` schema-path binding, which keys a
//! widget region on a schema path rather than its `/T` name.

use std::collections::HashMap;

use quillmark_core::{Backend, FileTreeNode, Quill};
use quillmark_typst::TypstBackend;

/// A self-contained quill from a `Quill.yaml` + `plate.typ` pair. No fonts dir
/// is needed — Typst's embedded defaults render text — and the helper package
/// (`@local/quillmark-helper`) is injected by the backend.
fn quill(yaml: &str, plate: &str) -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

#[test]
fn content_fields_emit_frame_regions() {
    const YAML: &str = r#"
quill:
  name: content_regions
  version: 0.1.0
  backend: typst
  description: content region auto-tag test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a short intro paragraph
    body:
      type: markdown
      description: a long body that wraps and breaks across pages
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#data.body
"#;

    // `body` is long enough to overflow page 0 and continue, so the frame walk
    // should emit one region per page it touches.
    let long = "This is a markdown paragraph that wraps across several lines. ".repeat(200);
    let data = serde_json::json!({
        "intro": "A **short** intro paragraph on the first page.",
        "body": long,
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();

    // One region per logical field — `field` is unique across the result.
    let mut seen = std::collections::HashSet::new();
    assert!(
        regions.iter().all(|r| seen.insert(r.field.clone())),
        "each field appears at most once: {:?}",
        regions.iter().map(|r| &r.field).collect::<Vec<_>>()
    );

    // intro: one region, keyed on the schema path (not a widget name).
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(intro.len(), 1, "intro is one region");
    let [x0, y0, x1, y1] = intro[0].rect;
    assert!(
        x1 > x0 && y1 > y0,
        "intro has positive area: {:?}",
        intro[0].rect
    );

    // body spans four pages but collapses to a single region anchored to the
    // first page it occupies (page 0).
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert_eq!(body.len(), 1, "page-spanning body collapses to one region");
    assert_eq!(body[0].page, 0, "anchored to the first page it occupies");
    assert!(
        body[0].rect[2] - body[0].rect[0] > 200.0,
        "body anchor spans most of the text column: {:?}",
        body[0].rect
    );
}

#[test]
fn card_regions_use_canonical_kind_ordinal_path() {
    // Two kinds, interleaved alpha/beta/alpha. The card address is kind + 0-based
    // ordinal *within that kind*, so the second alpha is `.1` even though it is
    // the third card overall, and beta's ordinal is unaffected by alpha.
    const YAML: &str = r#"
quill:
  name: card_regions
  version: 0.1.0
  backend: typst
  description: card region path test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a top-level intro
card_kinds:
  alpha:
    description: alpha card
    fields:
      note:
        type: markdown
        description: alpha note
  beta:
    description: beta card
    fields:
      note:
        type: markdown
        description: beta note
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#for card in data.at("$cards", default: ()) {
  card.at("note", default: [])
  parbreak()
}
"#;
    let data = serde_json::json!({
        "intro": "Top-level intro.",
        "$cards": [
            {"$kind": "alpha", "note": "Alpha one."},
            {"$kind": "beta",  "note": "Beta one."},
            {"$kind": "alpha", "note": "Alpha two."},
        ],
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let fields: std::collections::HashSet<String> =
        session.regions().into_iter().map(|r| r.field).collect();

    for expected in [
        "intro",
        "$cards.alpha.0.note",
        "$cards.beta.0.note",
        "$cards.alpha.1.note",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }
    // No positional/absolute card address leaks through.
    assert!(
        !fields.iter().any(|f| f.starts_with("$cards.0.")
            || f.starts_with("$cards.1.")
            || f.starts_with("$cards.2.")),
        "card regions must use kind+ordinal, not positional index: {fields:?}"
    );
}

#[test]
fn form_field_region_needs_a_schema_binding() {
    // Only a schema-addressable widget surfaces a region. `field:` keys it on a
    // schema path (so a signature widget named "Signature" routes to
    // `signature_block`); a widget that binds none has only a `/T` name and
    // exposes nothing.
    const YAML: &str = r#"
quill:
  name: field_binding
  version: 0.1.0
  backend: typst
  description: form-field schema binding test
typst:
  plate_file: plate.typ
main:
  fields: {}
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field, signature-field
#set page(width: 600pt, height: 400pt, margin: 50pt)
#form-field("Plain", type: "text", value: "hi")
#signature-field("Signature", field: "signature_block")
"#;
    let session = TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
        .expect("open");
    let fields: std::collections::HashSet<String> =
        session.regions().into_iter().map(|r| r.field).collect();

    assert!(
        fields.contains("signature_block"),
        "a `field:`-bound widget keys on the schema path: {fields:?}"
    );
    assert!(
        !fields.contains("Plain"),
        "an unbound widget is not schema-addressable and exposes no region: {fields:?}"
    );
    assert!(
        !fields.contains("Signature"),
        "the bound widget must not also leak its `/T` name: {fields:?}"
    );
}
