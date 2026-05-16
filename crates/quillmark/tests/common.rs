//! # Common Test Utilities
//!
//! Shared test helpers and utilities for integration tests.
//!
//! ## Purpose
//!
//! This module provides common functionality used across multiple test files:
//! - **`demo()` function** - Centralized blueprint plumbing for rendering demos
//!
//! ## Usage
//!
//! The `demo()` helper simplifies the common pattern of:
//! 1. Loading a quill from a path
//! 2. Using the quill's generated blueprint markdown
//! 3. Rendering to final output
//! 4. Writing outputs to example directory

use quillmark_fixtures::{example_output_dir, quills_path, write_example_output};
use std::error::Error;

/// Demo helper that centralizes blueprint plumbing.
///
/// It loads the quill and uses its generated blueprint, then renders it.
pub fn demo(
    quill_dir: &str,
    render_output: &str,
    use_resource_path: bool,
) -> Result<(), Box<dyn Error>> {
    // quill path (folder)
    let quill_path = if use_resource_path {
        quillmark_fixtures::resource_path(quill_dir)
    } else {
        quills_path(quill_dir)
    };
    // Default engine flow used by examples: Typst backend via the engine-provided quill.
    let engine = quillmark::Quillmark::new();
    let quill = engine
        .quill_from_path(quill_path.clone())
        .expect("Failed to load quill");

    // Use the quill's generated blueprint as the markdown document.
    let markdown = quill.source().config().blueprint();

    // Parse the markdown once
    let parsed = quillmark::Document::from_markdown(&markdown)?;

    // render output
    let rendered = quill.render(
        &parsed,
        &quillmark_core::RenderOptions {
            output_format: Some(quillmark_core::OutputFormat::Pdf),
            ..Default::default()
        },
    )?;
    let output_bytes = rendered.artifacts[0].bytes.clone();

    write_example_output(render_output, &output_bytes)?;

    println!("------------------------------");
    println!(
        "Access render output: {}",
        example_output_dir().join(render_output).display()
    );

    Ok(())
}
