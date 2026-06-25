//! Enforces the quill authoring contract from `prose/canon/BLUEPRINT.md`:
//! every quill in the fixtures quiver loads, and its `plate.typ` renders an
//! **empty document** (just `$quill` / `$kind: main`, no fields) to a
//! successful (non-error) output.
//!
//! Under zero-filled render (see `prose/canon/SCHEMAS.md`),
//! every absent field is filled with its type-empty (zero) value in the plate
//! projection. An empty document is therefore the type-minimal valid input —
//! the worst-case-but-renderable shape — so a plate that renders it has shown
//! it degrades gracefully on every type-valid input.
//!
//! A second test (`every_quill_blueprint_round_trips_and_renders`) additionally
//! generates each quill's `blueprint()`, round-trips it, and renders it — the
//! BLUEPRINT.md §Guarantees contract.

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
        let quill = quillmark::quill_from_path(quills_path(&name))
            .unwrap_or_else(|e| panic!("quill '{name}' failed to load: {e:?}"));

        // An empty document — zero-filled render fills every absent field with
        // its type-empty value in the plate projection.
        let config = quill.config();
        let markdown = format!(
            "~~~\n$quill: {}@{}\n$kind: main\n~~~\n",
            config.name, config.version
        );
        let parsed = Document::from_markdown(&markdown).unwrap_or_else(|e| {
            panic!("quill '{name}' empty document failed to parse: {e:?}\n---\n{markdown}")
        });

        let result = engine.render(
            &quill,
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

/// The `blueprint()` guarantee (BLUEPRINT.md §Guarantees): every bundled quill's
/// generated blueprint **parses**, **round-trips** idempotently, and **renders**
/// (its `!must_fill` markers zero-fill). Covers the typed-table synthetic-row
/// round-trip and the strengthened "blueprint renders" claim across fixtures.
#[test]
fn every_quill_blueprint_round_trips_and_renders() {
    let engine = Quillmark::new();

    for name in quiver_quills() {
        let quill = quillmark::quill_from_path(quills_path(&name))
            .unwrap_or_else(|e| panic!("quill '{name}' failed to load: {e:?}"));

        let bp = quill.config().blueprint();
        let doc1 = Document::from_markdown(&bp).unwrap_or_else(|e| {
            panic!("quill '{name}' blueprint failed to parse: {e:?}\n---\n{bp}")
        });
        let doc2 = Document::from_markdown(&doc1.to_markdown())
            .unwrap_or_else(|e| panic!("quill '{name}' blueprint re-emit failed to parse: {e:?}"));
        assert_eq!(doc1, doc2, "quill '{name}': blueprint must round-trip");

        let result = engine.render(
            &quill,
            &doc1,
            &RenderOptions {
                output_format: Some(OutputFormat::Pdf),
                ..Default::default()
            },
        );
        if let Err(RenderError::EngineCreation { diags }) = &result {
            if diags[0].message.contains("No fonts found") {
                continue;
            }
        }
        result.unwrap_or_else(|e| {
            panic!("quill '{name}' blueprint failed to render: {e:?}\n---\n{bp}")
        });
    }
}
