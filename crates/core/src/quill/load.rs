//! QuillSource loading and construction routines.
use std::path::{Component, Path};

use crate::error::{Diagnostic, Severity};
use crate::value::QuillValue;

use super::{FileTreeNode, QuillConfig, QuillSource};

fn diag(message: impl Into<String>, code: &str) -> Diagnostic {
    Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string())
}

impl QuillSource {
    /// Create a QuillSource from a tree structure.
    ///
    /// This is the authoritative method for creating a QuillSource from an
    /// in-memory file tree. Filesystem walking belongs upstream (see
    /// `quillmark::Quillmark::quill_from_path`).
    ///
    /// # Arguments
    ///
    /// * `root` - The root node of the file tree
    ///
    /// # Errors
    ///
    /// Returns a non-empty `Vec<Diagnostic>` describing every problem found.
    /// When `Quill.yaml` itself contains multiple errors they are all
    /// reported together; subsequent failures (missing plate, malformed
    /// example) surface as single-element vectors.
    pub fn from_tree(root: FileTreeNode) -> Result<Self, Vec<Diagnostic>> {
        let quill_yaml_bytes = root
            .get_file("Quill.yaml")
            .ok_or_else(|| vec![diag("Quill.yaml not found in file tree", "quill::missing_file")])?;

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

    /// Create a QuillSource from a QuillConfig and file tree.
    fn from_config(
        mut config: QuillConfig,
        root: FileTreeNode,
    ) -> Result<Self, Vec<Diagnostic>> {
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

        // Read the plate content from plate file (if specified)
        let plate_content: Option<String> = if let Some(ref plate_file_name) = config.plate_file {
            let plate_bytes = root.get_file(plate_file_name).ok_or_else(|| {
                vec![diag(
                    format!("Plate file '{}' not found in file tree", plate_file_name),
                    "quill::plate_missing",
                )]
            })?;

            let content = String::from_utf8(plate_bytes.to_vec()).map_err(|e| {
                vec![diag(
                    format!("Plate file '{}' is not valid UTF-8: {}", plate_file_name, e),
                    "quill::invalid_utf8",
                )]
            })?;
            Some(content)
        } else {
            None
        };

        // Read the markdown example content if specified, or check for default "example.md"
        let example_content = if let Some(ref example_file_name) = config.example_file {
            let example_path = Path::new(example_file_name);
            if example_path.is_absolute()
                || example_path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
            {
                return Err(vec![diag(
                    format!(
                        "Example file '{}' is outside the quill directory",
                        example_file_name
                    ),
                    "quill::example_path_traversal",
                )]);
            }

            let bytes = root.get_file(example_file_name).ok_or_else(|| {
                vec![diag(
                    format!(
                        "Example file '{}' referenced in Quill.yaml not found",
                        example_file_name
                    ),
                    "quill::example_missing",
                )]
            })?;
            Some(String::from_utf8(bytes.to_vec()).map_err(|e| {
                vec![diag(
                    format!(
                        "Example file '{}' is not valid UTF-8: {}",
                        example_file_name, e
                    ),
                    "quill::invalid_utf8",
                )]
            })?)
        } else if root.file_exists("example.md") {
            let bytes = root
                .get_file("example.md")
                .expect("invariant violation: file_exists(example.md) but get_file returned None");
            Some(String::from_utf8(bytes.to_vec()).map_err(|e| {
                vec![diag(
                    format!("Default example file 'example.md' is not valid UTF-8: {}", e),
                    "quill::invalid_utf8",
                )]
            })?)
        } else {
            None
        };

        config.example_markdown = example_content.clone();

        let source = QuillSource {
            metadata,
            name: config.name.clone(),
            backend_id: config.backend.clone(),
            plate: plate_content,
            example: example_content,
            config,
            files: root,
        };

        Ok(source)
    }
}
