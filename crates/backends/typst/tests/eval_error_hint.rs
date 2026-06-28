//! Issue #745: a Typst error whose span *does* resolve must be left unchanged
//! by the eval-hint fallback.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
use quillmark_typst::TypstBackend;

fn host_tree() -> FileTreeNode {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p: PathBuf = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.2.0");
    walk(&quill_path).expect("walk fixture")
}

/// Build the host quill with its `plate.typ` replaced by `plate`; the fixture's
/// `typst.plate_file: plate.typ` makes the backend read this override.
fn source_with_plate(plate: &str) -> Quill {
    let mut tree = host_tree();
    if let FileTreeNode::Directory { files } = &mut tree {
        files.insert(
            "plate.typ".to_string(),
            FileTreeNode::File {
                contents: plate.as_bytes().to_vec(),
            },
        );
    }
    Quill::from_tree(tree).expect("load source")
}

/// `eval`s an unknown variable; the error resolves to the call site in
/// `main.typ`, so it is the resolvable common case.
const EVAL_ERROR_PLATE: &str =
    "#set page(width: 400pt, height: 300pt)\n#eval(\"#general\", mode: \"markup\")\n";

/// Compilation happens during `open`, so the error may surface from either
/// `open` or `render`.
fn diagnostics_for(plate: &str) -> Vec<quillmark_core::Diagnostic> {
    match TypstBackend.open(&source_with_plate(plate), &serde_json::json!({})) {
        Ok(session) => session
            .render(&RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            })
            .expect_err("eval of `#general` should fail to compile")
            .into_diagnostics(),
        Err(err) => err.into_diagnostics(),
    }
}

/// A resolvable diagnostic keeps its location and does not get the generic hint.
#[test]
fn resolvable_eval_error_is_unchanged() {
    let diags = diagnostics_for(EVAL_ERROR_PLATE);
    assert!(
        !diags.is_empty(),
        "compilation error must carry diagnostics"
    );

    let diag = diags
        .iter()
        .find(|d| d.message.contains("unknown variable: general"))
        .expect("expected the `unknown variable: general` diagnostic");

    assert!(
        diag.location.is_some(),
        "this eval error resolves to the call site; expected a location, got None"
    );
    assert!(
        diag.hint
            .as_deref()
            .is_none_or(|h| !h.contains("dynamically evaluated content")),
        "a resolvable diagnostic must not receive the generic eval hint, got: {:?}",
        diag.hint
    );
}
