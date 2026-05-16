use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use clap::Parser;
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
    let quill = load_quill(&args.quill)?;

    let blueprint = quill.source().config().blueprint();

    // Output
    if let Some(output_path) = args.output {
        fs::write(&output_path, blueprint).map_err(CliError::Io)?;
    } else {
        println!("{}", blueprint);
    }

    Ok(())
}
