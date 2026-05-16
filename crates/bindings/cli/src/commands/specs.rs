use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::Quillmark;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
pub struct SpecsArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Output file path (optional)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

pub fn execute(args: SpecsArgs) -> Result<()> {
    // Validate quill path exists
    if !args.quill.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill directory not found: {}",
            args.quill.display()
        )));
    }

    // Load quill
    let engine = Quillmark::new();
    let quill = engine.quill_from_path(&args.quill)?;

    let blueprint = quill.source().config().blueprint();

    // Output
    if let Some(output_path) = args.output {
        fs::write(&output_path, blueprint).map_err(CliError::Io)?;
    } else {
        println!("{}", blueprint);
    }

    Ok(())
}
