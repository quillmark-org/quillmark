use crate::errors::Result;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Handles writing output to file or stdout
pub struct OutputWriter {
    use_stdout: bool,
    output_path: Option<PathBuf>,
    quiet: bool,
}

impl OutputWriter {
    pub fn new(use_stdout: bool, output_path: Option<PathBuf>, quiet: bool) -> Self {
        Self {
            use_stdout,
            output_path,
            quiet,
        }
    }

    /// Write bytes to the configured output destination
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        if self.use_stdout {
            io::stdout().write_all(bytes)?;
            Ok(())
        } else if let Some(path) = &self.output_path {
            self.write_to_file(path, bytes)?;
            if !self.quiet {
                println!("Output written to: {}", path.display());
            }
            Ok(())
        } else {
            Err(crate::errors::CliError::InvalidArgument(
                "No output path configured and stdout output not selected".to_string(),
            ))
        }
    }

    /// Write bytes to a file, creating parent directories if needed
    fn write_to_file(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, bytes)?;
        Ok(())
    }
}

/// Derive output filename from input markdown path
pub fn derive_output_path(markdown_path: &Path, format: &str) -> PathBuf {
    let mut output = markdown_path.to_path_buf();
    output.set_extension(format);
    output
}
