//! Quill loading and construction routines.
use crate::error::{Diagnostic, Severity};
use crate::value::QuillValue;

use super::{FileTreeNode, Quill, QuillConfig};

fn diag(message: impl Into<String>, code: &str) -> Diagnostic {
    Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string())
}

impl Quill {
    /// Create a Quill from a tree structure.
    ///
    /// This is the authoritative method for creating a Quill from an
    /// in-memory file tree. Filesystem walking belongs upstream (see
    /// `quillmark::quill_from_path`).
    ///
    /// # Arguments
    ///
    /// * `root` - The root node of the file tree
    ///
    /// # Errors
    ///
    /// Returns a non-empty `Vec<Diagnostic>` describing every problem found.
    /// When `Quill.yaml` itself contains multiple errors they are all
    /// reported together. Backend-specific assets (e.g. a Typst plate) are
    /// not read here — a backend resolves its own inputs at render time.
    pub fn from_tree(root: FileTreeNode) -> Result<Self, Vec<Diagnostic>> {
        let quill_yaml_bytes = root.get_file("Quill.yaml").ok_or_else(|| {
            vec![diag(
                "Quill.yaml not found in file tree",
                "quill::missing_file",
            )]
        })?;

        let quill_yaml_content = String::from_utf8(quill_yaml_bytes.to_vec()).map_err(|e| {
            vec![diag(
                format!("Quill.yaml is not valid UTF-8: {}", e),
                "quill::invalid_utf8",
            )]
        })?;

        // Parse YAML into QuillConfig — propagate the full diagnostic vector
        // so every Quill.yaml error reaches the caller.
        let (config, _warnings) = QuillConfig::from_yaml_with_warnings(&quill_yaml_content)?;

        Self::from_config(config, root)
    }

    /// Create a Quill from a QuillConfig and file tree.
    fn from_config(config: QuillConfig, root: FileTreeNode) -> Result<Self, Vec<Diagnostic>> {
        let mut metadata: std::collections::HashMap<String, QuillValue> =
            std::collections::HashMap::new();

        metadata.insert(
            "backend".to_string(),
            QuillValue::from_json(serde_json::Value::String(config.backend.clone())),
        );

        metadata.insert(
            "description".to_string(),
            QuillValue::from_json(serde_json::Value::String(config.description.clone())),
        );

        metadata.insert(
            "author".to_string(),
            QuillValue::from_json(serde_json::Value::String(config.author.clone())),
        );

        metadata.insert(
            "version".to_string(),
            QuillValue::from_json(serde_json::Value::String(config.version.clone())),
        );

        // Expose backend-specific config to metadata under `<backend>_<key>`.
        for (key, value) in &config.backend_config {
            metadata.insert(format!("{}_{}", config.backend, key), value.clone());
        }

        let source = Quill {
            metadata,
            config,
            files: root,
        };

        Ok(source)
    }
}
