//! Marker-tagged content produces schema-path-keyed regions read from the
//! laid-out frame tree — one region per (placement, page fragment): a
//! top-level markdown field, per-page fragments when content breaks across
//! pages, independent regions for a field placed twice, and the canonical card
//! address `$cards.<kind>.<n>.<field>` (per-kind 0-based ordinal, surviving
//! interleaved kinds). Plus the explicit `tagged(..)` placement path (scalars,
//! nested-tag collapse, schema-address validation, the card `$path` prefix)
//! and the `form-field` `field:` schema-path binding, which keys a widget
//! region on a schema path rather than its `/T` name.

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

    // intro: one placement, one page — exactly one region, keyed on the schema
    // path (not a widget name).
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(intro.len(), 1, "intro is one region");
    let [x0, y0, x1, y1] = intro[0].rect;
    assert!(
        x1 > x0 && y1 > y0,
        "intro has positive area: {:?}",
        intro[0].rect
    );

    // body spans several pages: one fragment per page it touches, in page
    // order starting at the page it opens on, each page at most once.
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert!(
        body.len() >= 2,
        "page-spanning body surfaces one fragment per page: {body:?}"
    );
    assert_eq!(body[0].page, 0, "first fragment on the page the body opens");
    let pages: Vec<usize> = body.iter().map(|r| r.page).collect();
    let mut sorted_pages = pages.clone();
    sorted_pages.sort();
    sorted_pages.dedup();
    assert_eq!(pages, sorted_pages, "fragments in page order, one per page");
    for r in &body {
        assert!(
            r.rect[2] - r.rect[0] > 200.0,
            "each body fragment spans most of the text column: {:?}",
            r.rect
        );
    }
}

#[test]
fn field_placed_twice_yields_independent_regions() {
    const YAML: &str = r#"
quill:
  name: two_placements
  version: 0.1.0
  backend: typst
  description: per-placement region test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a short paragraph placed twice
"#;
    // The auto-tagged value is placed at two sites with unrelated ink between
    // them; each placement must surface independently rather than as one
    // spanning union that would claim the middle content.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro

#lorem(40)

#data.intro
"#;
    let data = serde_json::json!({ "intro": "The same intro, placed twice." });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(intro.len(), 2, "two placements, two regions: {regions:?}");
    // Disjoint vertically (bottom-left origin: the later placement sits lower).
    assert!(
        intro[0].rect[1] > intro[1].rect[3] || intro[1].rect[1] > intro[0].rect[3],
        "placements do not union into a spanning rect: {:?} vs {:?}",
        intro[0].rect,
        intro[1].rect
    );
}

#[test]
fn tagged_scalar_placement_emits_region() {
    const YAML: &str = r#"
quill:
  name: tagged_scalar
  version: 0.1.0
  backend: typst
  description: explicit scalar tagging test
main:
  fields:
    subject:
      type: string
      description: a plain scalar the plate tags at its placement site
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, tagged
#set page(width: 612pt, height: 792pt, margin: 72pt)

#tagged("subject")[*#data.subject*]
"#;
    let data = serde_json::json!({ "subject": "Request for Quarters" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let subject: Vec<_> = regions.iter().filter(|r| r.field == "subject").collect();
    assert_eq!(
        subject.len(),
        1,
        "a tagged scalar placement surfaces one region: {regions:?}"
    );
    assert!(subject[0].rect[2] > subject[0].rect[0]);
}

#[test]
fn nested_same_field_tags_collapse_into_one_placement() {
    // A plate that wraps an already-auto-tagged verbatim value in an explicit
    // `tagged(..)` double-tags one placement; the scan depth-counts and emits
    // one region, not two overlapping ones.
    const YAML: &str = r#"
quill:
  name: nested_tags
  version: 0.1.0
  backend: typst
  description: nested same-field tag collapse test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: an auto-tagged value wrapped in an explicit tag
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, tagged
#set page(width: 612pt, height: 792pt, margin: 72pt)

#tagged("intro")[#data.intro]
"#;
    let data = serde_json::json!({ "intro": "A defensively double-tagged intro." });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(
        intro.len(),
        1,
        "nested same-field tags collapse into the outer placement: {regions:?}"
    );
}

#[test]
fn tagged_unknown_path_fails_the_compile() {
    const YAML: &str = r#"
quill:
  name: tagged_typo
  version: 0.1.0
  backend: typst
  description: tagged path validation test
typst:
  plate_file: plate.typ
main:
  fields:
    subject:
      type: string
      description: the only schema field
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, tagged
#tagged("subjcet")[#data.subject]
"#;
    let data = serde_json::json!({ "subject": "typo'd path" });

    let err = TypstBackend
        .open(&quill(YAML, PLATE), &data)
        .err()
        .expect("a typo'd tagged path must fail the compile");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("subjcet"),
        "the compile error names the bad path: {msg}"
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
fn card_path_prefix_composes_tagged_addresses() {
    // Each card dict carries its canonical `$path` prefix, so a plate tags a
    // card scalar without reimplementing the kind+ordinal grammar — and gets
    // the per-kind ordinal right where an absolute enumerate() index would
    // drift once kinds interleave.
    const YAML: &str = r#"
quill:
  name: card_path
  version: 0.1.0
  backend: typst
  description: card $path prefix test
typst:
  plate_file: plate.typ
main:
  fields: {}
card_kinds:
  alpha:
    description: alpha card
    fields:
      title:
        type: string
        description: a scalar card field
  beta:
    description: beta card
    fields:
      title:
        type: string
        description: a scalar card field
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, tagged
#set page(width: 612pt, height: 792pt, margin: 72pt)

#for card in data.at("$cards", default: ()) {
  tagged(card.at("$path") + "title")[#card.at("title")]
  parbreak()
}
"#;
    let data = serde_json::json!({
        "$cards": [
            {"$kind": "alpha", "title": "Alpha one"},
            {"$kind": "beta",  "title": "Beta one"},
            {"$kind": "alpha", "title": "Alpha two"},
        ],
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let fields: std::collections::HashSet<String> =
        session.regions().into_iter().map(|r| r.field).collect();
    for expected in [
        "$cards.alpha.0.title",
        "$cards.beta.0.title",
        "$cards.alpha.1.title",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }
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
