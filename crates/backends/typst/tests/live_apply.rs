//! Acceptance tests for `RenderSession::apply` — the incremental edit verb of
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
