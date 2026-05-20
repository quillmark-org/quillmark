//! Enforces the quill authoring contract from `prose/canon/BLUEPRINT.md`:
//! every quill in the fixtures quiver loads, and its `plate.typ` renders the
//! quill's own blueprint to a successful (non-error) output.
//!
//! The blueprint is the type-minimal valid input — every required field
//! present, every value type-correct, enums at their first value. A plate
//! that renders it has shown it degrades gracefully on every type-valid
//! input shape.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderError, RenderOptions};
use quillmark_fixtures::{quills_path, resource_path};
use std::fs;

fn quiver_quills() -> Vec<String> {
    let quills_dir = resource_path("quills");
    let mut names: Vec<String> = fs::read_dir(&quills_dir)
        .expect("quills directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

#[test]
fn every_quill_in_quiver_renders() {
    let engine = Quillmark::new();

    for name in quiver_quills() {
        let quill = engine
            .quill_from_path(quills_path(&name))
            .unwrap_or_else(|e| panic!("quill '{name}' failed to load: {e:?}"));

        let markdown = quill.source().config().blueprint();
        let parsed = Document::from_markdown(&markdown)
            .unwrap_or_else(|e| panic!("quill '{name}' blueprint failed to parse: {e:?}"));

        let result = quill.render(
            &parsed,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        );

        // Font-less CI environments cannot exercise the renderer; skip rather
        // than fail, matching the convention in quill_engine_test.rs.
        if let Err(RenderError::EngineCreation { diags }) = &result {
            if diags[0].message.contains("No fonts found") {
                eprintln!("quill '{name}': skipping — no fonts available");
                continue;
            }
        }

        let rendered = result
            .unwrap_or_else(|e| panic!("quill '{name}' failed to render: {e:?}\n---\n{markdown}"));
        assert!(
            !rendered.artifacts.is_empty(),
            "quill '{name}': render produced no artifacts"
        );
    }
}
