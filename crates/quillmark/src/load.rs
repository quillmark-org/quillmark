//! Filesystem loading for quills, kept in `quillmark` so that fs access stays
//! out of the fs-agnostic core. [`quill_from_path`] walks a directory into a
//! [`FileTreeNode`] and hands it to core's [`Quill::from_tree`], surfacing
//! config errors as a [`RenderError`].

use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};

use quillmark_core::{Diagnostic, FileTreeNode, Quill, QuillIgnore, RenderError, Severity};

/// Load a quill from a filesystem directory. Honours a root `.quillignore`,
/// else a default ignore set. (The fs walk lives here; core stays fs-agnostic.)
///
/// Pure config load — no backend, no engine; the declared backend is resolved
/// later, at render time. For an in-memory tree, call [`Quill::from_tree`].
pub fn quill_from_path<P: AsRef<Path>>(path: P) -> Result<Quill, RenderError> {
    let tree = load_tree_from_path(path.as_ref()).map_err(|e| RenderError::QuillConfig {
        diags: vec![
            Diagnostic::new(Severity::Error, format!("Failed to load quill: {}", e))
                .with_code("quill::load_failed".to_string()),
        ],
    })?;
    Quill::from_tree(tree).map_err(|diags| RenderError::QuillConfig { diags })
}

/// Walk a filesystem path into an in-memory [`FileTreeNode`].
///
/// Honours a `.quillignore` file at the root; otherwise applies a default
/// ignore set (`.git/`, `target/`, `node_modules/`, etc.).
fn load_tree_from_path(path: &Path) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    use std::fs;

    let quillignore_path = path.join(".quillignore");
    let ignore = if quillignore_path.exists() {
        let content = fs::read_to_string(&quillignore_path)
            .map_err(|e| format!("Failed to read .quillignore: {}", e))?;
        QuillIgnore::from_content(&content)
    } else {
        QuillIgnore::default()
    };

    load_dir(path, path, &ignore)
}

fn load_dir(
    current_dir: &Path,
    base_dir: &Path,
    ignore: &QuillIgnore,
) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    use std::fs;

    if !current_dir.exists() {
        return Ok(FileTreeNode::Directory {
            files: HashMap::new(),
        });
    }

    let mut files = HashMap::new();
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative_path: PathBuf = path
            .strip_prefix(base_dir)
            .map_err(|e| format!("Failed to get relative path: {}", e))?
            .to_path_buf();

        if ignore.is_ignored(&relative_path) {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid filename: {}", path.display()))?
            .to_string();

        if path.is_file() {
            let contents = fs::read(&path)
                .map_err(|e| format!("Failed to read file '{}': {}", path.display(), e))?;
            files.insert(filename, FileTreeNode::File { contents });
        } else if path.is_dir() {
            let subdir_tree = load_dir(&path, base_dir, ignore)?;
            files.insert(filename, subdir_tree);
        }
    }

    Ok(FileTreeNode::Directory { files })
}
