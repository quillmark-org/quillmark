//! Quill source bundle types and implementations.

mod blueprint;
mod config;
mod formats;
mod ignore;
mod load;
mod query;
mod schema;
mod schema_yaml;
mod tree;
mod types;
pub(crate) mod validation;

pub use config::{CoercionError, QuillConfig};
pub use ignore::QuillIgnore;
pub use schema::build_transform_schema;
pub use tree::FileTreeNode;
pub use types::{
    field_key, ui_key, BodyLeafSchema, FieldSchema, FieldType, LeafSchema, UiFieldSchema,
    UiLeafSchema,
};

use std::collections::HashMap;

use crate::value::QuillValue;

/// A quill source bundle — pure data parsed from an authored quill directory.
///
/// A `QuillSource` is the file-bundle, config, and metadata; it has no rendering
/// ability. The engine composes a `QuillSource` with a resolved backend into a
/// renderable `Quill` (see `quillmark::Quill`).
#[derive(Clone)]
pub struct QuillSource {
    pub(crate) metadata: HashMap<String, QuillValue>,
    pub(crate) name: String,
    pub(crate) backend_id: String,
    pub(crate) plate: Option<String>,
    pub(crate) example: Option<String>,
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
}

impl QuillSource {
    /// The quill's declared name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The backend identifier declared in Quill.yaml (e.g. `"typst"`).
    pub fn backend_id(&self) -> &str {
        &self.backend_id
    }

    /// Quill-specific metadata parsed from Quill.yaml.
    pub fn metadata(&self) -> &HashMap<String, QuillValue> {
        &self.metadata
    }

    /// The plate template content, if the quill declares one.
    pub fn plate(&self) -> Option<&str> {
        self.plate.as_deref()
    }

    /// The example Markdown content, if the quill ships one.
    pub fn example(&self) -> Option<&str> {
        self.example.as_deref()
    }

    /// The parsed schema configuration.
    pub fn config(&self) -> &QuillConfig {
        &self.config
    }

    /// The in-memory file tree for this quill.
    pub fn files(&self) -> &FileTreeNode {
        &self.files
    }
}

impl std::fmt::Debug for QuillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuillSource")
            .field("name", &self.name)
            .field("backend_id", &self.backend_id)
            .field(
                "plate",
                &self.plate.as_ref().map(|s| format!("<{} bytes>", s.len())),
            )
            .field("example", &self.example.is_some())
            .field("files", &"<FileTreeNode>")
            .finish()
    }
}

#[cfg(test)]
mod tests;
