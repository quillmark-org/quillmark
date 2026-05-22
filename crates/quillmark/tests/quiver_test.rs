//! Enforces the quill authoring contract from `prose/canon/BLUEPRINT.md`:
//! every quill in the fixtures quiver loads, and its `plate.typ` renders the
//! quill's own blueprint (with every `<must-fill>` cell replaced by a
//! type-empty value) to a successful (non-error) output.
//!
//! The blueprint, after sentinel substitution, is the type-minimal valid
//! input — every field present, every value type-correct, enums at their
//! first value. A plate that renders it has shown it degrades gracefully
//! on every type-valid input shape.

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

/// Replace every `<must-fill>` sentinel in a blueprint with a type-empty
/// value derived from the inline annotation on the same line. For enum
/// fields the substitution uses the first enum value so validation
/// accepts the result.
///
/// This mirrors the operation an MCP consumer would perform on a
/// blueprint before shipping: walk Must Fill cells, replace them with
/// concrete values. Here we use the leanest possible values to test
/// that `plate.typ` accepts type-empty inputs per the authoring
/// contract.
fn fill_must_fill(blueprint: &str) -> String {
    let mut out = String::new();
    for line in blueprint.lines() {
        out.push_str(&substitute_line(line));
        out.push('\n');
    }
    out
}

fn substitute_line(line: &str) -> String {
    // Inline form: `<key>: <must-fill>  # <annotation>`.
    const NEEDLE: &str = ": <must-fill>  # ";
    if let Some(idx) = line.find(NEEDLE) {
        let prefix = &line[..idx];
        let annotation = &line[idx + NEEDLE.len()..];
        let value = type_empty_for(annotation);
        return format!("{}: {}  # {}", prefix, value, annotation);
    }
    // Markdown block scalar form: `<indent><must-fill>` on its own line.
    if line.trim_start() == "<must-fill>" {
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        return indent;
    }
    line.to_string()
}

fn type_empty_for(annotation: &str) -> String {
    let head = annotation.split(';').next().unwrap_or(annotation).trim();
    if head.starts_with("array<") || head == "array" {
        return "[]".to_string();
    }
    if head == "integer" || head == "number" {
        return "0".to_string();
    }
    if head == "boolean" {
        return "false".to_string();
    }
    if let Some(inner) = head
        .strip_prefix("enum<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let first = inner.split('|').next().unwrap_or("").trim();
        return first.to_string();
    }
    // string, markdown (handled via the block-scalar branch above), date,
    // datetime, object: empty string. Date / datetime accept "" by design.
    "\"\"".to_string()
}

#[test]
fn every_quill_in_quiver_renders() {
    let engine = Quillmark::new();

    for name in quiver_quills() {
        let quill = engine
            .quill_from_path(quills_path(&name))
            .unwrap_or_else(|e| panic!("quill '{name}' failed to load: {e:?}"));

        let blueprint = quill.source().config().blueprint();
        let markdown = fill_must_fill(&blueprint);
        let parsed = Document::from_markdown(&markdown).unwrap_or_else(|e| {
            panic!("quill '{name}' blueprint failed to parse: {e:?}\n---\n{markdown}")
        });

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
