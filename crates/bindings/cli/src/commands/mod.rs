pub mod info;
pub mod render;
pub mod schema;
pub mod specs;
pub mod validate;

use crate::errors::{CliError, Result};
use quillmark::{Quill, Quillmark};
use std::path::Path;

/// Load a quill from a directory path.
///
/// Upgrades the engine's missing-path error to a clearer message before
/// delegating to `Quillmark::quill_from_path`.
pub fn load_quill(path: &Path) -> Result<Quill> {
    if !path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill directory not found: {}",
            path.display()
        )));
    }
    Ok(Quillmark::new().quill_from_path(path)?)
}
