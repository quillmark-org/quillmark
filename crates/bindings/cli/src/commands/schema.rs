use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
pub struct SchemaArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Output file path (optional)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

pub fn execute(args: SchemaArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;

    let config = quill.source().config();
    let schema_yaml = config
        .schema_yaml()
        .map_err(|e| CliError::InvalidArgument(format!("Failed to serialize schema: {}", e)))?;

    // Output
    if let Some(output_path) = args.output {
        fs::write(&output_path, schema_yaml).map_err(CliError::Io)?;
    } else {
        println!("{}", schema_yaml);
    }

    Ok(())
}
