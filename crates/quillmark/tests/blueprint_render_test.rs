//! Enforces the quill authoring contract from `prose/designs/BLUEPRINT.md`:
//! a quill's `plate.typ` must render the quill's own blueprint to a
//! successful (non-error) output.
//!
//! The blueprint is the type-minimal valid input — every required field
//! present, every value type-correct, enums at their first value. A plate
//! that renders it has shown it degrades gracefully on every type-valid
//! input shape.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderError, RenderOptions};
use quillmark_fixtures::quills_path;

/// Generate `quill_dir`'s blueprint and assert it renders without error.
fn assert_blueprint_renders(quill_dir: &str) {
    let engine = Quillmark::new();
    let quill = engine
        .quill_from_path(quills_path(quill_dir))
        .unwrap_or_else(|e| panic!("{quill_dir}: failed to load quill: {e:?}"));

    let blueprint = quill.source().config().blueprint();
    let parsed = Document::from_markdown(&blueprint)
        .unwrap_or_else(|e| panic!("{quill_dir}: blueprint must parse: {e:?}\n---\n{blueprint}"));

    let result = quill.render(
        &parsed,
        &RenderOptions {
            output_format: Some(OutputFormat::Pdf),
            ..Default::default()
        },
    );

    // Font-less CI environments cannot exercise the renderer; skip rather
    // than fail, matching the convention in quill_engine_test.rs.
    if let Err(RenderError::EngineCreation { diag }) = &result {
        if diag.message.contains("No fonts found") {
            eprintln!("{quill_dir}: skipping — no fonts available");
            return;
        }
    }

    let rendered = result.unwrap_or_else(|e| {
        panic!("{quill_dir}: plate.typ must render its own blueprint: {e:?}\n---\n{blueprint}")
    });
    assert!(
        !rendered.artifacts.is_empty(),
        "{quill_dir}: render produced no artifacts"
    );
}

#[test]
fn classic_resume_blueprint_renders() {
    assert_blueprint_renders("classic_resume");
}

#[test]
fn cmu_letter_blueprint_renders() {
    assert_blueprint_renders("cmu_letter");
}

#[test]
fn taro_blueprint_renders() {
    assert_blueprint_renders("taro");
}

#[test]
fn usaf_memo_blueprint_renders() {
    assert_blueprint_renders("usaf_memo");
}
