use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::Quillmark;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
pub struct SchemaArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill_path: PathBuf,

    /// Output file path (optional)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Include form-builder ui hints (group, order, compact, multiline,
    /// hide_body, default_title). Default emits the structural schema only.
    #[arg(long)]
    with_ui: bool,
}

pub fn execute(args: SchemaArgs) -> Result<()> {
    // Validate quill path exists
    if !args.quill_path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill directory not found: {}",
            args.quill_path.display()
        )));
    }

    // Load Quill
    let engine = Quillmark::new();
    let quill = engine.quill_from_path(&args.quill_path)?;

    let config = quill.source().config();
    let schema_yaml = if args.with_ui {
        config.form_schema_yaml()
    } else {
        config.schema_yaml()
    }
    .map_err(|e| CliError::InvalidArgument(format!("Failed to serialize schema: {}", e)))?;

    // Output
    if let Some(output_path) = args.output {
        fs::write(&output_path, schema_yaml).map_err(CliError::Io)?;
    } else {
        println!("{}", schema_yaml);
    }

    Ok(())
}
