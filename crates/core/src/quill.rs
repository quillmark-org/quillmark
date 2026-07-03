//! The `Quill` type — portable, validated quill data.

mod blueprint;
mod compose;
mod config;
mod fill;
mod formats;
mod ignore;
mod load;
mod query;
mod schema;
mod schema_yaml;
mod seed;
mod tree;
mod types;
pub(crate) mod validation;

pub use config::{CoercionError, QuillConfig};
pub use fill::zero_value;
pub use formats::parse_date_ymd;
pub use ignore::QuillIgnore;
pub use schema::build_transform_schema;
pub use tree::FileTreeNode;
pub use types::{
    field_key, ui_key, BodyCardSchema, CardSchema, FieldSchema, FieldType, UiCardSchema,
    UiFieldSchema,
};

use std::collections::HashMap;

use crate::value::QuillValue;

/// The quill-config keys every binding surfaces as typed, top-level fields
/// (`name` via [`Quill::name`]; the rest via [`Quill::metadata`]). Bindings
/// exclude these from the "additional/unstructured metadata" passthrough so a
/// typed field is never emitted twice. Single source of truth for that set.
pub const STANDARD_METADATA_KEYS: &[&str] =
    &["name", "backend", "description", "version", "author"];

/// Portable, validated quill data: the file bundle, parsed config, and
/// metadata of an authored quill, tagged with its *declared* backend id.
///
/// A `Quill` holds no backend and needs no engine to construct or use. Every
/// method here is a pure read of its parsed config — parse / load / validate /
/// schema / seed / blueprint / compile. Rendering is the engine's job; see
/// `quillmark::Quillmark`. Construct with [`Quill::from_tree`] (pure) or
/// `quillmark::quill_from_path` (filesystem; fs stays out of core).
#[derive(Clone)]
pub struct Quill {
    pub(crate) metadata: HashMap<String, QuillValue>,
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
}

impl Quill {
    /// The quill's declared name.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// The backend identifier declared in Quill.yaml (e.g. `"typst"`).
    pub fn backend_id(&self) -> &str {
        &self.config.backend
    }

    /// Quill-specific metadata parsed from Quill.yaml.
    pub fn metadata(&self) -> &HashMap<String, QuillValue> {
        &self.metadata
    }

    /// The parsed schema configuration.
    pub fn config(&self) -> &QuillConfig {
        &self.config
    }

    /// The in-memory file tree for this quill.
    pub fn files(&self) -> &FileTreeNode {
        &self.files
    }

    /// Flatten this quill's file bundle into `(path, contents)` pairs — the
    /// inverse of [`Quill::from_tree`]'s input. `Quill::from_tree` of the
    /// result reproduces an equivalent quill (every file is preserved; empty
    /// directories are not — see [`FileTreeNode::flatten`]), so this is how a
    /// quill crosses a process or WASM linear-memory boundary as plain data.
    pub fn to_tree(&self) -> Vec<(String, Vec<u8>)> {
        self.files.flatten()
    }
}

impl std::fmt::Debug for Quill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Quill")
            .field("name", &self.config.name)
            .field("backend_id", &self.config.backend)
            .field("files", &"<FileTreeNode>")
            .finish()
    }
}

#[cfg(test)]
mod tests;
