//! End-to-end guard for the overlapping wrap+code emit fix (#846): a corpus
//! that an editor can build but markdown import never produces — `strong[0,4)`
//! partially overlapping `code[2,6)` — must lower to Typst that actually
//! *compiles*, not merely markup that "looks balanced". The emitter's output is
//! embedded in a `#let _qm_cN = [ .. ]` content block, so a single unclosed `[`
//! breaks the whole generated helper file's parse and fails the render. This
//! drives `Backend::open`, which builds the world and compiles.

use std::collections::HashMap;

use quillmark_core::{Backend, FileTreeNode, Quill};
use quillmark_richtext::model::{Line, LineKind, MarkKind, RichText};
use quillmark_typst::TypstBackend;

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

/// The canonical corpus JSON the render seam carries for a richtext field, built
/// from a hand-placed free-overlap corpus (normalize + validate happen inside
/// `to_canonical_value`).
fn overlap_corpus() -> serde_json::Value {
    use quillmark_richtext::model::Mark;
    let rt = RichText {
        text: "abcdef".to_string(),
        lines: vec![Line {
            kind: LineKind::Para,
            containers: vec![],
            continues: false,
        }],
        marks: vec![
            Mark {
                start: 0,
                end: 4,
                kind: MarkKind::Strong,
            },
            Mark {
                start: 2,
                end: 6,
                kind: MarkKind::Code,
            },
        ],
        islands: vec![],
    };
    // The overlap survives normalize/validate — there is no cross-kind overlap
    // invariant — so the seam carries it straight to the emitter.
    quillmark_richtext::serial::to_canonical_value(&rt)
}

const YAML: &str = r#"
quill:
  name: overlap_compile
  version: 0.1.0
  backend: typst
  description: overlapping wrap+code compile guard
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a paragraph with overlapping wrap and code marks
"#;

const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)

#data.body
"#;

#[test]
fn overlapping_wrap_and_code_compiles() {
    let data = serde_json::json!({ "body": overlap_corpus() });
    let session = TypstBackend
        .open(&quill(YAML, PLATE), &data)
        .expect("overlapping wrap+code corpus must compile");
    assert!(session.page_count() >= 1, "produced at least one page");
}
