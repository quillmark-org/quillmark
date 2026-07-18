//! End-to-end coverage for GFM tables (issue #880): a table in a document
//! `$body` renders through the full markdown -> Content -> Typst -> artifact
//! pipeline. The inline codec's unit tests pin `import(export(t))`; this pins
//! the *rendered* path — the Typst emitter's `#table(...)` lowering, exercised
//! with column alignment, formatted cells, and a ragged row that the model
//! layer normalizes before it reaches the backend.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::quills_path;

fn render(markdown: &str, format: OutputFormat) -> quillmark::RenderResult {
    let engine = Quillmark::new();
    let quill =
        quillmark::quill_from_path(quills_path("table_demo")).expect("table_demo should load");
    let parsed = Document::parse(markdown)
        .unwrap_or_else(|e| panic!("document failed to parse: {e:?}\n---\n{markdown}"))
        .document;
    engine
        .render(
            &quill,
            &parsed,
            &RenderOptions {
                output_format: Some(format),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("render failed: {e:?}\n---\n{markdown}"))
}

const FRONTMATTER: &str = "\
~~~card-yaml
$quill: table_demo@0.1.0
$kind: main
title: Table Demo
~~~
";

fn doc(body: &str) -> String {
    format!("{FRONTMATTER}\n{body}\n")
}

/// A body-borne pipe table — aligned columns, a formatted header cell, and a
/// short (ragged) row that normalization pads — renders through Typst without a
/// compile error and draws visibly more than the same document with no table.
///
/// A successful render is itself the load-bearing signal: a malformed
/// `#table(...)` lowering would fail Typst compilation and the helper would
/// panic. SVG vectorizes text into glyph paths, so the cell strings are not
/// literal substrings — the size comparison against an empty body is the
/// non-fragile proxy for "the table drew a grid and rows onto the page".
#[test]
fn table_body_renders_through_typst() {
    let table = render(
        &doc(
            "| Fruit | **Rank** | Note |\n\
             | :--- | :---: | ---: |\n\
             | Taro | 1 | best |\n\
             | Vanilla | 2 |",
        ),
        OutputFormat::Svg,
    );
    assert!(!table.artifacts.is_empty(), "render produced no artifacts");
    let table_svg = &table.artifacts[0].bytes;
    assert!(!table_svg.is_empty(), "rendered artifact is empty");

    // Control: the same document with a plain one-line body. The table version
    // adds cell grid strokes and three rows of text, so it is clearly larger.
    let plain = render(&doc("just a line"), OutputFormat::Svg);
    let plain_svg = &plain.artifacts[0].bytes;
    assert!(
        table_svg.len() > plain_svg.len(),
        "table body ({} bytes) did not draw more than an empty body ({} bytes)",
        table_svg.len(),
        plain_svg.len()
    );
}

/// The table also renders to PDF (a distinct Typst export path) without error.
#[test]
fn table_body_renders_to_pdf() {
    let result = render(
        &doc("| a | b |\n| --- | --- |\n| 1 | 2 |"),
        OutputFormat::Pdf,
    );
    assert!(
        result.artifacts.first().is_some_and(|a| !a.bytes.is_empty()),
        "table did not render to a non-empty PDF"
    );
}
