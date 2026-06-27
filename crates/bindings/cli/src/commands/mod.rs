pub mod blueprint;
pub mod info;
pub mod qualify;
pub mod render;
pub mod scaffold;
pub mod schema;
pub mod validate;

use crate::errors::{CliError, Result};
use quillmark::Quill;
use std::path::Path;

/// Load a quill from a directory path.
///
/// Upgrades the missing-path error to a clearer message before delegating to
/// [`quillmark::quill_from_path`]. The returned quill is portable, declarative
/// data; rendering is done through a [`quillmark::Quillmark`] engine.
pub fn load_quill(path: &Path) -> Result<Quill> {
    if !path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill directory not found: {}",
            path.display()
        )));
    }
    Ok(quillmark::quill_from_path(path)?)
}
