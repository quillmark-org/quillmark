//! Ensures every quill in the fixtures quiver loads and renders its blueprint.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
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

        quill
            .render(
                &parsed,
                &RenderOptions {
                    output_format: Some(OutputFormat::Pdf),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("quill '{name}' failed to render: {e:?}"));
    }
}
