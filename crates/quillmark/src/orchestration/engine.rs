use quillmark_core::{
    Backend, Diagnostic, FileTreeNode, QuillIgnore, QuillSource, RenderError, Severity,
};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::Quill;

/// High-level engine for orchestrating backends and quills.
pub struct Quillmark {
    backends: HashMap<String, Arc<dyn Backend>>,
}

impl Quillmark {
    /// Create a new Quillmark with auto-registered backends based on enabled features.
    pub fn new() -> Self {
        let mut engine = Self {
            backends: HashMap::new(),
        };

        #[cfg(feature = "typst")]
        {
            engine.register_backend(Box::new(quillmark_typst::TypstBackend));
        }

        engine
    }

    /// Register a backend with the engine.
    pub fn register_backend(&mut self, backend: Box<dyn Backend>) {
        let id = backend.id().to_string();
        self.backends.insert(id, Arc::from(backend));
    }

    /// Build and return a render-ready quill from an in-memory file tree.
    pub fn quill(&self, tree: FileTreeNode) -> Result<Quill, RenderError> {
        let source =
            QuillSource::from_tree(tree).map_err(|diags| RenderError::QuillConfig { diags })?;
        self.assemble(source)
    }

    /// Load a quill from a filesystem path and attach the appropriate backend.
    pub fn quill_from_path<P: AsRef<Path>>(&self, path: P) -> Result<Quill, RenderError> {
        let tree = load_tree_from_path(path.as_ref()).map_err(|e| RenderError::QuillConfig {
            diags: vec![
                Diagnostic::new(Severity::Error, format!("Failed to load quill: {}", e))
                    .with_code("quill::load_failed".to_string()),
            ],
        })?;
        self.quill(tree)
    }

    fn assemble(&self, source: QuillSource) -> Result<Quill, RenderError> {
        let backend_id = source.backend_id();
        let backend =
            self.backends
                .get(backend_id)
                .ok_or_else(|| RenderError::UnsupportedBackend {
                    diag: Box::new(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Backend '{}' not registered or not enabled", backend_id),
                        )
                        .with_code("engine::backend_not_found".to_string())
                        .with_hint(format!(
                            "Available backends: {}",
                            self.backends.keys().cloned().collect::<Vec<_>>().join(", ")
                        )),
                    ),
                })?;
        Ok(Quill::new(Arc::new(source), Arc::clone(backend)))
    }

    /// Get a list of registered backend IDs.
    pub fn registered_backends(&self) -> Vec<&str> {
        self.backends.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
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
        QuillIgnore::new(vec![
            ".git/".to_string(),
            ".gitignore".to_string(),
            ".quillignore".to_string(),
            "target/".to_string(),
            "node_modules/".to_string(),
        ])
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
