//! Acceptance tests for `LiveSession::apply` — the incremental edit verb of
//! a live preview (#778).
//!
//! The session persists its `QuillWorld`; `apply` swaps new document data into
//! the helper package, recompiles, and reports the dirty page set. Commit is
//! transactional: a failed recompile leaves every read serving the last-good
//! compile.

use std::collections::HashMap;

use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
use quillmark_typst::TypstBackend;
use serde_json::json;

const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.at("msg")
"#;

fn quill() -> Quill {
    let yaml = b"quill:\n  name: live\n  version: 0.1.0\n  backend: typst\n  description: apply acceptance quill\n\ntypst:\n  plate_file: plate.typ\n\nmain:\n  fields:\n    msg:\n      description: message\n      type: string\n";
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: PLATE.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
}

/// `n` sentences; `marker` appended after sentence `edit_at`.
fn msg(n: usize, edit_at: Option<usize>, marker: &str) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str("Sentence ");
        s.push_str(&i.to_string());
        s.push_str(" lorem ipsum dolor sit amet consectetur adipiscing elit. ");
        if edit_at == Some(i) {
            s.push_str(marker);
            s.push(' ');
        }
    }
    s
}

#[test]
fn apply_commits_and_dirties_only_the_touched_suffix() {
    let backend = TypstBackend;
    let q = quill();
    let n = 60;

    let mut session = backend
        .open(&q, &json!({ "msg": msg(n, None, "") }))
        .expect("open");
    let pages = session.page_count();
    assert!(pages >= 3, "fixture must span several pages, got {pages}");

    // An end edit leaves every earlier page's content (and spans) intact:
    // the dirty set is exactly the last page.
    let cs = session
        .apply(&json!({ "msg": msg(n, Some(n - 1), "EDITED") }))
        .expect("apply");
    assert_eq!(cs.page_count, pages);
    assert_eq!(cs.dirty_pages, vec![pages - 1]);

    // A front edit dirties the first page (and possibly shifted successors).
    let cs = session
        .apply(&json!({ "msg": msg(n, Some(0), "EDITED") }))
        .expect("apply");
    assert!(cs.dirty_pages.contains(&0), "dirty: {:?}", cs.dirty_pages);

    // An identical re-apply changes nothing.
    let cs = session
        .apply(&json!({ "msg": msg(n, Some(0), "EDITED") }))
        .expect("apply");
    assert!(cs.dirty_pages.is_empty(), "dirty: {:?}", cs.dirty_pages);
}

/// A quill whose sole content-bearing field is a *markdown* field placed
/// through the span-tracked helper path (`#data.body`), not a scalar reference
/// into the static plate. This is the shape #801 was reported against: content
/// fields route glyph spans into the helper `lib.typ`, which is regenerated per
/// `apply` — the frame data `page_hashes` must fingerprint without folding in
/// those spans.
fn markdown_quill() -> Quill {
    const YAML: &str = r#"quill:
  name: live_markdown
  version: 0.1.0
  backend: typst
  description: markdown-content no-op reapply quill
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a markdown body
"#;
    const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.body
"#;
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: YAML.as_bytes().to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: PLATE.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
}

#[test]
fn identical_reapply_of_markdown_content_is_clean() {
    // #801: a page's fingerprint must not fold in glyph/shape/image `Span`s
    // (source-location metadata, not pixels), so reapplying byte-identical
    // markdown to a content-field session reports NOTHING dirty — every time,
    // including a second consecutive no-op (not a one-time settling artifact).
    let backend = TypstBackend;
    let q = markdown_quill();
    let body = "This is a **markdown** paragraph that renders some real ink. ".repeat(3);

    let mut session = backend.open(&q, &json!({ "body": body })).expect("open");
    let pages = session.page_count();
    assert!(pages >= 1);

    for round in 0..3 {
        let cs = session
            .apply(&json!({ "body": body }))
            .expect("apply identical");
        assert_eq!(cs.page_count, pages);
        assert!(
            cs.dirty_pages.is_empty(),
            "round {round}: identical markdown reapply must be clean, got {:?}",
            cs.dirty_pages
        );
    }

    // A real content change still dirties — the fingerprint didn't go blind.
    let cs = session
        .apply(&json!({ "body": format!("{body} plus a genuinely new sentence.") }))
        .expect("apply changed");
    assert!(
        !cs.dirty_pages.is_empty(),
        "a real edit must still dirty a page"
    );
}

/// A two-content-field quill whose plate places both fields, so the generated
/// helper `lib.typ` carries both a data literal and two content blocks.
fn two_field_quill() -> Quill {
    const YAML: &str = r#"quill:
  name: live_two_field
  version: 0.1.0
  backend: typst
  description: two markdown fields
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a markdown body
    note:
      type: richtext
      description: a markdown note
"#;
    const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#data.body

#data.note
"#;
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: YAML.as_bytes().to_vec(),
        },
    );
    files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: PLATE.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
}

#[test]
fn reapply_with_reordered_fields_same_content_is_clean() {
    // The web-app's #801 `[0]`, reproduced at the backend. `serde_json` is built
    // with `preserve_order`, so field insertion order survives on the wire; an
    // editor's mutate path can hand `apply` a document with the SAME content but
    // a different field order than `open` saw. Two independent layers keep that
    // reorder clean, and this pins their conjunction end-to-end: the helper
    // codegen emits dicts in canonical (sorted-key) order, so a reorder-only
    // apply produces byte-identical `lib.typ` (unit-pinned by
    // `reordered_input_emits_byte_identical_source`), and `page_hashes` excludes
    // source-location `Span`s, so even a byte-layout shift cannot dirty a page
    // whose ink didn't move (unit-pinned by
    // `page_hashes_ignore_span_shift_when_ink_is_identical`).
    let backend = TypstBackend;
    let q = two_field_quill();

    // Same values, opposite key order.
    let opened: serde_json::Value = serde_json::from_str(
        r#"{"body":"**Body** paragraph with real ink.","note":"A note with ink too."}"#,
    )
    .unwrap();
    let reordered: serde_json::Value = serde_json::from_str(
        r#"{"note":"A note with ink too.","body":"**Body** paragraph with real ink."}"#,
    )
    .unwrap();

    let mut session = backend.open(&q, &opened).expect("open");
    let cs = session.apply(&reordered).expect("apply reordered");
    assert!(
        cs.dirty_pages.is_empty(),
        "same content in a different field order moved no ink; got dirty {:?}",
        cs.dirty_pages
    );

    // And a genuine edit through the same reordered document still dirties.
    let mut edited = reordered.clone();
    edited["body"] = json!("**Body** paragraph with real ink, now extended further.");
    let cs = session.apply(&edited).expect("apply edited");
    assert!(!cs.dirty_pages.is_empty(), "a real edit must still dirty");
}

#[test]
fn apply_is_transactional_on_compile_failure() {
    let backend = TypstBackend;
    let q = quill();

    let mut session = backend
        .open(&q, &json!({ "msg": "last good" }))
        .expect("open");
    let pages = session.page_count();

    // No `msg` key → the plate's `data.at("msg")` fails at eval.
    let err = session.apply(&json!({})).expect_err("compile must fail");
    assert!(!err.diagnostics().is_empty());

    // Every read still serves the last-good compile.
    assert_eq!(session.page_count(), pages);
    session
        .render(&RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        })
        .expect("render serves last-good");

    // The session recovers on the next good apply.
    let cs = session
        .apply(&json!({ "msg": "recovered" }))
        .expect("apply after failure");
    assert_eq!(cs.page_count, session.page_count());
}

#[test]
fn apply_tracks_page_count_growth_and_shrink() {
    let backend = TypstBackend;
    let q = quill();

    let mut session = backend
        .open(&q, &json!({ "msg": msg(4, None, "") }))
        .expect("open");
    let small = session.page_count();

    let cs = session
        .apply(&json!({ "msg": msg(120, None, "") }))
        .expect("grow");
    assert!(cs.page_count > small);
    assert_eq!(cs.page_count, session.page_count());
    // Added pages are dirty.
    assert!(cs.dirty_pages.contains(&(cs.page_count - 1)));

    let cs = session
        .apply(&json!({ "msg": msg(4, None, "") }))
        .expect("shrink");
    assert_eq!(cs.page_count, small);
    assert_eq!(cs.page_count, session.page_count());
    // Removed pages are implied by page_count; dirty never exceeds it.
    assert!(cs.dirty_pages.iter().all(|&p| p < cs.page_count));
}
