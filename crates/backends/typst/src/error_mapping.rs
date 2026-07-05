//! Translates Typst diagnostics into Quillmark [`Diagnostic`](quillmark_core::Diagnostic) values.

use crate::world::QuillWorld;
use quillmark_core::{Diagnostic, Location, Severity};
use typst::diag::SourceDiagnostic;

/// Maps a slice of Typst diagnostics to Quillmark diagnostics.
pub fn map_typst_errors(errors: &[SourceDiagnostic], world: &QuillWorld) -> Vec<Diagnostic> {
    errors
        .iter()
        .map(|e| map_single_diagnostic(e, world))
        .collect()
}

fn map_single_diagnostic(error: &SourceDiagnostic, world: &QuillWorld) -> Diagnostic {
    let severity = match error.severity {
        typst::diag::Severity::Error => Severity::Error,
        typst::diag::Severity::Warning => Severity::Warning,
    };

    let location = resolve_span_to_location(error.span, world);

    // Content fields now ride generated markup **blocks**, not `eval` of an
    // ephemeral source, so an error in field content resolves to a real
    // position in the helper `lib.typ` — no synthetic "dynamically evaluated
    // content" hint is needed. Any hint Typst itself supplied is kept.
    let hint = error.hints.first().map(|h| h.v.to_string());

    // Extract error code from message (simple heuristic)
    let code = Some(format!(
        "typst::{}",
        error.message.split(':').next().unwrap_or("error").trim()
    ));

    Diagnostic {
        severity,
        code,
        message: error.message.to_string(),
        location,
        path: None,
        hint,
        source_chain: Vec::new(),
    }
}

fn resolve_span_to_location(span: typst::syntax::DiagSpan, world: &QuillWorld) -> Option<Location> {
    use typst::{World, WorldExt};

    // Resolve the span against its OWN source file. A diagnostic originating in
    // an injected helper package or a vendored package must report coordinates
    // (and a path) in that file, not in main.typ. Spans with no file id (the
    // detached span) fall back to main.
    let source_id = span.id().unwrap_or_else(|| world.main());
    let source = world.source(source_id).ok()?;
    let range = world.range(span)?;

    let text = source.text();
    let line = text[..range.start].matches('\n').count() + 1;
    let column = range.start - text[..range.start].rfind('\n').map_or(0, |pos| pos + 1) + 1;

    Some(Location {
        file: source.id().vpath().get_without_slash().to_string(),
        line: line as u32,
        column: column as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypstBackend;
    use quillmark_core::{Backend, FileTreeNode, OutputFormat, Quill, RenderOptions};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use typst::diag::SourceDiagnostic;
    use typst::syntax::Span;

    /// Walk the `usaf_memo@0.2.0` fixture into an in-memory tree, or `None` when
    /// the fixture is absent (a stripped checkout).
    fn walk_fixture() -> Option<FileTreeNode> {
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
        if !quill_path.exists() {
            return None;
        }
        Some(walk(&quill_path).expect("walk fixture"))
    }

    /// A `QuillWorld` with a valid main source to resolve spans against.
    fn fixture_world() -> Option<QuillWorld> {
        let tree = walk_fixture()?;
        let source = Quill::from_tree(tree).expect("load source");
        Some(QuillWorld::new(&source, "// Test").expect("create world"))
    }

    /// The host quill with its `plate.typ` replaced by `plate`; the fixture's
    /// `typst.plate_file: plate.typ` makes the backend read this override.
    fn source_with_plate(plate: &str) -> Option<Quill> {
        let mut tree = walk_fixture()?;
        if let FileTreeNode::Directory { files } = &mut tree {
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File {
                    contents: plate.as_bytes().to_vec(),
                },
            );
        }
        Some(Quill::from_tree(tree).expect("load source"))
    }

    /// An unresolvable span with no Typst-supplied hint carries none — the
    /// former eval-specific synthetic hint is retired now that content rides
    /// resolvable markup blocks rather than an ephemeral eval source.
    #[test]
    fn unresolvable_span_without_typst_hint_carries_no_hint() {
        let Some(world) = fixture_world() else {
            return;
        };

        let diag = SourceDiagnostic::error(Span::detached(), "unknown variable: general");
        let mapped = map_single_diagnostic(&diag, &world);

        assert!(
            mapped.location.is_none(),
            "detached span should not resolve to a location"
        );
        assert!(
            mapped.hint.is_none(),
            "no synthetic hint is injected: {:?}",
            mapped.hint
        );
        assert_eq!(mapped.message, "unknown variable: general");
    }

    /// A hint Typst already supplied is kept, not overwritten.
    #[test]
    fn unresolvable_span_keeps_existing_typst_hint() {
        let Some(world) = fixture_world() else {
            return;
        };

        let diag = SourceDiagnostic::error(Span::detached(), "unexpected closing bracket")
            .with_hint("try using a backslash escape: \\]");
        let mapped = map_single_diagnostic(&diag, &world);

        assert!(mapped.location.is_none());
        assert_eq!(
            mapped.hint.as_deref(),
            Some("try using a backslash escape: \\]"),
            "an existing Typst hint must not be overwritten"
        );
    }

    /// `eval`s an unknown variable; the error resolves to the call site in
    /// `main.typ`, so it is the resolvable common case.
    const EVAL_ERROR_PLATE: &str =
        "#set page(width: 400pt, height: 300pt)\n#eval(\"#general\", mode: \"markup\")\n";

    /// A resolvable eval error keeps its real source location: the error
    /// resolves to the call site, so the mapped diagnostic carries a location.
    /// (Issue #745; moved here from the retired `eval_error_hint.rs`.)
    #[test]
    fn resolvable_eval_error_is_unchanged() {
        let Some(source) = source_with_plate(EVAL_ERROR_PLATE) else {
            return;
        };

        // Compilation happens during `open`, so the error may surface from
        // either `open` or `render`.
        let diags = match TypstBackend.open(&source, &serde_json::json!({})) {
            Ok(session) => session
                .render(&RenderOptions {
                    output_format: Some(OutputFormat::Pdf),
                    ..Default::default()
                })
                .expect_err("eval of `#general` should fail to compile")
                .into_diagnostics(),
            Err(err) => err.into_diagnostics(),
        };
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
    }
}
